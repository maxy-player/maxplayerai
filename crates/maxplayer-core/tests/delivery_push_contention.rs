//! Two awarded jobs, one delivery remote, one lock — and the question the narrow change exists to
//! answer: when does the SECOND delivery's push token get signed?
//!
//! This is the shape the seat actually runs in. [`super::serialized_bounded_push`] serializes
//! deliveries to this seat's one remote, so a job that arrives while another is pushing waits — for
//! the whole of the first job's advertisement, pack POST and whatever the relay took to answer. A
//! token minted when the job was picked up would spend that entire wait ageing, and would be at its
//! oldest exactly when the relay finally checks it. Minted per request, from inside the transport,
//! it is signed on the far side of the wait instead.
//!
//! Everything here is the production article: the real serialized wrapper
//! ([`maxplayer_core::seller_node::run::serialized_bounded_push`]), the real signer actor (the
//! seller key never leaves it), the real delivery wrapper, and the real HTTPS git fixture recording
//! the exact `Authorization` bytes each request arrived with. The only test-owned part is the
//! recording wrapper around the minter, which timestamps each mint so the wait can be measured
//! rather than assumed.
//!
//! Its own test binary, not a unit test, for a concrete reason: the transport's HTTP client is built
//! once per process, baking in whether it accepts this fixture's self-signed certificate. Only a
//! process this test owns can stage that trust before anything else builds the client.
#![cfg(all(unix, feature = "git-delivery"))]

mod git_http_fixture;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use git_http_fixture::{FixtureOptions, GitHttpAuthServer};
use maxplayer_core::git_transport::{self, AuthMinter};
use maxplayer_core::home::bootstrap;
use maxplayer_core::seller_git;
use maxplayer_core::seller_node::run::{DELIVERY_PUSH_TIMEOUT, serialized_bounded_push};
use maxplayer_core::seller_node::signer::{self, SignerHandle};

static ENV_INIT: Once = Once::new();

/// Stage the process env once, before the transport's shared HTTP client exists.
fn init_test_env() {
    ENV_INIT.call_once(|| {
        // SAFETY (edition 2024 set_var): every test in this binary funnels through this Once before
        // it touches the transport; racing test threads block in call_once until the env is staged.
        unsafe {
            std::env::set_var("GIT_SSL_NO_VERIFY", "1");
            std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
            std::env::set_var("no_proxy", "127.0.0.1,localhost");
        }
    });
}

/// How long the fixture holds the first delivery's first leg. Long enough that the second delivery
/// demonstrably waits behind it, and that a re-mint is unmistakable on a one-second `created_at`.
const HELD_LEG: Duration = Duration::from_millis(2_500);

/// One mint the push asked for: when, for which destination, and the exact bytes handed back.
#[derive(Clone, Debug)]
struct Minted {
    at: Instant,
    destination: String,
    header: String,
}

fn temp(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "maxplayer-delivery-contention-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("mkdir root");
    root
}

/// A committed workdir for one job: a single commit on that job's delivery branch.
fn job_workdir(root: &Path, name: &str, branch: &str) -> (PathBuf, String) {
    let workdir = root.join(name);
    let repo = git2::Repository::init(&workdir).expect("init workdir");
    std::fs::write(
        workdir.join("deliverable.txt"),
        format!("work from {name}\n"),
    )
    .expect("write");
    let mut index = repo.index().expect("index");
    index.add_path(Path::new("deliverable.txt")).expect("add");
    index.write().expect("write index");
    let tree = repo
        .find_tree(index.write_tree().expect("tree"))
        .expect("find tree");
    let sig = git2::Signature::new("s", "s@example.invalid", &git2::Time::new(1_700_000_000, 0))
        .expect("sig");
    let oid = repo
        .commit(
            Some(&git_transport::delivery_ref(branch)),
            &sig,
            &sig,
            "delivery",
            &tree,
            &[],
        )
        .expect("commit");
    (workdir, oid.to_string())
}

