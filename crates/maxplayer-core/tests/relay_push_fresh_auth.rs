//! The delivery push, over real smart HTTP: one token per HTTP leg, the exact approved commit, and
//! the remote's own per-ref answer as the only proof of acceptance.
//!
//! What these tests hold down, against a local git-over-HTTPS fixture (no live infrastructure):
//!
//! - **Fresh authorization at every leg.** A delivery push waits for the seat's one push lock before
//!   its first byte. A token minted before that wait is already aging when the relay checks it, so
//!   the transport mints at the leg instead: the advertisement and the pack POST each carry their
//!   own token, signed at the moment that request goes out, and a second attempt mints again.
//! - **Correct ref and destination per job.** Every token is scoped to the one ref its push writes
//!   and bound to that job's remote; a leg aimed anywhere else gets no token at all.
//! - **The exact approved OID.** The push names the commit by value, so a local branch that moves
//!   after the gate cannot change what is delivered.
//! - **No silent success.** A rejected ref fails. And nothing is re-read from the remote afterwards:
//!   the recorded request list ends at the pack POST.
#![cfg(unix)]

mod git_http_fixture;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use git_http_fixture::GitHttpAuthServer;
use maxplayer_core::git_transport::{
    AuthMinter, TransportError, delivery_ref, nip98_authorization_header_scoped_with_keys,
    push_exact_oid,
};

static NEXT: AtomicU64 = AtomicU64::new(0);
static ENV_INIT: Once = Once::new();

fn temp(label: &str) -> PathBuf {
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "maxplayer-push-fresh-auth-{label}-{}-{id}",
        std::process::id()
    ))
}

/// `GIT_SSL_NO_VERIFY` so the in-process transport accepts the fixture's self-signed cert (the same
/// env var production honors), and a loopback proxy bypass so an ambient `https_proxy` cannot
/// swallow the fixture. Staged before the first HTTP client is built in this process.
fn init_env() {
    ENV_INIT.call_once(|| {
        // SAFETY (edition 2024 set_var): every test funnels through this Once before any client is
        // built, so no reader observes a partial update.
        unsafe {
            std::env::set_var("GIT_SSL_NO_VERIFY", "1");
            std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
            std::env::set_var("no_proxy", "127.0.0.1,localhost");
        }
    });
}

fn git_in(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed in {}", dir.display());
}

fn git_stdout(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed in {}",
        dir.display()
    );
    String::from_utf8(out.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

/// A seller workdir with one commit on a delivery branch. Returns (workdir, branch, commit oid).
fn source_repo(label: &str, branch: &str) -> (PathBuf, String) {
    let dir = temp(label);
    fs::create_dir_all(&dir).expect("workdir");
    git_in(&dir, &["init", &format!("--initial-branch={branch}")]);
    git_in(&dir, &["config", "user.name", "Delivery Agent"]);
    git_in(&dir, &["config", "user.email", "agent@example.invalid"]);
    // Unique content per repo: two repos built in the same second with identical trees would
    // produce the SAME commit oid, which would quietly defeat the tests that need distinct history.
    fs::write(
        dir.join("WORK.md"),
        format!("delivered work from {}\n", dir.display()),
    )
    .expect("write");
    git_in(&dir, &["add", "-A"]);
    git_in(&dir, &["commit", "-m", "delivery"]);
    let oid = git_stdout(&dir, &["rev-parse", "HEAD"]);
    (dir, oid)
}

/// Add one more commit on the current branch and return its oid.
fn commit_more(dir: &Path, content: &str) -> String {
    fs::write(dir.join("LATER.md"), content).expect("write");
    git_in(dir, &["add", "-A"]);
    git_in(dir, &["commit", "-m", "later local work"]);
    git_stdout(dir, &["rev-parse", "HEAD"])
}

/// The relay-git destination repo the fixture serves.
fn bare_dest(label: &str) -> PathBuf {
    let dir = temp(label);
    fs::create_dir_all(&dir).expect("bare dir");
    git_in(&dir, &["init", "--bare", "--initial-branch=main"]);
    dir
}

fn mount_for(owner_byte: &str) -> String {
    format!("/git/{}/delivery.git", owner_byte.repeat(32))
}

#[derive(Clone, Debug)]
struct MintRecord {
    destination: String,
    header: String,
    at: Instant,
}

/// A minter shaped like the seller node's: bound to ONE destination, scoped to ONE ref, signing a
/// fresh token per call. `delay` stands in for the signer round-trip, and makes each leg's token
/// land in a different `created_at` second so freshness is observable on the wire.
fn recording_minter(
    bound_remote: &str,
    scope_ref: &str,
    delay: Duration,
) -> (AuthMinter, Arc<Mutex<Vec<MintRecord>>>) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&records);
    let bound = bound_remote.trim_end_matches('/').to_owned();
    let scope = scope_ref.to_owned();
    let keys = nostr_sdk::Keys::generate();
    let minter: AuthMinter = Arc::new(move |destination: &str| {
        if destination.trim_end_matches('/') != bound {
            return Err(format!(
                "refusing to authorize a leg to {destination}: bound to {bound}"
            ));
        }
        std::thread::sleep(delay);
        let header = nip98_authorization_header_scoped_with_keys(destination, &keys, Some(&scope))
            .map_err(|error| error.to_string())?;
        sink.lock().expect("mint lock").push(MintRecord {
            destination: destination.to_owned(),
            header: header.clone(),
            at: Instant::now(),
        });
        Ok(header)
    });
    (minter, records)
}

