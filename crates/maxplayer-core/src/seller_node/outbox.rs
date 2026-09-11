//! The outbox publisher: drain the durable event outbox to the relay, retrying until each event is
//! confirmed or expires.
//!
//! The store is the source of truth: a state change and the event it must publish are written in one
//! transaction ([`super::store`]). This publisher is the async half — it reads `pending` rows, hands
//! each to an [`EventPublisher`], and records the outcome. Because every enqueue is deduped on a
//! stable key and every event is signed at a fixed authored-at second, a re-publish after a crash is
//! idempotent at the relay: the same event id is sent again and the relay collapses it. That is what
//! lets the publisher retry freely without ever double-paying or double-delivering.
//!
//! The concrete relay transport (sign via the signer actor, send over a nostr client) is the
//! deployable [`EventPublisher`] the node wires at cutover; this module owns the durable-drain logic
//! and is exercised here against a fake publisher so the retry/confirm/expire behavior is pinned
//! independent of the network.

use super::store::{OutboxItem, SellerStore, StoreError};

/// Publishes one outbox event and returns the published event id on success.
///
/// An internal seam driven by the node's own single-threaded drain loop, so the `async fn` in trait
/// (no `Send` bound on the returned future) is intentional — the lint is suppressed here.
#[allow(async_fn_in_trait)]
pub trait EventPublisher {
    /// Publish `item` (sign it at its fixed `created_at_unix`, send it to the relay). Returns the
    /// published event id, or an error string to retry later.
    async fn publish(&self, item: &OutboxItem) -> Result<String, String>;
}

/// What one drain pass did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DrainReport {
    pub confirmed: usize,
    pub failed: usize,
    pub expired: usize,
    /// R2B: rows this pass deliberately did NOT attempt — either out of scope, or reached after the
    /// pass had spent its budget. They stay `pending` and durable; the next drain takes them.
    pub deferred: usize,
    /// R2B: publishes that were STARTED and cut off when the pass ran out of time mid-flight.
    /// Distinct from `deferred` (never started) and from `failed` (the remote answered): the row is
    /// pending, its attempt is charged, and whether the relay saw it is unknown — which is safe,
    /// because the retry re-sends the byte-identical event the relay dedupes.
    pub timed_out: usize,
}

/// R2B: what a drain pass may spend — and, when the answer is nothing, WHY.
///
/// `Exhausted` and `Unbudgeted` are different facts and must never collapse into one `None`. A
/// spent pass that reports "no budget" the same way a live unbudgeted caller does buys itself a
/// fresh full terminal-drain ceiling, which is precisely the overrun the budget exists to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainBudget {
    /// No pass governs this drain: the periodic outbox step, which is allowed to finish the queue.
    Unbudgeted,
    /// This much time is left to the pass.
    Remaining(std::time::Duration),
    /// The pass is spent. Nothing may be started, in flight or otherwise.
    Exhausted,
}

impl DrainBudget {
    /// The ceiling for ONE awaited step, or `None` when nothing may be started at all.
    ///
    /// `None` here means EXHAUSTED and nothing else, so a caller reading it can act on the one fact
    /// it actually states: decide nothing, leave the durable row, let a later pass resolve it.
    pub fn step(self, ceiling: std::time::Duration) -> Option<std::time::Duration> {
        match self {
            DrainBudget::Unbudgeted => Some(ceiling),
            DrainBudget::Remaining(left) => Some(left.min(ceiling)),
            DrainBudget::Exhausted => None,
        }
    }

    /// Narrow the budget to `ceiling`. An unbudgeted caller becomes bounded by it; a spent pass
    /// stays spent, because a ceiling is not a grant.
    pub fn capped(self, ceiling: std::time::Duration) -> DrainBudget {
        match self {
            DrainBudget::Unbudgeted => DrainBudget::Remaining(ceiling),
            DrainBudget::Remaining(left) => DrainBudget::Remaining(left.min(ceiling)),
            DrainBudget::Exhausted => DrainBudget::Exhausted,
        }
    }
}

