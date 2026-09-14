//! The container reap against a REAL zombie-leader survivor, as the only process tree of a fresh
//! container.
//!
//! `#[ignore]`d, and refused outside a fresh container, because it runs the PRODUCTION reaper
//! ([`maxplayer_core::delivery_orchestrator::reap_other_processes`]). That function SIGKILLs every
//! process except pid 1 and its caller. On a developer host that is the developer's session. Run it
//! only through the script, which builds in one container and runs in a second, fresh one:
//!
//! ```text
//! ./scripts/reap-isolated-test.sh
//! ```
//!
//! The shape the script takes, by hand:
//!
//! ```text
//! cargo test -p maxplayer-core --features acp,gateway,git-delivery,wallet --locked \
//!   --test reap_isolated_linux --no-run                      # prints the executable path
//! docker run --rm --init -v <target dir>:/target:ro -e MAXPLAYER_REAP_TEST_ISOLATED=1 \
//!   rust:1-bookworm /target/debug/deps/reap_isolated_linux-<hash> \
//!   --ignored --test-threads=1 --nocapture
//! ```
//!
//! **Why this exists.** The unit test
//! `delivery_orchestrator::tests::a_zombie_leader_with_a_live_sibling_thread_is_reported_live` shows
//! that the enumerator REPORTS such a group live. It kills nothing, so it cannot show that the reaper
//! then kills the group and proves the container empty. This test runs the whole reap — the real
//! `/proc`, the real `kill`, the real retry loop — against the survivor shape of the attack: a
//! detached helper in its own session, whose initial thread exited while a worker thread lives.
//!
//! **Isolation, three guards.** Each test refuses to run unless:
//! 1. `MAXPLAYER_REAP_TEST_ISOLATED=1` is set;
//! 2. pid 1 is not `systemd` or `launchd`;
//! 3. no thread group other than pid 1 and this process exists in `/proc` at the start.
//!
//! The third guard is the positive proof. A fresh `docker run --init` container holds `docker-init`
//! and this binary and nothing else; a host never looks like that.
//!
//! **Ordering.** The reap needs no marker to be tested. `deliver_in_container` calls `reap()?` before
//! `write_agent_done_marker`, so a reap that fails returns before any marker exists, and a reap that
//! succeeds is complete before the marker invites a token in. The unit test
//! `entry_fails_closed_when_the_reap_cannot_prove_the_container_empty` proves that order with an
//! injected reap; this test proves the reap itself.
#![cfg(target_os = "linux")]

use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use maxplayer_core::delivery_orchestrator::reap_other_processes;

/// The variable that permits a reap here. Set by `scripts/reap-isolated-test.sh` on the run container.
const ISOLATED_ENV: &str = "MAXPLAYER_REAP_TEST_ISOLATED";
/// The variable that turns a re-executed copy of this binary into the zombie-leader helper.
const ZOMBIE_HELPER_ENV: &str = "MAXPLAYER_TEST_ZOMBIE_HELPER";
/// The libtest name of [`zombie_leader_helper_entry`] in this binary.
const ZOMBIE_HELPER_TEST: &str = "zombie_leader_helper_entry";

/// Both reaping tests hold this lock: two reaps at once would kill each other's helper.
static REAP_LOCK: Mutex<()> = Mutex::new(());

/// Take [`REAP_LOCK`]. The lock guards no data, so a lock that a failed test poisoned is still a
/// lock; the control must not fail because the other test did.
fn hold_reap_lock() -> std::sync::MutexGuard<'static, ()> {
    REAP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// NOT a test of anything. This is the body of the helper process that the reaping test re-executes
// this binary into. Without the environment variable it returns at once and passes. With it, it
// never returns; the reap, or the parent test, kills the process.
#[test]
fn zombie_leader_helper_entry() {
    if std::env::var(ZOMBIE_HELPER_ENV).as_deref() != Ok("1") {
        return;
    }
    become_zombie_leader_with_live_sibling();
}