/// The minter the delivery push runs with, built exactly as `execute` builds it — bound to this
/// job's remote, scoped to this job's ref, signed through the actor, bounded by the push deadline —
/// wrapped so the test can see when each mint happened.
fn production_shaped_minter(
    signer: SignerHandle,
    remote: &str,
    scope: &str,
    deadline: Instant,
    authority: Arc<AtomicBool>,
) -> (AuthMinter, Arc<Mutex<Vec<Minted>>>) {
    let log: Arc<Mutex<Vec<Minted>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let intended = remote.to_owned();
    let scope = scope.to_owned();
    let minter: AuthMinter = Arc::new(move |destination: &str| {
        if !git_transport::same_destination(&intended, destination) {
            return Err(format!(
                "refusing to authorize a leg to {destination}: this delivery is bound to {intended}"
            ));
        }
        if !authority.load(Ordering::SeqCst) {
            return Err("this delivery's push authority has ended".to_owned());
        }
        if Instant::now() >= deadline {
            return Err("this delivery's push deadline has passed".to_owned());
        }
        let header = signer.http_auth_header_blocking(
            destination.to_owned(),
            Some(scope.clone()),
            deadline,
        )?;
        sink.lock().expect("log").push(Minted {
            at: Instant::now(),
            destination: destination.to_owned(),
            header: header.clone(),
        });
        Ok(header)
    });
    (minter, log)
}

/// Decode the NIP-98 event JSON out of an `Authorization: Nostr <base64>` header.
fn token_json(header: &str) -> String {
    use base64::Engine as _;
    let b64 = header.strip_prefix("Nostr ").expect("Nostr scheme");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .expect("base64");
    String::from_utf8(bytes).expect("utf8")
}