/// Run one drain pass: first expire rows past their retry window, then publish every remaining
/// pending row. A confirmed publish is marked `confirmed`; a failed one bumps its attempt count and
/// stays `pending` for the next pass.
pub async fn drain_once<P: EventPublisher>(
    store: &SellerStore,
    publisher: &P,
    now_unix: i64,
) -> Result<DrainReport, StoreError> {
    drain_scoped(store, publisher, now_unix, None, DrainBudget::Unbudgeted).await
}

/// R2B: one drain pass that can be SCOPED to a single job and BOUNDED in time.
///
/// The unscoped, unbounded pass is the node's periodic outbox step and keeps its old behavior. This
/// variant exists for the drains that hang off a job's terminal decision, where awaiting the whole
/// backlog is a mistake in two separate ways:
///
/// - **Scope.** The promptness a terminal decision owes is for ITS OWN event. Publishing every
///   unrelated pending row on the way is work the decision did not ask for and cannot bound, and a
///   deep backlog turns one job's completion into everyone's queue.
/// - **Budget.** A publisher awaiting a slow signer or an unresponsive relay makes each row take as
///   long as the remote feels like taking. Without a ceiling, one such row holds the tick open for
///   the whole worklist.
///
/// Nothing is dropped: rows not attempted stay `pending` and durable — the atomic enqueue is
/// untouched — and are counted as `deferred` so a pass that stops early is visible rather than
/// silent. The next drain resumes the backlog.
///
/// The budget is checked BEFORE each publish AND carried INTO it. A pre-start check alone bounds
/// only the decision to begin: the publish it admits can then take as long as the remote likes, so
/// one row that accepts bytes and never answers holds the terminal completion open exactly as if
/// there were no budget at all. Each publish therefore runs under the time the pass has LEFT at
/// that moment, and the remainder shrinks across the loop through successes, refusals and cut-offs
/// alike.
///
/// Cutting a publish off is safe in all three senses that matter here, and none of them is an
/// accident of this function:
///
/// - **Durable event identity.** The row is signed at its FIXED `created_at_unix`, so the retry
///   re-derives a byte-identical event id and the relay dedupes it. A cut-off publish that did
///   reach the relay cannot become a second event.
/// - **Retry.** The row is never marked confirmed on a cut-off, its attempt IS charged, and it
///   stays `pending`, so the ordinary retry window and expiry apply to it — a relay that always
///   stalls eventually expires the row instead of being retried hot forever.
/// - **Cancellation.** Dropping the publish future is the only cancellation, and the store is not
///   touched on that path, so no state exists that a later pass must unwind.
pub async fn drain_scoped<P: EventPublisher>(
    store: &SellerStore,
    publisher: &P,
    now_unix: i64,
    job_scope: Option<&str>,
    budget: DrainBudget,
) -> Result<DrainReport, StoreError> {
    let started = std::time::Instant::now();
    let mut report = DrainReport::default();
    // Expiry is a local write with no remote await, so it stays whole even for a scoped pass: the
    // retry window is a property of the outbox, not of whoever happened to trigger this drain.
    report.expired = store.expire_outbox(now_unix)?;

    for item in store.pending_outbox(now_unix)? {
        if job_scope.is_some_and(|job| !item_belongs_to(&item, job)) {
            report.deferred += 1;
            continue;
        }
        // What is left AT THIS ROW, not what was left when the pass began.
        let remaining = match budget {
            DrainBudget::Unbudgeted => None,
            DrainBudget::Exhausted => {
                report.deferred += 1;
                continue;
            }
            DrainBudget::Remaining(total) => {
                let left = total.saturating_sub(started.elapsed());
                if left.is_zero() {
                    report.deferred += 1;
                    continue;
                }
                Some(left)
            }
        };
        // The publish runs INSIDE that remainder. `None` on the outer result is the pass running
        // out mid-flight; the future is dropped and nothing was written.
        let published = match remaining {
            None => Some(publisher.publish(&item).await),
            Some(left) => tokio::time::timeout(left, publisher.publish(&item))
                .await
                .ok(),
        };
        match published {
            Some(Ok(event_id)) => {
                store.mark_confirmed(item.id, &event_id, now_unix)?;
                report.confirmed += 1;
            }
            Some(Err(_)) => {
                store.record_attempt(item.id, now_unix)?;
                report.failed += 1;
            }
            None => {
                store.record_attempt(item.id, now_unix)?;
                report.timed_out += 1;
            }
        }
    }
    Ok(report)
}