/// Make this process a thread group whose LEADER is a zombie while another thread lives: the shape a
/// job can leave behind (a detached helper whose initial thread calls `pthread_exit`).
///
/// libtest runs every test body on a spawned thread, so the initial thread of the process — the
/// group leader — is libtest's, parked on a channel. This thread installs a `SIGUSR1` handler that
/// exits only the thread it runs on (`exit`, not `exit_group`), and sends the signal to the leader
/// with `tgkill`. The kernel keeps the leader as a zombie because the group is not empty, so
/// `/proc/<pid>/status` reads `Z` while this thread and a worker thread sleep. Never returns.
fn become_zombie_leader_with_live_sibling() -> ! {
    extern "C" fn exit_this_thread_only(_signal: libc::c_int) {
        // SAFETY: a raw `exit` syscall ends only the calling thread and touches no memory of ours.
        // It is async-signal-safe.
        unsafe {
            libc::syscall(libc::SYS_exit, 0);
        }
    }
    // A second live thread besides this one, so "a sibling lives" does not rest on libtest.
    std::thread::spawn(|| std::thread::sleep(Duration::from_secs(120)));
    // SAFETY: plain libc calls with valid arguments. The handler has the signature `signal` expects,
    // and `tgkill` targets the leader of this process (tid == tgid).
    unsafe {
        let handler: extern "C" fn(libc::c_int) = exit_this_thread_only;
        let handler = handler as libc::sighandler_t;
        assert_ne!(libc::signal(libc::SIGUSR1, handler), libc::SIG_ERR);
        let tgid = libc::getpid();
        assert_eq!(
            libc::syscall(libc::SYS_tgkill, tgid, tgid, libc::SIGUSR1),
            0
        );
    }
    loop {
        std::thread::sleep(Duration::from_secs(120));
    }
}

/// The `State:` character of one `/proc/…/status` file, read by the test itself and not by the code
/// under test. `None` when the file is gone or has no `State:` line.
fn observed_state(status_path: &str) -> Option<char> {
    std::fs::read_to_string(status_path)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("State:"))
        .and_then(|rest| rest.trim().chars().next())
}

/// `(tid, state)` of every task of `tgid`, read by the test itself. Empty when the group is gone.
fn observed_tasks(tgid: u32) -> Vec<(u32, char)> {
    let Ok(entries) = std::fs::read_dir(format!("/proc/{tgid}/task")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|tid| {
            let state = observed_state(&format!("/proc/{tgid}/task/{tid}/status"))?;
            Some((tid, state))
        })
        .collect()
}

/// The session id of `pid`, from field 6 of `/proc/<pid>/stat` (the fields after the `(comm)`).
fn session_of(pid: u32) -> Option<i32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_comm = stat.rsplit_once(')')?.1;
    after_comm.split_whitespace().nth(3)?.parse().ok()
}

/// `/proc/1/comm`, trimmed.
fn init_comm() -> String {
    std::fs::read_to_string("/proc/1/comm")
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// Every thread group in `/proc` except pid 1 and this process, read by the test itself.
fn foreign_groups() -> Vec<u32> {
    let me = std::process::id();
    std::fs::read_dir("/proc")
        .expect("list /proc")
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| *pid != 1 && *pid != me)
        .collect()
}

/// Refuse to run anywhere but as the only process tree of a fresh container.
fn assert_isolated() {
    assert_eq!(
        std::env::var(ISOLATED_ENV).as_deref(),
        Ok("1"),
        "{ISOLATED_ENV}=1 is not set. This test runs the production reaper, which SIGKILLs every \
         process but pid 1 and itself. Run it through scripts/reap-isolated-test.sh."
    );
    let init = init_comm();
    assert!(
        !matches!(init.as_str(), "systemd" | "launchd"),
        "pid 1 is {init}: this is a host, not a fresh container; refusing to reap"
    );
    let foreign = foreign_groups();
    assert!(
        foreign.is_empty(),
        "thread groups {foreign:?} exist besides pid 1 and this process: this is not a fresh \
         container; refusing to reap"
    );
}

/// The helper process: a re-execution of this binary, detached into its own session. Killed and
/// reaped on drop, so a failed assertion leaves nothing behind.
struct ZombieHelper(Option<Child>);