fn event_of(header: &str) -> nostr_sdk::Event {
    use base64::Engine as _;
    use nostr_sdk::JsonUtil;
    let encoded = header.strip_prefix("Nostr ").expect("Nostr scheme");
    let json = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .expect("base64");
    nostr_sdk::Event::from_json(&json).expect("event json")
}

fn tag_of(event: &nostr_sdk::Event, kind: &str) -> Option<String> {
    event
        .tags
        .iter()
        .find(|t| t.kind() == nostr_sdk::TagKind::custom(kind))
        .and_then(|t| t.content().map(str::to_owned))
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

fn wire(server: &GitHttpAuthServer) -> Vec<String> {
    server
        .requests()
        .iter()
        .map(|r| format!("{} {}", r.method, r.target))
        .collect()
}

/// A push that waits — for the delivery lock, then again inside each leg's signing — still presents
/// authorization minted for THAT leg, and presents a different token on every leg. Nothing is read
/// back from the remote after the pack POST.
#[test]
fn every_leg_of_a_delayed_push_carries_its_own_fresh_token() {
    init_env();
    let branch = "maxplayer/aa11bb22";
    let (workdir, approved) = source_repo("fresh-src", branch);
    let bare = bare_dest("fresh-dest");
    let mount = mount_for("ab");
    let server = GitHttpAuthServer::spawn(&bare, &mount);
    let url = server.repo_url();
    let (minter, mints) =
        recording_minter(&url, &delivery_ref(branch), Duration::from_millis(1100));

    // Stand in for the serialized delivery-push lock: the wait happens BEFORE the first byte, and a
    // token minted here (the pre-mint shape) would already be this old when the relay saw it.
    let before_wait = now_secs();
    std::thread::sleep(Duration::from_millis(1500));
    let after_wait = now_secs();
    assert!(
        after_wait > before_wait,
        "the wait crossed a second boundary"
    );

    let delivered =
        push_exact_oid(&workdir, &url, branch, &approved, Some(minter)).expect("push accepted");
    assert_eq!(delivered, approved);

    let mints = mints.lock().expect("mint lock").clone();
    assert!(
        mints.len() >= 2,
        "one mint per HTTP leg (advertisement + pack POST), got {}",
        mints.len()
    );
    let events: Vec<nostr_sdk::Event> = mints.iter().map(|m| event_of(&m.header)).collect();

    let ids: HashSet<String> = events.iter().map(|e| e.id.to_hex()).collect();
    assert_eq!(ids.len(), events.len(), "no token was reused across legs");

    for (record, event) in mints.iter().zip(&events) {
        event.verify().expect("token verifies");
        assert_eq!(event.kind.as_u16(), 27235, "NIP-98 kind");
        assert_eq!(
            tag_of(event, "ref").as_deref(),
            Some(delivery_ref(branch).as_str()),
            "scoped to the one ref this push writes"
        );
        assert_eq!(
            tag_of(event, "u").as_deref(),
            Some(url.as_str()),
            "bound to this job's destination"
        );
        assert_eq!(record.destination, url);
        assert!(
            event.created_at.as_u64() >= after_wait,
            "token minted AFTER the lock wait, not before it"
        );
    }

    let first = events.first().expect("first leg").created_at.as_u64();
    let last = events.last().expect("last leg").created_at.as_u64();
    assert!(
        last > first,
        "a later leg minted a later token ({first} -> {last})"
    );

    // The wire agrees, and shows no extra leg: advertisement, pack POST, nothing after.
    assert_eq!(
        wire(&server),
        vec![
            format!("GET {mount}/info/refs?service=git-receive-pack"),
            format!("POST {mount}/git-receive-pack"),
        ],
        "exactly the two push legs — no post-push remote read-back"
    );
    let auths: Vec<String> = server
        .requests()
        .iter()
        .map(|r| r.authorization.clone().expect("every leg is authorized"))
        .collect();
    assert_ne!(
        auths[0], auths[1],
        "the POST did not reuse the advertisement's token"
    );

    // And the delivery landed: the remote ref is the approved commit.
    assert_eq!(
        git_stdout(&bare, &["rev-parse", &delivery_ref(branch)]),
        approved
    );
}

/// A second attempt at the same push mints again — no token survives from the first attempt.
#[test]
fn a_repeated_attempt_mints_new_tokens_rather_than_reusing_the_first() {
    init_env();
    let branch = "maxplayer/cc33dd44";
    let (workdir, approved) = source_repo("retry-src", branch);
    let bare = bare_dest("retry-dest");
    let mount = mount_for("cd");
    let server = GitHttpAuthServer::spawn(&bare, &mount);
    let url = server.repo_url();
    let (minter, mints) =
        recording_minter(&url, &delivery_ref(branch), Duration::from_millis(1100));

    push_exact_oid(&workdir, &url, branch, &approved, Some(Arc::clone(&minter))).expect("first");
    let first_legs: Vec<MintRecord> = mints.lock().expect("mint lock").clone();
    let first_round: HashSet<String> = first_legs
        .iter()
        .map(|m| event_of(&m.header).id.to_hex())
        .collect();
    assert!(!first_round.is_empty());

    push_exact_oid(&workdir, &url, branch, &approved, Some(minter)).expect("second");
    let all: Vec<MintRecord> = mints.lock().expect("mint lock").clone();
    let second_round: Vec<nostr_sdk::Event> = all[first_legs.len()..]
        .iter()
        .map(|m| event_of(&m.header))
        .collect();

    assert!(
        !second_round.is_empty(),
        "the second attempt minted its own tokens"
    );
    for event in &second_round {
        assert!(
            !first_round.contains(&event.id.to_hex()),
            "a second-attempt leg reused a token from the first attempt"
        );
    }
}

/// The commit the gate approved is the commit that ships, even when the local branch has moved on.
#[test]
fn the_approved_oid_is_delivered_after_the_local_branch_moves() {
    init_env();
    let branch = "maxplayer/ee55ff66";
    let (workdir, approved) = source_repo("moved-src", branch);
    let bare = bare_dest("moved-dest");
    let mount = mount_for("ef");
    let server = GitHttpAuthServer::spawn(&bare, &mount);
    let url = server.repo_url();
    let (minter, _mints) = recording_minter(&url, &delivery_ref(branch), Duration::ZERO);

    // Between the gate and the push, something else commits in this workdir.
    let moved = commit_more(&workdir, "written after the gate\n");
    assert_ne!(moved, approved, "the local branch really moved");

    let delivered = push_exact_oid(&workdir, &url, branch, &approved, Some(minter)).expect("push");

    assert_eq!(delivered, approved);
    assert_eq!(
        git_stdout(&bare, &["rev-parse", &delivery_ref(branch)]),
        approved,
        "the remote holds the APPROVED commit, not the moved branch tip"
    );
    assert_eq!(
        git_stdout(&workdir, &["rev-parse", "HEAD"]),
        moved,
        "the local branch is untouched by the push"
    );
}

/// Two jobs, one remote: each push writes its own ref and each token names its own ref.
#[test]
fn each_job_pushes_its_own_ref_with_a_token_scoped_to_it() {
    init_env();
    let bare = bare_dest("perjob-dest");
    let mount = mount_for("9a");
    let server = GitHttpAuthServer::spawn(&bare, &mount);
    let url = server.repo_url();

    let mut delivered = Vec::new();
    for branch in ["maxplayer/11112222", "maxplayer/33334444"] {
        let (workdir, approved) = source_repo("perjob-src", branch);
        let (minter, mints) = recording_minter(&url, &delivery_ref(branch), Duration::ZERO);
        push_exact_oid(&workdir, &url, branch, &approved, Some(minter)).expect("push");

        for record in mints.lock().expect("mint lock").iter() {
            let event = event_of(&record.header);
            assert_eq!(
                tag_of(&event, "ref").as_deref(),
                Some(delivery_ref(branch).as_str()),
                "token scoped to THIS job's ref"
            );
            assert_eq!(tag_of(&event, "u").as_deref(), Some(url.as_str()));
        }
        delivered.push((branch, approved));
    }

    for (branch, approved) in delivered {
        assert_eq!(
            git_stdout(&bare, &["rev-parse", &delivery_ref(branch)]),
            approved,
            "{branch} holds its own job's commit"
        );
    }
}

/// A remote that refuses the ref update fails the push — the rejection is read from the remote's
/// per-ref status, with no read-back needed to discover it.
#[test]
fn a_rejected_ref_update_fails_the_push() {
    init_env();
    let branch = "maxplayer/7777aaaa";
    let (workdir, approved) = source_repo("rejected-src", branch);
    let bare = bare_dest("rejected-dest");

    // The remote accepts the pack and then declines the ref update — the shape that only shows up in
    // the per-ref report-status, which is exactly the answer this push is required to read.
    let hook = bare.join("hooks/pre-receive");
    fs::create_dir_all(bare.join("hooks")).expect("hooks dir");
    fs::write(
        &hook,
        "#!/bin/sh\necho 'delivery ref is closed' >&2\nexit 1\n",
    )
    .expect("hook");
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&hook).expect("hook meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&hook, perms).expect("hook exec bit");

    let server = GitHttpAuthServer::spawn(&bare, &mount_for("7a"));
    let url = server.repo_url();
    let (minter, _mints) = recording_minter(&url, &delivery_ref(branch), Duration::ZERO);

    let error = push_exact_oid(&workdir, &url, branch, &approved, Some(minter))
        .expect_err("a refused ref update must fail the push");
    assert!(
        matches!(&error, TransportError::Rejected(message) if message.contains(&delivery_ref(branch))),
        "the failure names the refused ref: {error:?}"
    );
    assert!(
        git_stdout(&bare, &["for-each-ref", "--format=%(refname)"]).is_empty(),
        "the refused update left no ref behind"
    );
}

/// A leg aimed at anything but this job's remote gets no token, so the push fails before any pack
/// is uploaded.
#[test]
fn a_leg_to_another_destination_is_never_authorized() {
    init_env();
    let branch = "maxplayer/beef0001";
    let (workdir, approved) = source_repo("bound-src", branch);
    let bare = bare_dest("bound-dest");
    let mount = mount_for("be");
    let server = GitHttpAuthServer::spawn(&bare, &mount);
    let url = server.repo_url();

    // Bound to a DIFFERENT repo on the same relay: every leg to this server is refused a token.
    let (minter, mints) = recording_minter(
        "https://relay.example/git/other/other.git",
        &delivery_ref(branch),
        Duration::ZERO,
    );

    let error = push_exact_oid(&workdir, &url, branch, &approved, Some(minter))
        .expect_err("an unauthorized destination must fail");
    assert!(
        format!("{error}").contains("mint authorization")
            || matches!(error, TransportError::Io(_) | TransportError::Transport(_)),
        "the failure is the refused mint: {error:?}"
    );
    assert!(
        mints.lock().expect("mint lock").is_empty(),
        "no token was ever signed for the wrong destination"
    );
    assert!(
        !wire(&server).iter().any(|line| line.starts_with("POST")),
        "no pack was uploaded: {:?}",
        wire(&server)
    );
    assert!(
        git_stdout(&bare, &["for-each-ref", "--format=%(refname)"]).is_empty(),
        "the remote has no refs"
    );
}