/// Whether an outbox row is one of `job_id`'s own events.
///
/// Rows are keyed `<kind>:<job_id>`, so the job is the segment after the first colon. Comparing the
/// SEGMENT rather than testing a suffix matters: job ids are hex of a fixed length here, but a
/// suffix test would quietly widen scope the day any id becomes a suffix of another.
fn item_belongs_to(item: &OutboxItem, job_id: &str) -> bool {
    item.dedup_key
        .split_once(':')
        .is_some_and(|(_, keyed_job)| keyed_job == job_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn fresh_store(label: &str) -> (SellerStore, std::path::PathBuf) {
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "maxplayer-seller-outbox-{label}-{}-{id}.sqlite",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        (SellerStore::open(&path).expect("open"), path)
    }

    fn claim_draft() -> crate::gateway::EventDraft {
        crate::gateway::claim_draft(&"e".repeat(64), &"b".repeat(64), &"s".repeat(64), crate::gateway::ClaimPayment::Sat("creqA"), &[], &Default::default())
    }

    /// Records every publish call; can be told to fail so the retry path is exercised.
    struct FakePublisher {
        calls: RefCell<Vec<String>>,
        fail: bool,
    }

    impl EventPublisher for FakePublisher {
        async fn publish(&self, item: &OutboxItem) -> Result<String, String> {
            self.calls.borrow_mut().push(item.dedup_key.clone());
            if self.fail {
                Err("relay down".into())
            } else {
                Ok(format!("evt-{}", item.dedup_key))
            }
        }
    }

    /// Publishes, but slowly — the delayed signer/relay the budget exists for.
    struct StallingPublisher {
        calls: RefCell<Vec<String>>,
        per_call: std::time::Duration,
    }

    impl EventPublisher for StallingPublisher {
        async fn publish(&self, item: &OutboxItem) -> Result<String, String> {
            {
                self.calls.borrow_mut().push(item.dedup_key.clone());
            }
            tokio::time::sleep(self.per_call).await;
            Ok(format!("evt-{}", item.dedup_key))
        }
    }

    fn seed_pending(store: &SellerStore, job: &str) {
        store
            .claim_and_enqueue(job, &"o".repeat(64), Some("creqA"), &claim_draft(), 1, 9_999, 1)
            .expect("claim");
    }

    /// R2B — A JOB'S OWN EVENT, NOT EVERYONE'S QUEUE.
    ///
    /// The drain hanging off a terminal decision used to publish every pending row on the way past,
    /// so one job's completion did the whole backlog's work while holding the tick. Scoped, it
    /// publishes its own event and leaves the rest alone.
    ///
    /// The second half is the half that matters: the untouched rows are DEFERRED, not dropped —
    /// still pending, still durable — and the very next ordinary pass publishes them. That is what
    /// makes the scope safe rather than a way to lose events.
    #[tokio::test(flavor = "current_thread")]
    async fn a_scoped_drain_publishes_only_its_own_job_and_a_later_pass_takes_the_backlog() {
        let (store, path) = fresh_store("scope");
        let mine = "a".repeat(64);
        let backlog = ["b".repeat(64), "c".repeat(64)];
        seed_pending(&store, &mine);
        for job in &backlog {
            seed_pending(&store, job);
        }

        let publisher = FakePublisher { calls: RefCell::new(vec![]), fail: false };
        let scoped = drain_scoped(&store, &publisher, 2, Some(&mine), DrainBudget::Unbudgeted)
            .await
            .expect("scoped drain");

        assert_eq!(scoped.confirmed, 1, "exactly this job's own event went out");
        assert_eq!(scoped.deferred, 2, "and the unrelated backlog was left for the drain step");
        assert_eq!(
            publisher.calls.borrow().clone(),
            vec![format!("claim:{mine}")],
            "the backlog was not even ATTEMPTED — an attempt is the unbounded remote await this \
             scope exists to avoid"
        );

        // Later loop progress: the ordinary pass resolves what the scoped one deferred.
        let later = drain_once(&store, &publisher, 3).await.expect("later pass");
        assert_eq!(later.confirmed, 2, "the deferred rows were still pending and went out next");
        assert_eq!(
            drain_once(&store, &publisher, 4).await.expect("third pass").confirmed,
            0,
            "and nothing is left over"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// R2B — A SLOW PUBLISHER CANNOT OWN THE PASS.
    ///
    /// Each publish is a remote await the node does not control. With no ceiling, a signer or relay
    /// that answers slowly multiplies by the length of the worklist and the pass runs as long as
    /// the remote pleases.
    ///
    /// Checking the budget before each publish is NOT enough, and this test is the one that says
    /// so: a pre-start check admits an await that then runs unbounded, so a single row that accepts
    /// bytes and never answers overruns the pass by as much as the remote likes. Round 5 asserted
    /// exactly that overrun as correct — "the row already in flight is finished" — and the publish
    /// here takes 200ms against a 50ms pass. The publish is therefore bounded IN FLIGHT: it is cut
    /// off at the budget and counted `timed_out`.
    ///
    /// Custody survives the stop in both forms. The unattempted rows stay pending, the cut-off row
    /// stays pending with its attempt charged, and a later pass finishes all three — including the
    /// one that was interrupted, whose event id is byte-identical because it is signed at the row's
    /// fixed `created_at_unix`.
    #[tokio::test(flavor = "current_thread")]
    async fn a_drain_whose_publisher_stalls_stops_on_its_budget_and_a_later_pass_finishes_the_rows() {
        let (store, path) = fresh_store("stall");
        for job in ["a".repeat(64), "b".repeat(64), "c".repeat(64)] {
            seed_pending(&store, &job);
        }

        let publisher = StallingPublisher {
            calls: RefCell::new(vec![]),
            per_call: std::time::Duration::from_millis(200),
        };
        let started = std::time::Instant::now();
        let bounded = drain_scoped(
            &store,
            &publisher,
            2,
            None,
            DrainBudget::Remaining(std::time::Duration::from_millis(50)),
        )
        .await
        .expect("bounded drain");
        let elapsed = started.elapsed();

        assert_eq!(bounded.confirmed, 0, "a publish that outran the pass did not confirm anything");
        assert_eq!(bounded.timed_out, 1, "the in-flight publish was cut off at the budget");
        assert_eq!(bounded.deferred, 2, "the rest were not started");
        assert_eq!(
            publisher.calls.borrow().len(),
            1,
            "one remote await, not three: the budget stopped the pass from opening more"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(150),
            "the pass returned on its own 50ms budget, not at the 200ms pace of the remote \
             (took {elapsed:?})"
        );
        assert_eq!(
            store.pending_outbox(2).expect("read the outbox").len(),
            3,
            "every row — the cut-off one included — is still pending and durable"
        );

        let later = drain_once(&store, &publisher, 3).await.expect("later pass");
        assert_eq!(
            later.confirmed, 3,
            "a pass with time finishes everything the bounded one held, the interrupted row included"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn drain_confirms_pending_and_is_a_noop_second_time() {
        let (store, path) = fresh_store("confirm");
        let job = "j".repeat(64);
        store.claim_and_enqueue(&job, &"o".repeat(64), Some("creqA"), &claim_draft(), 1, 9_999, 1).expect("claim");

        let publisher = FakePublisher { calls: RefCell::new(vec![]), fail: false };
        let report = drain_once(&store, &publisher, 2).await.expect("drain");
        assert_eq!(report.confirmed, 1);
        assert_eq!(publisher.calls.borrow().len(), 1);
        assert_eq!(
            store.outbox_row(&format!("claim:{job}")).expect("row").expect("exists").0,
            "confirmed"
        );

        // Second pass: the row is confirmed, so nothing is published again.
        let report = drain_once(&store, &publisher, 3).await.expect("drain2");
        assert_eq!(report.confirmed, 0);
        assert_eq!(publisher.calls.borrow().len(), 1, "no re-publish of a confirmed row");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_publish_stays_pending_and_bumps_attempts() {
        let (store, path) = fresh_store("retry");
        let job = "j".repeat(64);
        store.claim_and_enqueue(&job, &"o".repeat(64), Some("creqA"), &claim_draft(), 1, 9_999, 1).expect("claim");

        let publisher = FakePublisher { calls: RefCell::new(vec![]), fail: true };
        let report = drain_once(&store, &publisher, 2).await.expect("drain");
        assert_eq!(report.failed, 1);
        let row = store.outbox_row(&format!("claim:{job}")).expect("row").expect("exists");
        assert_eq!(row.0, "pending", "a failed publish retries");
        assert_eq!(row.1, 1, "attempt was recorded");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn expired_rows_are_not_published() {
        let (store, path) = fresh_store("expire");
        let job = "j".repeat(64);
        store.claim_and_enqueue(&job, &"o".repeat(64), Some("creqA"), &claim_draft(), 1, 100, 1).expect("claim");

        let publisher = FakePublisher { calls: RefCell::new(vec![]), fail: false };
        // now=200 is past expires_at=100 ⇒ the row expires and is never handed to the publisher.
        let report = drain_once(&store, &publisher, 200).await.expect("drain");
        assert_eq!(report.expired, 1);
        assert_eq!(report.confirmed, 0);
        assert!(publisher.calls.borrow().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    /// R2B — THE REMAINDER CARRIES THROUGH THE SUCCESSES.
    ///
    /// A budget that is re-read as "the original allowance" at every row is not a budget: three
    /// rows under a 250ms pass would each be allowed 250ms and the pass would run 750ms. What each
    /// publish gets is what the pass has LEFT when that publish starts, and the successes before it
    /// are what spent the rest. Here two 100ms publishes succeed and the third finds ~50ms left, so
    /// it is cut off in flight rather than granted a fourth fresh allowance.
    ///
    /// RED ON REVERT: pass `total` instead of `total - started.elapsed()` into the per-publish
    /// timeout and all three confirm ⇒ this fails.
    #[tokio::test(flavor = "current_thread")]
    async fn the_remaining_budget_shrinks_across_successful_publishes() {
        let (store, path) = fresh_store("shrink");
        for job in ["a".repeat(64), "b".repeat(64), "c".repeat(64)] {
            seed_pending(&store, &job);
        }
        let publisher = StallingPublisher {
            calls: RefCell::new(vec![]),
            per_call: std::time::Duration::from_millis(100),
        };
        let started = std::time::Instant::now();
        let report = drain_scoped(
            &store,
            &publisher,
            2,
            None,
            DrainBudget::Remaining(std::time::Duration::from_millis(250)),
        )
        .await
        .expect("bounded drain");
        let elapsed = started.elapsed();

        assert_eq!(report.confirmed, 2, "two publishes fitted inside the pass");
        assert_eq!(
            report.timed_out, 1,
            "the third started with less than it needed and was cut off in flight"
        );
        assert_eq!(report.deferred, 0, "every row was reached; the last one simply ran out of time");
        assert!(
            elapsed < std::time::Duration::from_millis(450),
            "the pass stayed near its 250ms budget rather than 3x100ms of remote time \
             (took {elapsed:?})"
        );
        assert_eq!(
            store.pending_outbox(2).expect("read the outbox").len(),
            1,
            "the cut-off row is still pending for the next pass"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// R2B — "SPENT" AND "UNGOVERNED" ARE DIFFERENT ANSWERS.
    ///
    /// The arithmetic is the claim, so it is asserted directly. One `None` for both facts is what
    /// let a spent pass be read as an ungoverned one and receive a fresh full ceiling.
    #[test]
    fn a_spent_pass_and_an_ungoverned_pass_are_not_the_same_fact() {
        let ceiling = std::time::Duration::from_secs(5);

        assert_eq!(
            DrainBudget::Unbudgeted.step(ceiling),
            Some(ceiling),
            "an ungoverned caller may take the standing ceiling"
        );
        assert_eq!(
            DrainBudget::Exhausted.step(ceiling),
            None,
            "a spent pass may start nothing at all"
        );
        assert_eq!(
            DrainBudget::Remaining(std::time::Duration::from_secs(2)).step(ceiling),
            Some(std::time::Duration::from_secs(2)),
            "and a live pass is bounded by whichever of the two is smaller"
        );

        assert_eq!(
            DrainBudget::Exhausted.capped(ceiling),
            DrainBudget::Exhausted,
            "a ceiling is not a grant: capping a spent pass leaves it spent"
        );
        assert_eq!(
            DrainBudget::Unbudgeted.capped(ceiling),
            DrainBudget::Remaining(ceiling),
            "capping an ungoverned caller makes it bounded"
        );
    }
}