impl ZombieHelper {
    fn spawn_detached() -> Self {
        let exe = std::env::current_exe().expect("this test binary has a path");
        let mut command = Command::new(exe);
        command
            .args([ZOMBIE_HELPER_TEST, "--exact", "--test-threads=1"])
            .env(ZOMBIE_HELPER_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // SAFETY: `setsid` is async-signal-safe and touches no memory of ours. It runs in the child
        // between fork and exec, so the helper starts in its own session, outside every process group
        // this test owns — the detached shape of the attack.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command
            .spawn()
            .expect("re-execute this test binary as the detached helper");
        Self(Some(child))
    }

    fn tgid(&self) -> u32 {
        self.0.as_ref().expect("the helper is live").id()
    }

    /// Wait until the leader is `Z` and another task is not `Z`/`X`. The test reads both facts
    /// itself. Returns the observed leader state and task list. Panics with the helper's output when
    /// the scenario does not appear in time.
    fn await_zombie_leader_with_live_sibling(
        &mut self,
        timeout: Duration,
    ) -> (char, Vec<(u32, char)>) {
        let tgid = self.tgid();
        let started = Instant::now();
        loop {
            let leader = observed_state(&format!("/proc/{tgid}/status"));
            let tasks = observed_tasks(tgid);
            let sibling_lives = tasks
                .iter()
                .any(|(tid, state)| *tid != tgid && !matches!(state, 'Z' | 'X'));
            if leader == Some('Z') && sibling_lives {
                return ('Z', tasks);
            }
            if started.elapsed() > timeout {
                let output = self.kill_and_reap();
                panic!(
                    "the helper did not become a zombie leader with a live sibling within \
                     {timeout:?}: leader {leader:?}, tasks {tasks:?}; helper stdout {:?}, stderr {:?}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// SIGKILL the whole group and reap it.
    fn kill_and_reap(&mut self) -> Output {
        let mut child = self.0.take().expect("the helper is live");
        let _ = child.kill();
        child.wait_with_output().expect("reap the helper")
    }

    /// Reap the helper WITHOUT a kill: only correct once the reap under test killed it. Blocks until
    /// the group is empty, so call it only after the `/proc` check.
    fn wait(&mut self) -> ExitStatus {
        let mut child = self.0.take().expect("the helper is live");
        child.wait().expect("reap the helper")
    }
}

impl Drop for ZombieHelper {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

// Control: nothing but pid 1 and this process exists, and the reap says so.
#[test]
#[ignore = "runs the production reaper; only as the sole process tree of a fresh container (scripts/reap-isolated-test.sh)"]
fn the_reap_reports_an_empty_container_ok() {
    let _lock = hold_reap_lock();
    assert_isolated();
    reap_other_processes().expect("an empty container is proven empty");
    println!(
        "control: reap_other_processes() = Ok(()) with no helper; pid 1 is {:?}, this process is {}",
        init_comm(),
        std::process::id()
    );
}

// The attack: a detached helper in its own session, its leader a zombie, a worker thread alive. The
// reap must kill the worker and return Ok. RED ON REVERT to the leader-only `State:` test: the reap
// returns Ok without a kill, and the worker survives it.
#[test]
#[ignore = "runs the production reaper; only as the sole process tree of a fresh container (scripts/reap-isolated-test.sh)"]
fn the_reap_kills_a_zombie_leader_group_whose_sibling_thread_lives() {
    let _lock = hold_reap_lock();
    assert_isolated();
    let mut helper = ZombieHelper::spawn_detached();
    let tgid = helper.tgid();
    let (leader, tasks) = helper.await_zombie_leader_with_live_sibling(Duration::from_secs(5));
    // SAFETY: `getsid(0)` reads the session of the calling process and touches no memory of ours.
    let my_session = unsafe { libc::getsid(0) };
    let helper_session = session_of(tgid);
    println!(
        "before the reap: /proc/{tgid}/status State: {leader}; tasks {tasks:?}; helper session \
         {helper_session:?}, test session {my_session}"
    );
    assert_eq!(leader, 'Z', "the helper's leader is a zombie");
    assert!(
        tasks
            .iter()
            .any(|(tid, state)| *tid != tgid && !matches!(state, 'Z' | 'X')),
        "a sibling thread of the helper lives: {tasks:?}"
    );
    assert_ne!(
        helper_session,
        Some(my_session),
        "the helper is detached into its own session"
    );

    let result = reap_other_processes();

    let after = observed_tasks(tgid);
    let gone = !Path::new(&format!("/proc/{tgid}")).exists();
    println!(
        "after the reap: result {result:?}; /proc/{tgid} exists: {}; tasks {after:?}",
        !gone
    );
    result.expect("the reap proves the container empty once the group is dead");
    assert!(
        gone || after.iter().all(|(_, state)| matches!(state, 'Z' | 'X')),
        "the helper's worker thread survived the reap: tasks {after:?}"
    );
    // This process is the helper's parent: reap the zombie leader the SIGKILL left for us.
    let status = helper.wait();
    println!("helper exit status after our wait: {status:?}");
    assert!(
        !Path::new(&format!("/proc/{tgid}")).exists(),
        "the helper is gone once its parent reaped it"
    );
}