/// The second delivery's tokens are signed AFTER it stopped waiting for the first — through the real
/// lock, the real actor and the real wire.
///
/// Red-on-revert: mint the token before `serialized_bounded_push` (the pre-minted-header shape) and
/// the second job's tokens exist before the first job's push even finished, failing the ordering
/// assertion; make `HttpStream::send` reuse one token per operation and the per-leg distinctness
/// assertions fail with it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_delivery_that_waited_for_the_lock_signs_after_the_wait() {
    init_test_env();
    let root = temp("two-jobs");
    let first_branch = "maxplayer/aaaa1111";
    let second_branch = "maxplayer/bbbb2222";
    let (first_workdir, first_oid) = job_workdir(&root, "job-a", first_branch);
    let (second_workdir, second_oid) = job_workdir(&root, "job-b", second_branch);

    // One delivery remote for the seat, holding its first response so the second job visibly waits.
    let relay_repo = root.join("relay.git");
    git2::Repository::init_bare(&relay_repo).expect("relay bare");
    let relay = GitHttpAuthServer::spawn_with(
        &relay_repo,
        "/git/seller/r.git",
        FixtureOptions {
            first_response_delay: Some(HELD_LEG),
            ..FixtureOptions::default()
        },
    );
    let url = relay.repo_url();

    // The real signer actor: one seller key, owned by the actor, shared by both deliveries.
    let home_root = root.join("home");
    let home = bootstrap(&home_root).expect("bootstrap home");
    let signer = signer::spawn(&home).expect("spawn signer");

    // The seat's ONE delivery lock.
    let lock = Arc::new(tokio::sync::Mutex::new(()));

    let run_delivery = |workdir: PathBuf, branch: &'static str, oid: String| {
        let lock = Arc::clone(&lock);
        let signer = signer.clone();
        let url = url.clone();
        async move {
            let deadline = Instant::now() + DELIVERY_PUSH_TIMEOUT;
            let authority = Arc::new(AtomicBool::new(true));
            let (minter, minted) = production_shaped_minter(
                signer,
                &url,
                &git_transport::delivery_ref(branch),
                deadline,
                Arc::clone(&authority),
            );
            let outcome = serialized_bounded_push(&lock, DELIVERY_PUSH_TIMEOUT, || {
                seller_git::neutralize_then_push_off_runtime(
                    workdir,
                    url.clone(),
                    branch.to_owned(),
                    oid,
                    Some(minter),
                )
            })
            .await;
            authority.store(false, Ordering::SeqCst);
            (outcome, minted.lock().expect("log").clone())
        }
    };

    let first = tokio::spawn(run_delivery(first_workdir, first_branch, first_oid.clone()));
    // Give the first delivery the lock before the second asks for it, so "the one that waited" is
    // determinate rather than a race.
    tokio::time::sleep(Duration::from_millis(250)).await;
    let second = tokio::spawn(run_delivery(
        second_workdir,
        second_branch,
        second_oid.clone(),
    ));

    let (first_outcome, first_mints) = first.await.expect("first delivery task");
    let (second_outcome, second_mints) = second.await.expect("second delivery task");

    assert_eq!(
        first_outcome.expect("first delivery pushed"),
        first_oid,
        "the first delivery shipped its gated object"
    );
    assert_eq!(
        second_outcome.expect("second delivery pushed"),
        second_oid,
        "the second delivery shipped its gated object"
    );

    // Each delivery minted one token per wire request, and no two are the same bytes.
    assert_eq!(first_mints.len(), 2, "{first_mints:?}");
    assert_eq!(second_mints.len(), 2, "{second_mints:?}");
    let all: Vec<&Minted> = first_mints.iter().chain(second_mints.iter()).collect();
    for (index, minted) in all.iter().enumerate() {
        assert_eq!(
            minted.destination, url,
            "mint {index} named the seat's remote"
        );
        for other in all.iter().skip(index + 1) {
            assert_ne!(
                minted.header, other.header,
                "every leg of every delivery carries its own token"
            );
        }
    }

    // THE POINT: the delivery that waited signed nothing until the wait was over. The comparison is
    // against the first delivery's LAST mint — which happens while it still holds the lock, so this
    // is an ordering the lock guarantees rather than a race against when a task got rescheduled.
    let second_first_mint = second_mints[0].at;
    assert!(
        second_first_mint > first_mints[1].at,
        "the second delivery's first token was signed before the first delivery's last leg — it \
         would have been ageing through the wait instead of signed after it"
    );
    let waited = second_first_mint.duration_since(first_mints[0].at);
    assert!(
        waited >= HELD_LEG,
        "the wait the second delivery sat through is real: {waited:?}"
    );

    // Each delivery's tokens name ITS ref, never the other job's.
    for (mints, mine, theirs) in [
        (&first_mints, first_branch, second_branch),
        (&second_mints, second_branch, first_branch),
    ] {
        for minted in mints.iter() {
            let json = token_json(&minted.header);
            assert!(
                json.contains(&git_transport::delivery_ref(mine)),
                "token is scoped to its own job's ref: {json}"
            );
            assert!(
                !json.contains(&git_transport::delivery_ref(theirs)),
                "token must not name the other job's ref: {json}"
            );
            assert!(
                json.contains(&url),
                "token is bound to the seat's remote: {json}"
            );
        }
    }

    // And the wire agrees: four authorized requests, each carrying the exact bytes minted for it.
    let requests = relay.requests();
    assert_eq!(
        requests.len(),
        4,
        "two legs per delivery, and no read-back after either: {requests:?}"
    );
    let on_the_wire: Vec<&str> = requests
        .iter()
        .map(|request| {
            request
                .authorization
                .as_deref()
                .expect("every leg carried a token")
        })
        .collect();
    for minted in all.iter() {
        assert!(
            on_the_wire.contains(&minted.header.as_str()),
            "a minted token that never reached the wire: {minted:?}"
        );
    }

    // Both deliveries landed, each at its own ref.
    let bare = git2::Repository::open_bare(&relay_repo).expect("relay bare");
    assert_eq!(
        bare.refname_to_id(&git_transport::delivery_ref(first_branch))
            .expect("first ref")
            .to_string(),
        first_oid
    );
    assert_eq!(
        bare.refname_to_id(&git_transport::delivery_ref(second_branch))
            .expect("second ref")
            .to_string(),
        second_oid
    );

    drop(relay);
    let _ = std::fs::remove_dir_all(&root);
}
