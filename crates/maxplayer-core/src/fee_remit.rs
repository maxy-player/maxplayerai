//! Paying the accrued platform fee to the platform's Lightning address — **the one remit path in
//! the product**, and the first code in it that moves real money.
//!
//! ## Who calls this, and when
//!
//! [`remit`] has exactly three callers, and all three are named here so a grep confirms it:
//!
//! 1. **The seller node's collect path** (`seller_node::run`, the `Collected::New` arm of the
//!    receipt write), through [`remit_live_best_effort`] → [`remit_best_effort`] under
//!    [`RemitTrigger::Collect`]. This is the mechanism, and the fast path: a fee the seller had to
//!    remember to pay would not be a fee, so the node remits as a consequence of collecting, normally
//!    within a second of the sale. It fires only when a receipt was journaled **New** — never on a
//!    replayed wrap (`Duplicate`), never on an error — and it is **best-effort**: whatever happens
//!    here is logged and journaled; it cannot fail the collect, delay the job being marked paid, or
//!    change what the seller received.
//! 2. **The seller node's retry tick** (`seller_node::run`, an arm of the live loop's `select!`),
//!    through the same two functions under [`RemitTrigger::Retry`]. This is the safety net behind
//!    the fast path (stage 2a, addendum 2): a failed remittance retries on the node's own clock — base
//!    30 s, doubling to a 30-minute cap, full jitter — for as long as the node runs, and stops with
//!    the loop. [`RemitBackoff`] is the pacing; [`RemitFlight`] keeps the two node paths to one
//!    attempt in flight.
//! 3. **`maxplayer seller fees remit`** (`crates/maxplayer/src/seller_fees.rs`): inspection (the
//!    default dry run resolves, quotes, prints the plan and the recent attempts, and moves nothing),
//!    recovery (`--confirm` forces an attempt now, for an operator whose automatic path has been
//!    failing), and reconciliation (an interrupted attempt is settled or released against the mint).
//!
//! The `[platform_fee] auto_remit` switch ([`crate::home::PlatformFeeConfig`]) turns BOTH node
//! paths off — the collect path's attempt and the retry tick — and does not touch accrual;
//! `--confirm` pays regardless. There is no startup sweep and no other call site. The node's two
//! attempts run on threads the node owns and drains on shutdown (bounded); see `seller_node::run`.
//!
//! ## What one attempt does
//!
//! Reconcile any in-flight attempt first; read the unremitted balance; resolve
//! [`PLATFORM_FEE_ADDRESS`] over LNURL-pay ([`crate::lnurl_pay`], fail-closed) and refuse below its
//! minimum — the expected steady state for small sellers, not an error; probe the mint's melt fee
//! reserve on the gross and invoice the **net**, so the fee comes OUT of the accrued amount; journal
//! the plan (which pins the receipts, records this process as the row's owner under a lease, and
//! refuses a duplicate — the idempotency that makes two concurrent collects pay at most once); pass
//! the **pre-spend gate** — re-confirm ownership of the planned row, then melt under a hard
//! [`MeltCeiling`] through [`crate::wallet_ops::melt_within_blocking`], the same gated melt
//! `maxplayer wallet melt` uses (it honours `allow_real_mints`), which refuses BEFORE any proof is
//! spent if the quote raised at payment time would take more than the accrued gross; settle. Every
//! attempt that meant to pay is journaled with its outcome (`fee_remit_attempts`), so a payout that
//! keeps failing is visible in the read-out rather than silent.
//!
//! ## The two invariants (stage 2a, addendum 3)
//!
//! **Money hold (§1):** the seller never pays more than the fee it accrued — gross is the ceiling,
//! the melt fee comes out of it, and the ceiling is enforced at the moment of spending, not
//! estimated beforehand or regretted afterwards. The estimate at plan time is a plan; the quote the
//! wallet actually pays under is checked against `gross` inside the melt, and a reserve that grew in
//! between is a refused, journaled, failed attempt with the balance intact.
//!
//! **Ownership (§2):** a `planned` row is paid only by the process that planned it, and is released
//! by another process only when its quote is provably terminal (FAILED at the mint) or its owner is
//! provably gone (the lease of [`REMIT_LEASE`] has run out) — never on UNPAID alone, because UNPAID
//! means "not yet", not "abandoned". The owner re-validates its claim immediately before spending
//! and refuses if less than [`SPEND_MARGIN`] of lease remains, so a spend can never start close
//! enough to the lease's end to land after a release. The owner's own reconciliation of its own row
//! may release on UNPAID: a process runs at most one attempt at a time
//! ([`RemitFlight`] in the node; one shot for the command), so its earlier attempt is over.
//!
//! Every effect on the world goes through [`RemitEffects`], so the decision logic is tested against
//! scripted effects without a network or a mint. Exactly one method of that trait spends:
//! [`RemitEffects::melt`].

use std::fmt;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::home::MaxplayerHome;
use crate::lnurl_pay::{self, HttpsFetch, LightningAddress, PayRequest, ResolvedInvoice};
use crate::platform_fee::PLATFORM_FEE_ADDRESS;
use crate::seller_node::store::{
    FeeRemittance, OwnershipLost, PlanRefused, RemitAttempt, RemitAttemptOutcome,
    RemitAttemptTrigger, RemitSettlement, RemittancePlan, SellerStore, SettledBy,
};
use crate::wallet_ops::{
    self, MeltCeiling, MeltEstimate, MeltOutcome, MeltQuoteState, MeltQuoteStatus, WalletOpsError,
};

/// How many journaled attempts the command prints, newest first.
pub const RECENT_ATTEMPTS_SHOWN: usize = 5;

/// How long a process's claim on a `planned` row stands (addendum 3 §2). Another process may
/// release the row on UNPAID / no-quote only after this has passed since the plan. Five minutes is
/// far longer than a melt needs to leave UNPAID: a melt that reaches the mint turns its quote
/// PENDING or PAID within seconds, and one that never reaches it errors out and ends the attempt.
pub const REMIT_LEASE: Duration = Duration::from_secs(5 * 60);

/// How much of its lease an owner must still hold to START a payment. Another process is entitled
/// to release the row the instant the lease ends; a spend begun with less than this margin could
/// land after that release. Processes share one host clock (the store is a local file), so the
/// margin covers scheduling pauses, not clock skew.
pub const SPEND_MARGIN: Duration = Duration::from_secs(60);

/// Why [`RemitEffects::melt`] did not pay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeltFailure {
    /// The melt was refused BEFORE any proof was selected, prepared or sent — the quote the mint
    /// raised at payment time did not fit the [`MeltCeiling`]. Nothing left the wallet, which the
    /// caller may rely on: it releases its planned row itself.
    RefusedBeforeSpending(String),
    /// The melt failed somewhere the caller cannot see: proofs may or may not have reached the mint.
    /// The planned row stays for reconciliation against the mint.
    Failed(String),
}

impl fmt::Display for MeltFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefusedBeforeSpending(reason) => write!(formatter, "{reason}"),
            Self::Failed(error) => write!(formatter, "{error}"),
        }
    }
}

/// The remit path's effects on the world, behind a trait so the decision logic — what is paid,
/// when, and what is refused — is tested without a network or a mint. Exactly one method moves
/// money: [`Self::melt`]. Everything else reads.
pub trait RemitEffects {
    /// This process's opaque owner token for the rows it plans (addendum 3 §2): stable for the
    /// life of the process, distinct across processes. The node's attempts and the command's run
    /// each speak as one owner.
    fn owner(&self) -> &str;
    /// LNURL step 1–2: the destination's payRequest (callback + sendable bounds).
    fn pay_request(&mut self, address: &LightningAddress) -> Result<PayRequest, String>;
    /// LNURL step 3–4: an invoice for exactly `amount_sats`.
    fn invoice(&mut self, pay: &PayRequest, amount_sats: u64) -> Result<ResolvedInvoice, String>;
    /// A melt quote for the invoice — the mint's fee reserve — WITHOUT paying.
    fn melt_estimate(&mut self, bolt11: &str) -> Result<MeltEstimate, String>;
    /// **The payment.** Pays the invoice from the seller's ecash under a hard ceiling, refusing
    /// before anything is spent if the quote raised at payment time does not fit. The only method
    /// here that spends.
    fn melt(&mut self, bolt11: &str, ceiling: &MeltCeiling) -> Result<MeltOutcome, MeltFailure>;
    /// What the mint says about the melt quote(s) this wallet raised for the invoice, if any —
    /// used to reconcile an interrupted attempt.
    fn melt_status(&mut self, bolt11: &str) -> Result<Option<MeltQuoteStatus>, String>;
    /// Observation point: called once the plan is journaled, before the pre-spend gate. The live
    /// effects do nothing here; tests pause here to interleave a second process against the
    /// planned row (addendum 3 §2.2).
    fn after_plan(&mut self, _planned: &FeeRemittance) {}
}

/// This process's owner token: pid plus a boot nonce from the OS RNG, fixed for the life of the
/// process. Every [`LiveEffects`] in the process speaks as this one owner.
fn process_owner() -> &'static str {
    static OWNER: OnceLock<String> = OnceLock::new();
    OWNER.get_or_init(|| {
        let mut nonce = [0u8; 8];
        let nonce = match getrandom::fill(&mut nonce) {
            Ok(()) => u64::from_le_bytes(nonce),
            Err(_) => 0,
        };
        format!("pid{}-{nonce:016x}", std::process::id())
    })
}

/// The shipped effects: LNURL over https, the packaged CDK wallet at `home`, the home's default
/// mint (the first accepted mint — where a seller's receipts land). Every wallet call goes through
/// the `*_blocking` wrappers in [`crate::wallet_ops`], which refuse to run inside a Tokio runtime —
/// so the seller node drives this from a plain thread of its own, never from a task.
pub struct LiveEffects {
    home: MaxplayerHome,
    fetch: HttpsFetch,
}

impl LiveEffects {
    pub fn new(home: MaxplayerHome) -> Result<Self, String> {
        let fetch = HttpsFetch::new().map_err(|error| error.to_string())?;
        Ok(Self { home, fetch })
    }
}

impl RemitEffects for LiveEffects {
    fn owner(&self) -> &str {
        process_owner()
    }

    fn pay_request(&mut self, address: &LightningAddress) -> Result<PayRequest, String> {
        lnurl_pay::fetch_pay_request(&self.fetch, address).map_err(|error| error.to_string())
    }

    fn invoice(&mut self, pay: &PayRequest, amount_sats: u64) -> Result<ResolvedInvoice, String> {
        lnurl_pay::request_invoice(&self.fetch, pay, amount_sats).map_err(|error| error.to_string())
    }

    fn melt_estimate(&mut self, bolt11: &str) -> Result<MeltEstimate, String> {
        wallet_ops::melt_quote_blocking(&self.home, bolt11, None).map_err(|error| error.to_string())
    }

    fn melt(&mut self, bolt11: &str, ceiling: &MeltCeiling) -> Result<MeltOutcome, MeltFailure> {
        wallet_ops::melt_within_blocking(&self.home, bolt11, None, Some(ceiling)).map_err(|error| {
            match error {
                // The one error the melt raises BEFORE selecting a proof, typed so it can be relied
                // on: nothing left the wallet. Every other error is opaque as to how far it got.
                refused @ WalletOpsError::MeltExceedsCeiling { .. } => {
                    MeltFailure::RefusedBeforeSpending(refused.to_string())
                }
                other => MeltFailure::Failed(other.to_string()),
            }
        })
    }

    fn melt_status(&mut self, bolt11: &str) -> Result<Option<MeltQuoteStatus>, String> {
        wallet_ops::melt_status_for_invoice_blocking(&self.home, bolt11, None)
            .map_err(|error| error.to_string())
    }
}

/// Who is running the attempt, and therefore whether it pays and how it is journaled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemitTrigger {
    /// `maxplayer seller fees remit` without `--confirm`: resolve, quote, print, move nothing.
    DryRun,
    /// `maxplayer seller fees remit --confirm`: the operator's recovery path. Pays.
    Command,
    /// The seller node, after a receipt was journaled `Collected::New`. Pays.
    Collect,
    /// The seller node's retry tick (addendum 2): the loop's own clock, paced by [`RemitBackoff`].
    /// Pays.
    Retry,
}

impl RemitTrigger {
    /// Whether this run may journal a plan and melt.
    pub fn pays(self) -> bool {
        !matches!(self, Self::DryRun)
    }

    /// How the attempt is journaled — a dry run is not an attempt.
    fn journal_as(self) -> Option<RemitAttemptTrigger> {
        match self {
            Self::DryRun => None,
            Self::Command => Some(RemitAttemptTrigger::Command),
            Self::Collect => Some(RemitAttemptTrigger::Collect),
            Self::Retry => Some(RemitAttemptTrigger::Retry),
        }
    }
}

/// Why an attempt declined and moved nothing. [`Self::is_threshold`] separates the expected steady
/// state (nothing owed yet, or not enough to clear the destination's minimum) — which is accrued
/// silently and never journaled as an attempt — from the refusals an operator should see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The unremitted balance is zero.
    NothingUnremitted,
    /// The unremitted balance is below the destination's resolved `minSendable`.
    BelowMinimum { unremitted: u64, min_sats: u64 },
    /// The unremitted balance is above the destination's resolved `maxSendable`; this path remits
    /// the whole balance or nothing.
    AboveMaximum { unremitted: u64, max_sats: u64 },
    /// The mint's melt fee reserve leaves nothing, leaves less than the minimum, or would take more
    /// out of the wallet than was accrued.
    ReserveDoesNotFit { gross: u64, reserve: u64 },
    /// An earlier attempt is still settling at the mint (melt quote PENDING or unknown).
    Settling { remittance_id: String },
    /// An earlier attempt's planned row belongs to another process whose lease has not run out, and
    /// the mint does not report its quote terminal (addendum 3 §2): UNPAID means "not yet", not
    /// "abandoned", so this run may not release it and may not plan on top of it.
    HeldByOwner {
        remittance_id: String,
        owner: String,
        lease_until_unix: i64,
    },
    /// The pre-spend gate refused: between journaling the plan and paying it, this process lost its
    /// claim on the row (another process reconciled it), or too little lease remained to start a
    /// payment safely. Nothing was spent.
    OwnershipLost {
        remittance_id: String,
        reason: String,
    },
    /// The store refused to journal the plan (a row already in flight, the balance moved under us,
    /// an invoice already used).
    PlanRefused(String),
}

impl Refusal {
    /// The two refusals that are the expected steady state for a small seller: not an error, not an
    /// attempt, nothing to journal — the balance accumulates until it clears the minimum.
    pub fn is_threshold(&self) -> bool {
        matches!(self, Self::NothingUnremitted | Self::BelowMinimum { .. })
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NothingUnremitted => write!(formatter, "nothing unremitted"),
            Self::BelowMinimum {
                unremitted,
                min_sats,
            } => write!(
                formatter,
                "unremitted {unremitted} sats is below the destination's minimum of {min_sats} sats"
            ),
            Self::AboveMaximum {
                unremitted,
                max_sats,
            } => write!(
                formatter,
                "unremitted {unremitted} sats exceeds the destination's maximum of {max_sats} sats"
            ),
            Self::ReserveDoesNotFit { gross, reserve } => write!(
                formatter,
                "the mint's melt fee reserve ({reserve} sats) does not fit inside the {gross} sats accrued"
            ),
            Self::Settling { remittance_id } => write!(
                formatter,
                "remittance {remittance_id} is still settling at the mint"
            ),
            Self::HeldByOwner {
                remittance_id,
                owner,
                lease_until_unix,
            } => write!(
                formatter,
                "remittance {remittance_id} is planned by another live process ({owner}, lease until unix {lease_until_unix}) and its quote is not terminal; not releasing a live payer's intent"
            ),
            Self::OwnershipLost {
                remittance_id,
                reason,
            } => write!(
                formatter,
                "refused before spending: remittance {remittance_id} — {reason}"
            ),
            Self::PlanRefused(reason) => write!(formatter, "plan refused: {reason}"),
        }
    }
}

/// How one run of [`remit`] ended. `Err` from [`remit`] is the other ending: an effect failed before
/// or during reconciliation and nothing was paid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemitOutcome {
    /// The plan was printed and nothing moved.
    DryRun,
    /// The melt settled and the remittance is journaled settled.
    Paid {
        remittance_id: String,
        net_sats: u64,
        melt_fee_sats: u64,
    },
    /// Declined; nothing moved.
    Refused(Refusal),
    /// The plan was journaled and the melt then failed: the row stays `planned` and is reconciled
    /// with the mint by the next attempt — settled if the payment landed, released if it did not.
    MeltFailed {
        remittance_id: String,
        error: String,
    },
    /// The plan was journaled and the melt REFUSED before spending anything — the quote raised at
    /// payment time would have taken more than the accrued gross (addendum 3 §1). Nothing left the
    /// wallet, so this process released its own row: the balance is unremitted again, the attempt
    /// is journaled failed, and the backoff escalates.
    MeltRefused {
        remittance_id: String,
        reason: String,
    },
}

/// What reconciliation decides about the one `planned` row, from the mint's answer about its quote
/// and the row's ownership (addendum 3 §2.1). Pure, so the rule is tested as a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reconcile {
    /// The mint reports the quote PAID: settle, keep the receipts discharged, never melt again.
    Settle,
    /// Release the receipts back to unremitted; the reason is printed and journaled.
    Release(String),
    /// Leave the row exactly as it is and refuse this run; the reason is printed and journaled.
    Hold(Refusal),
}

/// The release rule. A `planned` row is released only when:
/// - the mint reports its quote **FAILED** — terminal, whoever owns the row; or
/// - the mint reports **UNPAID**, or the wallet never raised a quote for its invoice, AND either
///   the row is **this process's own** (a process runs one attempt at a time, so its earlier
///   attempt is over and cannot still be paying) or the owner's **lease has run out** (the owner is
///   provably gone, or provably not spending: it refuses to start a payment inside
///   [`SPEND_MARGIN`] of the lease's end).
///
/// It is **held** — nothing written, this run refused — when the quote is PENDING or UNKNOWN (a
/// payment may be settling) or when it is UNPAID / absent but another process's lease still stands:
/// UNPAID means "not yet", not "abandoned".
pub fn reconcile_decision(
    row: &FeeRemittance,
    status: Option<&MeltQuoteStatus>,
    my_owner: &str,
    now_unix: i64,
) -> Reconcile {
    let unpaid_reason = match status {
        None => "the wallet never raised a melt quote for its invoice — no sats left the wallet"
            .to_owned(),
        Some(status) => format!(
            "mint {} reports melt quote {} {} — no sats left the wallet",
            status.mint_url, status.quote_id, status.state
        ),
    };
    match status.map(|status| status.state) {
        Some(MeltQuoteState::Paid) => Reconcile::Settle,
        Some(MeltQuoteState::Pending) | Some(MeltQuoteState::Unknown) => {
            Reconcile::Hold(Refusal::Settling {
                remittance_id: row.remittance_id.clone(),
            })
        }
        Some(MeltQuoteState::Failed) => Reconcile::Release(format!(
            "{unpaid_reason}; FAILED is terminal at the mint whoever owns the row"
        )),
        Some(MeltQuoteState::Unpaid) | None => {
            if row.owner.as_deref() == Some(my_owner) {
                Reconcile::Release(format!(
                    "{unpaid_reason}; the row is this process's own earlier attempt, which is over"
                ))
            } else if row.lease_expired(now_unix) {
                Reconcile::Release(format!(
                    "{unpaid_reason}; its owner's lease ran out at unix {} (owner {})",
                    row.lease_until_unix.unwrap_or(row.created_at_unix),
                    row.owner.as_deref().unwrap_or("none recorded")
                ))
            } else {
                Reconcile::Hold(Refusal::HeldByOwner {
                    remittance_id: row.remittance_id.clone(),
                    owner: row.owner.clone().unwrap_or_default(),
                    lease_until_unix: row.lease_until_unix.unwrap_or(row.created_at_unix),
                })
            }
        }
    }
}

/// What the run learned before it ended, for the attempt journal.
#[derive(Default)]
struct AttemptTrace {
    unremitted: Option<u64>,
    remittance_id: Option<String>,
}

/// **The remit entry point.** Runs one attempt against `store` through `effects`, printing every
/// step to `out` in the seller's words, and — when the trigger pays — journals the attempt and its
/// outcome (`fee_remit_attempts`) so a failing payout is visible. Refusals at the threshold
/// ([`Refusal::is_threshold`]) are the expected steady state and are not journaled.
///
/// `Err` is an effect failure (LNURL host, mint quote, reconciliation query) before any plan was
/// journaled, or the store failing to write; nothing was paid. A melt that fails AFTER the plan is
/// [`RemitOutcome::MeltFailed`], not `Err`, because there is a row to reconcile.
///
/// Order of operations, and why:
/// 1. Reconcile any `planned` row first — a payment may be in flight from an interrupted run, and
///    nothing may be planned on top of it. PAID ⇒ settle; FAILED ⇒ release; UNPAID / no quote ⇒
///    release only if the row is this process's own or its owner's lease has run out, else refuse;
///    PENDING ⇒ refuse this run ([`reconcile_decision`]).
/// 2. Read the unremitted total. Zero ⇒ refuse (nothing to do), before any network.
/// 3. Resolve the destination; refuse below its minimum with the shortfall (expected for small
///    sellers, not an error).
/// 4. Probe the melt fee reserve on an invoice for the GROSS, then invoice for `gross − reserve` so
///    the fee comes out of the accrued amount — a seller never pays more than it accrued — and check
///    the second quote still fits.
/// 5. Print the plan. A dry run stops here.
/// 6. Journal the plan (pins the receipts, records this process as owner under [`REMIT_LEASE`];
///    refuses a duplicate), pass the pre-spend gate (ownership re-confirmed with [`SPEND_MARGIN`] of
///    lease left; the ceiling checked inside the melt against the quote raised at payment time),
///    melt, settle. A melt REFUSED at the ceiling released the row (nothing was spent); a melt
///    ERROR leaves the row `planned` for step 1 of the next run.
pub fn remit(
    store: &SellerStore,
    effects: &mut dyn RemitEffects,
    trigger: RemitTrigger,
    now_unix: i64,
    out: &mut dyn Write,
) -> Result<RemitOutcome, String> {
    let mut trace = AttemptTrace::default();
    let result = remit_inner(store, effects, trigger, now_unix, out, &mut trace);
    let attempt = trigger.journal_as().and_then(|journal_trigger| {
        attempt_record(store, journal_trigger, now_unix, &trace, &result)
    });
    if let Some(attempt) = attempt
        && let Err(error) = store.record_remit_attempt(&attempt)
    {
        let _ = writeln!(
            out,
            "WARNING: could not journal this attempt ({error}); the outcome above stands."
        );
    }
    result
}

/// The attempt row for a finished run, or `None` when the run is not an attempt (a dry run, or a
/// refusal at the threshold).
fn attempt_record(
    store: &SellerStore,
    trigger: RemitAttemptTrigger,
    now_unix: i64,
    trace: &AttemptTrace,
    result: &Result<RemitOutcome, String>,
) -> Option<RemitAttempt> {
    let (outcome, detail, remittance_id) = match result {
        Ok(RemitOutcome::DryRun) => return None,
        Ok(RemitOutcome::Refused(refusal)) if refusal.is_threshold() => return None,
        Ok(RemitOutcome::Paid {
            remittance_id,
            net_sats,
            melt_fee_sats,
        }) => (
            RemitAttemptOutcome::Paid,
            format!(
                "paid {net_sats} sats to {PLATFORM_FEE_ADDRESS} (melt fee {melt_fee_sats} sats)"
            ),
            Some(remittance_id.clone()),
        ),
        Ok(RemitOutcome::Refused(refusal)) => (
            RemitAttemptOutcome::Refused,
            refusal.to_string(),
            trace.remittance_id.clone(),
        ),
        Ok(RemitOutcome::MeltFailed {
            remittance_id,
            error,
        }) => (
            RemitAttemptOutcome::Failed,
            format!("melt failed: {error}"),
            Some(remittance_id.clone()),
        ),
        Ok(RemitOutcome::MeltRefused {
            remittance_id,
            reason,
        }) => (
            RemitAttemptOutcome::Failed,
            format!("refused before spending: {reason}"),
            Some(remittance_id.clone()),
        ),
        Err(error) => (
            RemitAttemptOutcome::Failed,
            error.clone(),
            trace.remittance_id.clone(),
        ),
    };
    // A run that failed before it read the ledger still journals the balance it was attempting.
    let unremitted = trace.unremitted.or_else(|| {
        store
            .accrued_fees()
            .ok()
            .map(|accrued| accrued.unremitted_fee_sats)
    });
    Some(RemitAttempt {
        attempt_id: 0,
        started_at_unix: now_unix,
        trigger,
        unremitted_sats: unremitted.unwrap_or(0),
        outcome,
        detail,
        remittance_id,
    })
}

fn remit_inner(
    store: &SellerStore,
    effects: &mut dyn RemitEffects,
    trigger: RemitTrigger,
    now_unix: i64,
    out: &mut dyn Write,
    trace: &mut AttemptTrace,
) -> Result<RemitOutcome, String> {
    // 0. For the operator: what the recent attempts did. The collect path skips this — its output is
    //    the node's log, and the journal is what the command prints.
    if trigger != RemitTrigger::Collect {
        print_recent_attempts(store, out)?;
    }

    // 1. Reconcile an in-flight attempt before anything else — under the release rule
    //    (`reconcile_decision`): never a live payer's intent.
    if let Some(active) = store
        .in_flight_remittance()
        .map_err(|error| format!("read remittances: {error}"))?
    {
        trace.remittance_id = Some(active.remittance_id.clone());
        let _ = writeln!(
            out,
            "Reconciling in-flight remittance {} (planned at unix {} by {}, lease until unix {}: {} sats to {}, gross {} sats)",
            active.remittance_id,
            active.created_at_unix,
            active.owner.as_deref().unwrap_or("nobody recorded"),
            active
                .lease_until_unix
                .map(|until| until.to_string())
                .unwrap_or_else(|| "none recorded".to_owned()),
            active.net_sats,
            active.destination,
            active.gross_sats
        );
        let status = effects.melt_status(&active.bolt11)?;
        match reconcile_decision(&active, status.as_ref(), effects.owner(), now_unix) {
            Reconcile::Settle => {
                let status = status.expect("Settle is decided only on a PAID status");
                let settlement = RemitSettlement {
                    net_paid_sats: Some(status.amount_sats),
                    melt_fee_sats: None,
                    melt_fee_reserve_sats: Some(status.fee_reserve_sats),
                    melt_quote_id: Some(status.quote_id.clone()),
                    settled_by: SettledBy::Reconciliation,
                };
                store
                    .settle_remittance(&active.remittance_id, &settlement, now_unix)
                    .map_err(|error| format!("record settled remittance: {error}"))?;
                let _ = writeln!(
                    out,
                    "  mint {} reports melt quote {} PAID — recorded as settled by reconciliation: {} sats reached {}; melt fee at most {} sats (the quote's reserve — the mint reports a quote paid by another run as PAID, not what it kept, so the actual fee is recorded as not observed)",
                    status.mint_url,
                    status.quote_id,
                    status.amount_sats,
                    active.destination,
                    status.fee_reserve_sats
                );
            }
            Reconcile::Release(reason) => {
                store
                    .fail_remittance(&active.remittance_id, now_unix)
                    .map_err(|error| format!("record failed remittance: {error}"))?;
                let _ = writeln!(
                    out,
                    "  {reason}; released {} sats back to unremitted",
                    active.gross_sats
                );
            }
            Reconcile::Hold(refusal) => {
                let _ = writeln!(
                    out,
                    "  {}. REFUSED — nothing moved by this run; re-run later to reconcile.",
                    match &refusal {
                        Refusal::Settling { .. } => format!(
                            "mint {} reports melt quote {} {}: the payment is still settling",
                            status.as_ref().map(|s| s.mint_url.as_str()).unwrap_or("?"),
                            status.as_ref().map(|s| s.quote_id.as_str()).unwrap_or("?"),
                            status
                                .as_ref()
                                .map(|s| s.state.to_string())
                                .unwrap_or_default()
                        ),
                        other => other.to_string(),
                    }
                );
                return Ok(RemitOutcome::Refused(refusal));
            }
        }
        trace.remittance_id = None;
    }

    // 2. What is owed.
    let accrued = store
        .accrued_fees()
        .map_err(|error| format!("read receipts: {error}"))?;
    let gross = accrued.unremitted_fee_sats;
    trace.unremitted = Some(gross);
    let _ = writeln!(
        out,
        "Accrued platform fee: {} sats all-time — {} sats remitted, {} sats unremitted",
        accrued.total_fee_sats, accrued.remitted_fee_sats, gross
    );
    if gross == 0 {
        let _ = writeln!(out, "Nothing to remit. REFUSED — nothing moved.");
        return Ok(RemitOutcome::Refused(Refusal::NothingUnremitted));
    }

    // 3. The destination and its bounds.
    let address =
        LightningAddress::parse(PLATFORM_FEE_ADDRESS).map_err(|error| error.to_string())?;
    let pay = effects.pay_request(&address)?;
    let min_sats = pay.min_sendable_sats();
    let max_sats = pay.max_sendable_sats();
    let _ = writeln!(
        out,
        "Destination: {address} (LNURL-pay; accepts {min_sats} to {max_sats} sats)"
    );
    if gross < min_sats {
        let _ = writeln!(
            out,
            "REFUSED — unremitted {gross} sats is below the destination's minimum of {min_sats} sats ({} sats short). The balance accumulates until it clears the minimum. Nothing moved.",
            min_sats - gross
        );
        return Ok(RemitOutcome::Refused(Refusal::BelowMinimum {
            unremitted: gross,
            min_sats,
        }));
    }
    if gross > max_sats {
        let _ = writeln!(
            out,
            "REFUSED — unremitted {gross} sats exceeds the destination's maximum of {max_sats} sats; this path remits the whole balance or nothing. Nothing moved."
        );
        return Ok(RemitOutcome::Refused(Refusal::AboveMaximum {
            unremitted: gross,
            max_sats,
        }));
    }

    // 4. The melt fee comes OUT of the gross. Probe the reserve on the gross, then invoice the net.
    let probe = effects.invoice(&pay, gross)?;
    let probe_estimate = effects.melt_estimate(&probe.bolt11)?;
    if probe_estimate.amount_sats != gross {
        return Err(format!(
            "mint {} quoted {} sats for a {gross}-sat invoice; refusing",
            probe_estimate.mint_url, probe_estimate.amount_sats
        ));
    }
    let reserve = probe_estimate.fee_reserve_sats;
    if reserve >= gross {
        let _ = writeln!(
            out,
            "REFUSED — mint {} needs a melt fee reserve of {reserve} sats to pay {gross} sats, which leaves nothing for the destination. The balance accumulates. Nothing moved.",
            probe_estimate.mint_url
        );
        return Ok(RemitOutcome::Refused(Refusal::ReserveDoesNotFit {
            gross,
            reserve,
        }));
    }
    let net = gross - reserve;
    if net < min_sats {
        let _ = writeln!(
            out,
            "REFUSED — after the mint's melt fee reserve ({reserve} sats) the {gross} sats unremitted leaves {net} sats, below the destination's minimum of {min_sats} sats ({} sats short). The balance accumulates. Nothing moved.",
            min_sats - net
        );
        return Ok(RemitOutcome::Refused(Refusal::ReserveDoesNotFit {
            gross,
            reserve,
        }));
    }
    let (invoice, estimate) = if reserve == 0 {
        (probe, probe_estimate)
    } else {
        let invoice = effects.invoice(&pay, net)?;
        let estimate = effects.melt_estimate(&invoice.bolt11)?;
        if estimate.amount_sats != net {
            return Err(format!(
                "mint {} quoted {} sats for a {net}-sat invoice; refusing",
                estimate.mint_url, estimate.amount_sats
            ));
        }
        (invoice, estimate)
    };
    let debit_ceiling = net.saturating_add(estimate.fee_reserve_sats);
    if debit_ceiling > gross {
        let _ = writeln!(
            out,
            "REFUSED — mint {} quotes a {} sats fee reserve on {net} sats, so up to {debit_ceiling} sats would leave the wallet against {gross} sats accrued. A seller never pays more than it accrued. Nothing moved.",
            estimate.mint_url, estimate.fee_reserve_sats
        );
        return Ok(RemitOutcome::Refused(Refusal::ReserveDoesNotFit {
            gross,
            reserve: estimate.fee_reserve_sats,
        }));
    }

    // 5. The plan, in the seller's words.
    let _ = writeln!(
        out,
        "Plan:\n  unremitted platform fee (gross): {gross} sats\n  mint melt fee reserve (ceiling): {} sats — taken out of the gross, never on top\n  invoice amount ({address} receives): {net} sats\n  leaves your wallet: at most {debit_ceiling} sats (≤ {gross}); unused reserve returns as change\n  mint: {} (melt quote {})\n  invoice payment hash: {}",
        estimate.fee_reserve_sats, estimate.mint_url, estimate.quote_id, invoice.payment_hash
    );
    if !trigger.pays() {
        let _ = writeln!(
            out,
            "DRY RUN — nothing moved. Re-run with --confirm to pay {net} sats to {address}."
        );
        return Ok(RemitOutcome::DryRun);
    }

    // 6. Journal (as this process, under a lease), pass the pre-spend gate, pay under the ceiling,
    //    settle.
    let plan = RemittancePlan {
        payment_hash: invoice.payment_hash.clone(),
        gross_sats: gross,
        net_sats: net,
        melt_fee_reserve_sats: estimate.fee_reserve_sats,
        destination: address.to_string(),
        bolt11: invoice.bolt11.clone(),
        melt_quote_id: Some(estimate.quote_id.clone()),
    };
    let lease_until_unix = now_unix.saturating_add(lease_secs(REMIT_LEASE));
    let planned = match store.plan_remittance(&plan, effects.owner(), lease_until_unix, now_unix) {
        Ok(planned) => planned,
        Err(PlanRefused::Store(error)) => return Err(format!("journal remittance: {error}")),
        Err(refused) => {
            let _ = writeln!(out, "REFUSED — {refused}. Nothing moved.");
            if let PlanRefused::InFlight(active) = &refused {
                trace.remittance_id = Some(active.remittance_id.clone());
            }
            return Ok(RemitOutcome::Refused(Refusal::PlanRefused(
                refused.to_string(),
            )));
        }
    };
    trace.remittance_id = Some(planned.remittance_id.clone());
    let _ = writeln!(
        out,
        "Journaled remittance {} covering {} receipt{} (owner {}, lease until unix {lease_until_unix}); paying...",
        planned.remittance_id,
        planned.receipts,
        if planned.receipts == 1 { "" } else { "s" },
        effects.owner()
    );
    effects.after_plan(&planned);

    // The pre-spend gate, both conditions, before a single proof is touched:
    // (a) ownership — the row is still planned, still ours, with the spending margin left in the
    //     lease (another process may release it the moment the lease ends);
    // (b) the ceiling — enforced INSIDE the melt against the quote raised at payment time.
    match store
        .confirm_remittance_ownership(
            &planned.remittance_id,
            effects.owner(),
            now_unix,
            lease_secs(SPEND_MARGIN),
        )
        .map_err(|error| format!("re-read remittance {}: {error}", planned.remittance_id))?
    {
        Ok(_) => {}
        Err(lost) => {
            // Ours but too little lease left: nothing was spent, so release our own row. Not ours
            // (or no longer planned): another process holds or resolved it — touch nothing.
            if matches!(lost, OwnershipLost::LeaseTooShort { .. }) {
                store
                    .fail_remittance(&planned.remittance_id, now_unix)
                    .map_err(|error| format!("release remittance: {error}"))?;
            }
            let reason = lost.to_string();
            let _ = writeln!(
                out,
                "REFUSED before spending — {reason}. Nothing moved by this run{}.",
                if matches!(lost, OwnershipLost::LeaseTooShort { .. }) {
                    format!(
                        "; released {} sats back to unremitted for the next attempt",
                        planned.gross_sats
                    )
                } else {
                    String::new()
                }
            );
            return Ok(RemitOutcome::Refused(Refusal::OwnershipLost {
                remittance_id: planned.remittance_id,
                reason,
            }));
        }
    }
    let ceiling = MeltCeiling {
        max_debit_sats: gross,
        invoice_sats: net,
        planned_quote_id: Some(estimate.quote_id.clone()),
    };
    match effects.melt(&invoice.bolt11, &ceiling) {
        Ok(outcome) => {
            let settlement = RemitSettlement {
                net_paid_sats: Some(outcome.paid_sats),
                melt_fee_sats: Some(outcome.fee_sats),
                melt_fee_reserve_sats: Some(outcome.fee_reserve_sats),
                melt_quote_id: Some(outcome.quote_id.clone()),
                settled_by: SettledBy::Melt,
            };
            let settled = store
                .settle_remittance(&planned.remittance_id, &settlement, now_unix)
                .map_err(|error| {
                    format!(
                        "PAID {} sats (melt fee {} sats, quote {}) but could not record the settlement: {error}. \
                         Remittance {} stays planned; the next attempt reconciles it with the mint before paying anything else.",
                        outcome.paid_sats, outcome.fee_sats, outcome.quote_id, planned.remittance_id
                    )
                })?;
            let debit = outcome.paid_sats.saturating_add(outcome.fee_sats);
            let _ = writeln!(
                out,
                "PAID — remittance {} settled\n  gross discharged: {} sats\n  melt fee taken by the mint: {} sats (quote {} reserved {} sats; ceiling {gross} sats held at the moment of spending)\n  net paid to {}: {} sats\n  stays in your wallet (unused reserve): {} sats\n  wallet balance now: {} sats at {}\n  receipts discharged: {}",
                settled.remittance_id,
                settled.gross_sats,
                outcome.fee_sats,
                outcome.quote_id,
                outcome.fee_reserve_sats,
                settled.destination,
                outcome.paid_sats,
                gross.saturating_sub(debit),
                outcome.balance_sats,
                outcome.mint_url,
                settled.receipts
            );
            if debit > gross {
                // Belt behind the braces: the melt refuses a quote over the ceiling before spending,
                // and the mint's fee is at most its reserve, so this line should never print.
                let _ = writeln!(
                    out,
                    "WARNING: the mint debited {debit} sats against {gross} sats accrued — above the ceiling the melt was admitted under. Recorded as settled; report this."
                );
            }
            Ok(RemitOutcome::Paid {
                remittance_id: settled.remittance_id,
                net_sats: outcome.paid_sats,
                melt_fee_sats: outcome.fee_sats,
            })
        }
        Err(MeltFailure::RefusedBeforeSpending(reason)) => {
            // Typed as "nothing left the wallet", and the row is ours: release it ourselves so the
            // balance is unremitted again for the next attempt (which re-quotes).
            store
                .fail_remittance(&planned.remittance_id, now_unix)
                .map_err(|error| format!("release remittance after refusal: {error}"))?;
            let _ = writeln!(
                out,
                "REFUSED before spending — {reason}.\n  A seller never pays more than it accrued: the ceiling is {gross} sats, enforced against the quote the mint raised for the payment. Nothing left the wallet; released {gross} sats back to unremitted. The next attempt re-quotes.",
            );
            Ok(RemitOutcome::MeltRefused {
                remittance_id: planned.remittance_id,
                reason,
            })
        }
        Err(MeltFailure::Failed(error)) => {
            let _ = writeln!(
                out,
                "melt failed: {error}\n  remittance {} stays journaled as planned. The next attempt (automatic, or `maxplayer seller fees remit`) reconciles it with the mint: settled if the payment landed, released if it did not. Nothing else was attempted.",
                planned.remittance_id
            );
            Ok(RemitOutcome::MeltFailed {
                remittance_id: planned.remittance_id,
                error,
            })
        }
    }
}

/// A `Duration` as whole unix seconds, for lease arithmetic.
fn lease_secs(duration: Duration) -> i64 {
    i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
}

fn print_recent_attempts(store: &SellerStore, out: &mut dyn Write) -> Result<(), String> {
    let attempts = store
        .recent_remit_attempts(RECENT_ATTEMPTS_SHOWN)
        .map_err(|error| format!("read remit attempts: {error}"))?;
    if attempts.is_empty() {
        let _ = writeln!(out, "Recent attempts: none journaled yet");
        return Ok(());
    }
    let _ = writeln!(
        out,
        "Recent attempts (newest first, last {}):",
        RECENT_ATTEMPTS_SHOWN
    );
    for attempt in &attempts {
        let _ = writeln!(
            out,
            "  unix {}: {} attempt saw {} sats unremitted — {}: {}{}",
            attempt.started_at_unix,
            match attempt.trigger {
                RemitAttemptTrigger::Collect => "automatic (after collect)",
                RemitAttemptTrigger::Retry => "automatic (retry tick)",
                RemitAttemptTrigger::Command => "operator (--confirm)",
            },
            attempt.unremitted_sats,
            attempt.outcome.as_str().to_uppercase(),
            attempt.detail,
            match &attempt.remittance_id {
                Some(id) => format!(" [remittance {id}]"),
                None => String::new(),
            }
        );
    }
    Ok(())
}

/// What the collect path gets back: the outcome and every line the attempt printed, for the node's
/// log. Never an error the caller has to handle — that is the point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemitReport {
    pub outcome: Result<RemitOutcome, String>,
    pub lines: Vec<String>,
}

impl RemitReport {
    /// A refusal at the threshold — the expected steady state; the caller may log it briefly.
    pub fn is_quiet(&self) -> bool {
        matches!(&self.outcome, Ok(RemitOutcome::Refused(refusal)) if refusal.is_threshold())
    }

    /// One line saying how the attempt ended.
    pub fn summary(&self) -> String {
        match &self.outcome {
            Ok(RemitOutcome::Paid {
                remittance_id,
                net_sats,
                melt_fee_sats,
            }) => format!(
                "PAID {net_sats} sats to {PLATFORM_FEE_ADDRESS} (melt fee {melt_fee_sats} sats), remittance {remittance_id}"
            ),
            Ok(RemitOutcome::DryRun) => "dry run; nothing moved".to_owned(),
            Ok(RemitOutcome::Refused(refusal)) if refusal.is_threshold() => format!(
                "nothing moved ({refusal}); the balance accumulates until it clears the destination's minimum"
            ),
            Ok(RemitOutcome::Refused(refusal)) => {
                format!("REFUSED, nothing moved: {refusal}; the balance stays unremitted")
            }
            Ok(RemitOutcome::MeltFailed {
                remittance_id,
                error,
            }) => format!(
                "melt FAILED ({error}); remittance {remittance_id} stays planned and is reconciled on the next attempt"
            ),
            Ok(RemitOutcome::MeltRefused {
                remittance_id,
                reason,
            }) => format!(
                "melt REFUSED before spending ({reason}); remittance {remittance_id} released, the balance stays unremitted and the next attempt re-quotes"
            ),
            Err(error) => format!(
                "attempt FAILED ({error}); the balance stays unremitted and the node retries with backoff while it runs"
            ),
        }
    }

    /// Whether this attempt counts as a FAILURE for pacing ([`RemitBackoff::observe`]): it meant to
    /// pay and did not, for a reason that is not the steady state. `Err` (an effect failed),
    /// `MeltFailed`, `MeltRefused`, and every refusal that is not at the threshold — the balance
    /// stays owed and hammering the same host or mint every 30 s would not change that. A threshold
    /// refusal, a payment and a dry run are not failures.
    pub fn is_failure(&self) -> bool {
        match &self.outcome {
            Err(_) | Ok(RemitOutcome::MeltFailed { .. }) | Ok(RemitOutcome::MeltRefused { .. }) => {
                true
            }
            Ok(RemitOutcome::Refused(refusal)) => !refusal.is_threshold(),
            Ok(RemitOutcome::Paid { .. }) | Ok(RemitOutcome::DryRun) => false,
        }
    }
}

/// **The node's attempt** — [`remit`] under a paying node trigger ([`RemitTrigger::Collect`] or
/// [`RemitTrigger::Retry`]), with every error caught into the report. Nothing here can fail the
/// caller: on the collect path the receipt is already journaled and the job already marked paid
/// before this runs; a failure leaves the balance unremitted for the retry tick (and the next
/// collect) to try again.
pub fn remit_best_effort(
    store: &SellerStore,
    effects: &mut dyn RemitEffects,
    trigger: RemitTrigger,
    now_unix: i64,
) -> RemitReport {
    debug_assert!(
        matches!(trigger, RemitTrigger::Collect | RemitTrigger::Retry),
        "the node's best-effort attempt runs under a node trigger, never the operator's"
    );
    let mut out = Vec::new();
    let outcome = remit(store, effects, trigger, now_unix, &mut out);
    let lines = String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_owned)
        .collect();
    RemitReport { outcome, lines }
}

/// [`remit_best_effort`] over the shipped [`LiveEffects`] — what the seller node runs, on a thread
/// of its own, after a receipt is journaled `Collected::New` and on each retry tick. A failure to
/// build the https client is itself journaled as a failed attempt, so even that is visible in the
/// read-out.
pub fn remit_live_best_effort(
    store: &SellerStore,
    home: MaxplayerHome,
    trigger: RemitTrigger,
    now_unix: i64,
) -> RemitReport {
    match LiveEffects::new(home) {
        Ok(mut effects) => remit_best_effort(store, &mut effects, trigger, now_unix),
        Err(error) => {
            let error = format!("build https client for LNURL: {error}");
            let unremitted = store
                .accrued_fees()
                .map(|accrued| accrued.unremitted_fee_sats)
                .unwrap_or(0);
            let journaled = store.record_remit_attempt(&RemitAttempt {
                attempt_id: 0,
                started_at_unix: now_unix,
                trigger: trigger.journal_as().unwrap_or(RemitAttemptTrigger::Collect),
                unremitted_sats: unremitted,
                outcome: RemitAttemptOutcome::Failed,
                detail: error.clone(),
                remittance_id: None,
            });
            let mut lines = vec![error.clone()];
            if let Err(journal_error) = journaled {
                lines.push(format!("could not journal the attempt: {journal_error}"));
            }
            RemitReport {
                outcome: Err(error),
                lines,
            }
        }
    }
}

// ---- retry pacing (stage 2a, addendum 2) ------------------------------------------------------

/// The retry tick's base delay: the first attempt after boot waits at least this long
/// ([`RemitBackoff::boot_delay`]), and a streak of failures doubles it from here.
pub const RETRY_BASE: Duration = Duration::from_secs(30);
/// The ceiling the doubling stops at. A node whose payout host is down keeps trying at most this
/// often, for as long as it runs.
pub const RETRY_CAP: Duration = Duration::from_secs(30 * 60);

/// How one observed attempt moved the pacing — what the loop logs, and how loudly (addendum 2 §5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pacing {
    /// The steady state: nothing owed, or not enough to clear the destination's minimum. Not a
    /// failure — the streak is untouched and nothing is logged at full volume.
    Idle,
    /// A payment with no failure streak behind it.
    Paid,
    /// The first failure of a streak — the one to log in full, with its error.
    FirstFailure,
    /// Another failure in the same streak. `entered_cap` marks the transition into the 30-minute
    /// ceiling — a line an operator wants once, not every half hour.
    RepeatFailure { streak: u32, entered_cap: bool },
    /// A payment that ended a streak: how many attempts failed first, and for how long the fee sat
    /// owed while they did. The line an operator wants when they ask "did it ever go out?".
    Recovered {
        failed_attempts: u32,
        owed_for_secs: i64,
    },
}

/// The retry tick's backoff: **base 30 s, doubling on consecutive failures, capped at 30 minutes,
/// with full jitter**; reset to base by a successful remittance and by nothing else.
///
/// One instance per node, shared by the loop's tick and the collect path's thread, so a success on
/// either path resets it and a failure on either escalates it: both back off against the same LNURL
/// host and the same mint.
///
/// **Why full jitter, and why nobody may "simplify" it away:** every seller's node backs off against
/// the same payout host and the same mint. If they all slept the computed delay, an outage would end
/// with every node in the fleet retrying in the same second — the correlated burst that turns a
/// recovered host back into a failed one. So the delay actually slept is a uniform random value in
/// `[0, computed_delay]`, not the delay plus a small wobble ([`Self::next_delay`]). The first
/// attempt after boot is the one exception to "from zero" (addendum 3 RULING 1): it waits the full
/// base and THEN a jitter in `[0, base]` — `[30 s, 60 s]` — so a fleet restarting together neither
/// attempts at once nor attempts at startup ([`Self::boot_delay`]). Zero is never a legal first
/// delay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemitBackoff {
    base: Duration,
    cap: Duration,
    /// Consecutive failures observed since the last success.
    streak: u32,
    /// When the current streak began (unix seconds), for the recovery line.
    streak_since_unix: Option<i64>,
}

impl Default for RemitBackoff {
    fn default() -> Self {
        Self::new()
    }
}

impl RemitBackoff {
    /// The shipped bounds: [`RETRY_BASE`] doubling to [`RETRY_CAP`].
    pub fn new() -> Self {
        Self::with_bounds(RETRY_BASE, RETRY_CAP)
    }

    /// Explicit bounds — for tests that must not sleep 30 minutes. `cap` below `base` is clamped
    /// to `base`.
    pub fn with_bounds(base: Duration, cap: Duration) -> Self {
        Self {
            base,
            cap: cap.max(base),
            streak: 0,
            streak_since_unix: None,
        }
    }

    /// Consecutive failures so far (0 = healthy).
    pub fn streak(&self) -> u32 {
        self.streak
    }

    /// The delay the current streak computes to, BEFORE jitter: `base × 2^streak`, capped.
    pub fn computed_delay(&self) -> Duration {
        let mut delay = self.base;
        for _ in 0..self.streak {
            if delay >= self.cap {
                break;
            }
            delay = delay.saturating_mul(2);
        }
        delay.min(self.cap)
    }

    /// Whether the doubling has reached the cap.
    pub fn at_cap(&self) -> bool {
        self.computed_delay() >= self.cap
    }

    /// The delay to actually sleep before the next attempt: full jitter over
    /// [`Self::computed_delay`], drawn from the OS RNG. Never above the computed delay, never below
    /// zero. If the RNG is unavailable (it should never be), sleeps the full computed delay — later
    /// is the safe direction.
    pub fn next_delay(&self) -> Duration {
        jittered(self.computed_delay(), os_entropy())
    }

    /// The delay before the FIRST attempt after boot (addendum 3 RULING 1): one full base delay,
    /// plus an additive jitter in `[0, base]` — `[base, 2 × base]`, never less than the base, never
    /// zero. See [`boot_delay_for`] for the pure form.
    pub fn boot_delay(&self) -> Duration {
        boot_delay_for(self.base, os_entropy())
    }

    /// Fold one finished attempt into the pacing and say what changed. Failures
    /// ([`RemitReport::is_failure`]) lengthen the streak; a payment resets it to base; the steady
    /// state ([`Pacing::Idle`]) leaves it exactly as it was — a balance under the threshold neither
    /// escalates nor resets.
    pub fn observe(&mut self, report: &RemitReport, now_unix: i64) -> Pacing {
        if report.is_failure() {
            let was_at_cap = self.at_cap();
            self.streak = self.streak.saturating_add(1);
            if self.streak == 1 {
                self.streak_since_unix = Some(now_unix);
                return Pacing::FirstFailure;
            }
            return Pacing::RepeatFailure {
                streak: self.streak,
                entered_cap: !was_at_cap && self.at_cap(),
            };
        }
        match &report.outcome {
            Ok(RemitOutcome::Paid { .. }) => {
                let failed_attempts = self.streak;
                let owed_for_secs = self
                    .streak_since_unix
                    .map(|since| now_unix.saturating_sub(since).max(0))
                    .unwrap_or(0);
                self.streak = 0;
                self.streak_since_unix = None;
                if failed_attempts == 0 {
                    Pacing::Paid
                } else {
                    Pacing::Recovered {
                        failed_attempts,
                        owed_for_secs,
                    }
                }
            }
            _ => Pacing::Idle,
        }
    }
}

/// Eight bytes from the OS RNG as a `u64`. If the RNG is unavailable (it should never be), the
/// maximum — which every caller maps to the LONGEST delay: later is the safe direction.
fn os_entropy() -> u64 {
    let mut bytes = [0u8; 8];
    match getrandom::fill(&mut bytes) {
        Ok(()) => u64::from_le_bytes(bytes),
        Err(_) => u64::MAX,
    }
}

/// The boot delay's pure form: `base + jittered(base, entropy)`, so `[base, 2 × base]` — `entropy
/// = 0` gives exactly the base, never less. Saturates rather than overflowing.
pub fn boot_delay_for(base: Duration, entropy: u64) -> Duration {
    base.saturating_add(jittered(base, entropy))
}

/// Full jitter: a uniform point in `[0, computed]` chosen by `entropy` (`0` ⇒ zero, `u64::MAX` ⇒
/// the whole computed delay). Pure, so the bound is tested without a clock or an RNG.
pub fn jittered(computed: Duration, entropy: u64) -> Duration {
    // Integer arithmetic, scaled in two parts so nothing overflows even at `Duration::MAX`:
    // `secs × entropy` and `subsec_nanos × entropy` each fit u128 (u64 × u64), and the remainder
    // of the seconds part becomes nanoseconds.
    let scale = u128::from(u64::MAX);
    let entropy = u128::from(entropy);
    let secs_scaled = u128::from(computed.as_secs()) * entropy;
    let whole_secs = secs_scaled / scale;
    let carry_nanos = (secs_scaled % scale) * 1_000_000_000 / scale;
    let subsec_nanos = u128::from(computed.subsec_nanos()) * entropy / scale;
    let nanos = carry_nanos + subsec_nanos;
    let secs = Duration::from_secs(u64::try_from(whole_secs).unwrap_or(u64::MAX));
    secs.checked_add(Duration::from_nanos(
        u64::try_from(nanos).unwrap_or(u64::MAX),
    ))
    .unwrap_or(computed)
    .min(computed)
}

/// **Single-flight for the node's two paths** (addendum 2 §3): one remittance attempt in flight per
/// process, ever. The collect thread and the loop's tick can reach the entry point at the same time;
/// whichever cannot take the permit **skips and returns** — it does not queue, block or fail. This is
/// a liveness device, not the correctness argument: the store's one-`planned`-row rule is what makes
/// a double payment impossible, including against `maxplayer seller fees remit --confirm` in another
/// process, which this guard cannot see.
#[derive(Debug, Clone, Default)]
pub struct RemitFlight(Arc<AtomicBool>);

/// Held by the one attempt in flight; the slot frees when it drops (including on a panic).
#[derive(Debug)]
pub struct RemitPermit(Arc<AtomicBool>);

impl RemitFlight {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the slot if it is free. `None` means an attempt is already in flight: skip.
    pub fn try_acquire(&self) -> Option<RemitPermit> {
        self.0
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RemitPermit(Arc::clone(&self.0)))
    }

    /// Whether an attempt holds the slot right now.
    pub fn in_flight(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

impl Drop for RemitPermit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Scripted effects for tests, shared with `seller_node::run`'s collect-path tests.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};

    use super::{MeltFailure, RemitEffects};
    use crate::lnurl_pay::{LightningAddress, PayRequest, ResolvedInvoice, Url};
    use crate::seller_node::store::FeeRemittance;
    use crate::wallet_ops::{MeltCeiling, MeltEstimate, MeltOutcome, MeltQuoteStatus};

    /// A rendezvous a test uses to PAUSE one attempt at a chosen point (after the plan is journaled,
    /// or inside the melt) while another attempt runs against the same store — the deterministic
    /// interleaving addendum 3 §2.2 asks for. The paused side calls [`Self::arrive_and_wait`]; the
    /// test waits for [`Self::wait_arrived`], does what it wants, then [`Self::release`]s.
    pub(crate) struct Gate {
        arrived: AtomicBool,
        released: Mutex<bool>,
        cv: Condvar,
    }

    impl Gate {
        pub(crate) fn new() -> Arc<Self> {
            Arc::new(Self {
                arrived: AtomicBool::new(false),
                released: Mutex::new(false),
                cv: Condvar::new(),
            })
        }

        pub(crate) fn arrive_and_wait(&self) {
            self.arrived.store(true, Ordering::SeqCst);
            let mut released = self.released.lock().unwrap_or_else(|e| e.into_inner());
            while !*released {
                released = self.cv.wait(released).unwrap_or_else(|e| e.into_inner());
            }
        }

        pub(crate) fn arrived(&self) -> bool {
            self.arrived.load(Ordering::SeqCst)
        }

        /// Spin (bounded) until the paused side has arrived.
        pub(crate) fn wait_arrived(&self, bound: std::time::Duration) {
            let deadline = std::time::Instant::now() + bound;
            while !self.arrived() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "the paused attempt never reached the gate"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }

        pub(crate) fn release(&self) {
            let mut released = self.released.lock().unwrap_or_else(|e| e.into_inner());
            *released = true;
            self.cv.notify_all();
        }
    }

    /// Scripted effects. `reserve_for(amount)` is the mint's fee reserve policy at ESTIMATE time;
    /// `live_reserve_for`, when set, is the reserve the quote raised at PAYMENT time carries (the
    /// two can differ — addendum 3 §1); `melt_results` are consumed in order; `status` answers the
    /// reconciliation query; `pay_request_error` makes the LNURL host unreachable. Every call is
    /// logged so a test can assert what was — and was not — touched; `melt_counter`, when set,
    /// counts ACTUAL debits across Fakes on different threads (a melt refused at the ceiling is not
    /// a debit and is logged in `ceiling_refusals` instead). `owner` is the process this Fake
    /// speaks as; `invoice_tag` makes one Fake's invoices distinct from another's, as two real LNURL
    /// calls would be.
    pub(crate) struct Fake {
        pub(crate) owner: String,
        pub(crate) invoice_tag: String,
        pub(crate) min_msat: u64,
        pub(crate) max_msat: u64,
        pub(crate) reserve_for: Box<dyn Fn(u64) -> u64 + Send>,
        pub(crate) live_reserve_for: Option<Box<dyn Fn(u64) -> u64 + Send>>,
        pub(crate) melt_results: Vec<Result<(u64, u64), String>>,
        pub(crate) status: Result<Option<MeltQuoteStatus>, String>,
        pub(crate) pay_request_error: Option<String>,
        pub(crate) pay_requests: usize,
        pub(crate) invoices: Vec<u64>,
        pub(crate) estimates: Vec<String>,
        pub(crate) melts: Vec<String>,
        pub(crate) ceiling_refusals: Vec<String>,
        pub(crate) status_calls: Vec<String>,
        pub(crate) melt_counter: Option<Arc<AtomicUsize>>,
        pub(crate) plan_gate: Option<Arc<Gate>>,
        pub(crate) melt_gate: Option<Arc<Gate>>,
        pub(crate) planned_seen: Vec<FeeRemittance>,
    }

    impl Fake {
        pub(crate) fn new(reserve_for: impl Fn(u64) -> u64 + Send + 'static) -> Self {
            Self {
                owner: "fake-owner".to_owned(),
                invoice_tag: String::new(),
                min_msat: 1000,
                max_msat: 1_000_000_000,
                reserve_for: Box::new(reserve_for),
                live_reserve_for: None,
                melt_results: Vec::new(),
                status: Ok(None),
                pay_request_error: None,
                pay_requests: 0,
                invoices: Vec::new(),
                estimates: Vec::new(),
                melts: Vec::new(),
                ceiling_refusals: Vec::new(),
                status_calls: Vec::new(),
                melt_counter: None,
                plan_gate: None,
                melt_gate: None,
                planned_seen: Vec::new(),
            }
        }

        pub(crate) fn bolt11_for(amount_sats: u64, sequence: usize) -> String {
            format!("lnbc-fake-{amount_sats}-{sequence}")
        }

        pub(crate) fn hash_for(amount_sats: u64, sequence: usize) -> String {
            format!("hash-{amount_sats}-{sequence}")
        }

        fn amount_in(bolt11: &str) -> u64 {
            bolt11
                .split('-')
                .nth(2)
                .and_then(|raw| raw.parse().ok())
                .expect("fake bolt11 carries its amount")
        }
    }

    impl RemitEffects for Fake {
        fn owner(&self) -> &str {
            &self.owner
        }

        fn after_plan(&mut self, planned: &FeeRemittance) {
            self.planned_seen.push(planned.clone());
            if let Some(gate) = &self.plan_gate {
                gate.arrive_and_wait();
            }
        }

        fn pay_request(&mut self, address: &LightningAddress) -> Result<PayRequest, String> {
            assert_eq!(address.to_string(), "maxplayer@agi.cash");
            self.pay_requests += 1;
            if let Some(error) = &self.pay_request_error {
                return Err(error.clone());
            }
            Ok(PayRequest {
                callback: Url::parse("https://agi.cash/cb").unwrap(),
                min_sendable_msat: self.min_msat,
                max_sendable_msat: self.max_msat,
            })
        }

        fn invoice(
            &mut self,
            pay: &PayRequest,
            amount_sats: u64,
        ) -> Result<ResolvedInvoice, String> {
            pay.invoice_url(amount_sats)
                .map_err(|error| error.to_string())?;
            self.invoices.push(amount_sats);
            let sequence = self.invoices.len();
            Ok(ResolvedInvoice {
                bolt11: format!(
                    "{}{}",
                    Self::bolt11_for(amount_sats, sequence),
                    self.invoice_tag
                ),
                payment_hash: format!(
                    "{}{}",
                    Self::hash_for(amount_sats, sequence),
                    self.invoice_tag
                ),
                amount_sats,
                amount_msat: amount_sats * 1000,
            })
        }

        fn melt_estimate(&mut self, bolt11: &str) -> Result<MeltEstimate, String> {
            self.estimates.push(bolt11.to_owned());
            let amount_sats = Self::amount_in(bolt11);
            Ok(MeltEstimate {
                mint_url: "https://mint.example".to_owned(),
                quote_id: format!("quote-{bolt11}"),
                amount_sats,
                fee_reserve_sats: (self.reserve_for)(amount_sats),
            })
        }

        /// The wallet's melt as the shipped one behaves: raise the PAYMENT quote (its reserve is
        /// `live_reserve_for`, or the estimate's policy when unset), check it against the ceiling
        /// BEFORE anything is spent — a refusal is not a debit — then pay.
        fn melt(
            &mut self,
            bolt11: &str,
            ceiling: &MeltCeiling,
        ) -> Result<MeltOutcome, MeltFailure> {
            if let Some(gate) = &self.melt_gate {
                gate.arrive_and_wait();
            }
            let amount_sats = Self::amount_in(bolt11);
            let live_reserve = match &self.live_reserve_for {
                Some(live) => live(amount_sats),
                None => (self.reserve_for)(amount_sats),
            };
            if !ceiling.admits(amount_sats, live_reserve) {
                let reason = format!(
                    "melt refused before spending: mint https://mint.example quote paid-quote-{bolt11} would debit {} sats ({amount_sats} sats invoice + {live_reserve} sats fee reserve; planned invoice {} sats) against a ceiling of {} sats; nothing left the wallet",
                    amount_sats.saturating_add(live_reserve),
                    ceiling.invoice_sats,
                    ceiling.max_debit_sats
                );
                self.ceiling_refusals.push(reason.clone());
                return Err(MeltFailure::RefusedBeforeSpending(reason));
            }
            self.melts.push(bolt11.to_owned());
            if let Some(counter) = &self.melt_counter {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            let (paid, fee) = self.melt_results.remove(0).map_err(MeltFailure::Failed)?;
            Ok(MeltOutcome {
                mint_url: "https://mint.example".to_owned(),
                paid_sats: paid,
                fee_sats: fee,
                balance_sats: 1_000,
                quote_id: format!("paid-quote-{bolt11}"),
                fee_reserve_sats: live_reserve,
            })
        }

        fn melt_status(&mut self, bolt11: &str) -> Result<Option<MeltQuoteStatus>, String> {
            self.status_calls.push(bolt11.to_owned());
            self.status.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    use super::test_support::Fake;
    use super::*;
    use crate::seller_node::STATE_DB_FILE;
    use crate::seller_node::store::{ReceiptFees, RemittanceState};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temp_home(label: &str) -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "maxplayer-fee-remit-{label}-{}-{id}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("mk root");
        root
    }

    fn store_with_fees(label: &str, fees: &[u64]) -> (SellerStore, PathBuf) {
        let root = temp_home(label);
        let store = SellerStore::open(root.join(STATE_DB_FILE)).expect("open store");
        for (index, fee) in fees.iter().enumerate() {
            store
                .collect_receipt(
                    &format!("receipt-{index}"),
                    &format!("job-{index}"),
                    fee * 10,
                    ReceiptFees {
                        mint_fee_sats: 1,
                        fee_bps: 1000,
                        fee_sats: *fee,
                    },
                    index as i64 + 1,
                )
                .expect("collect");
        }
        (store, root)
    }

    fn run_remit(
        store: &SellerStore,
        fake: &mut Fake,
        trigger: RemitTrigger,
        now: i64,
    ) -> (RemitOutcome, String) {
        let mut out = Vec::new();
        let outcome = remit(store, fake, trigger, now, &mut out).expect("remit runs");
        (outcome, String::from_utf8(out).expect("utf8"))
    }

    fn is_paid(outcome: &RemitOutcome) -> bool {
        matches!(outcome, RemitOutcome::Paid { .. })
    }

    // §3.2: no flag ⇒ dry run. It resolves, quotes, prints every figure, and MOVES NOTHING: no melt,
    // no journal row, no attempt row, the unremitted balance untouched.
    #[test]
    fn dry_run_prints_the_plan_and_moves_nothing() {
        let (store, root) = store_with_fees("dry-run", &[10, 5]);
        let mut fake = Fake::new(|_| 2);
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::DryRun, 100);
        assert_eq!(outcome, RemitOutcome::DryRun, "{out}");
        for needle in [
            "Recent attempts: none journaled yet",
            "Accrued platform fee: 15 sats all-time — 0 sats remitted, 15 sats unremitted",
            "Destination: maxplayer@agi.cash (LNURL-pay; accepts 1 to 1000000 sats)",
            "unremitted platform fee (gross): 15 sats",
            "mint melt fee reserve (ceiling): 2 sats — taken out of the gross, never on top",
            "invoice amount (maxplayer@agi.cash receives): 13 sats",
            "leaves your wallet: at most 15 sats (≤ 15)",
            "mint: https://mint.example (melt quote quote-lnbc-fake-13-2)",
            "invoice payment hash: hash-13-2",
            "DRY RUN — nothing moved. Re-run with --confirm to pay 13 sats to maxplayer@agi.cash.",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }
        assert!(fake.melts.is_empty(), "a dry run never melts");
        assert_eq!(
            fake.invoices,
            vec![15, 13],
            "probe on the gross, then invoice the net"
        );
        assert_eq!(fake.estimates.len(), 2);
        assert!(
            store.remittances().expect("rows").is_empty(),
            "a dry run journals nothing"
        );
        assert!(
            store
                .recent_remit_attempts(10)
                .expect("attempts")
                .is_empty(),
            "a dry run is not an attempt"
        );
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 15);
        let _ = std::fs::remove_dir_all(&root);
    }

    // §3.2 + §3.3 + §3.4: `--confirm` pays ONCE, the fee comes out of the gross, the settlement is
    // journaled with gross / melt fee / net / destination literal / payment hash / quote id, the
    // receipts are discharged, the attempt is journaled PAID — and a second `--confirm` pays nothing.
    #[test]
    fn confirm_pays_once_takes_the_fee_out_of_the_gross_and_is_idempotent() {
        let (store, root) = store_with_fees("confirm", &[10, 5]);
        let mut fake = Fake::new(|_| 2);
        fake.melt_results = vec![Ok((13, 1))];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 100);
        assert_eq!(
            outcome,
            RemitOutcome::Paid {
                remittance_id: "hash-13-2".to_owned(),
                net_sats: 13,
                melt_fee_sats: 1,
            },
            "{out}"
        );
        assert_eq!(
            fake.melts,
            vec!["lnbc-fake-13-2".to_owned()],
            "exactly one melt, of the NET invoice"
        );
        for needle in [
            "Journaled remittance hash-13-2 covering 2 receipts (owner fake-owner, lease until unix 400); paying...",
            "PAID — remittance hash-13-2 settled",
            "gross discharged: 15 sats",
            "melt fee taken by the mint: 1 sats",
            "net paid to maxplayer@agi.cash: 13 sats",
            "stays in your wallet (unused reserve): 1 sats",
            "wallet balance now: 1000 sats at https://mint.example",
            "receipts discharged: 2",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }
        assert!(!out.contains("WARNING"), "{out}");
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.state, RemittanceState::Settled);
        assert_eq!(row.remittance_id, "hash-13-2");
        assert_eq!(row.payment_hash, "hash-13-2");
        assert_eq!(row.bolt11, "lnbc-fake-13-2");
        assert_eq!(
            (row.gross_sats, row.melt_fee_sats, row.net_sats),
            (15, Some(1), 13)
        );
        assert_eq!(
            row.destination, "maxplayer@agi.cash",
            "the literal paid is journaled"
        );
        assert_eq!(
            row.melt_quote_id,
            Some("paid-quote-lnbc-fake-13-2".to_owned())
        );
        assert_eq!((row.created_at_unix, row.settled_at_unix), (100, Some(100)));
        assert_eq!(row.receipts, 2);
        let accrued = store.accrued_fees().expect("read");
        assert_eq!(accrued.unremitted_fee_sats, 0);
        assert_eq!(accrued.remitted_fee_sats, 15);
        assert!(
            accrued
                .by_job
                .iter()
                .all(|r| r.remittance_id.as_deref() == Some("hash-13-2"))
        );
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            (
                attempts[0].trigger,
                attempts[0].outcome,
                attempts[0].unremitted_sats,
                attempts[0].remittance_id.as_deref(),
                attempts[0].started_at_unix,
            ),
            (
                RemitAttemptTrigger::Command,
                RemitAttemptOutcome::Paid,
                15,
                Some("hash-13-2"),
                100
            )
        );
        assert_eq!(
            attempts[0].detail,
            "paid 13 sats to maxplayer@agi.cash (melt fee 1 sats)"
        );

        // Idempotent: a second --confirm finds nothing unremitted, touches no network, pays nothing,
        // and — a threshold refusal — journals no attempt.
        let pay_requests_before = fake.pay_requests;
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 101);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::NothingUnremitted),
            "{out}"
        );
        assert!(
            out.contains("Nothing to remit. REFUSED — nothing moved."),
            "{out}"
        );
        assert!(
            out.contains("unix 100: operator (--confirm) attempt saw 15 sats unremitted — PAID: paid 13 sats to maxplayer@agi.cash (melt fee 1 sats) [remittance hash-13-2]"),
            "the command prints the journaled attempts:\n{out}"
        );
        assert_eq!(
            fake.pay_requests, pay_requests_before,
            "no LNURL round trip"
        );
        assert_eq!(fake.melts.len(), 1, "still exactly one melt, ever");
        assert_eq!(store.remittances().expect("rows").len(), 1);
        assert_eq!(store.recent_remit_attempts(10).expect("attempts").len(), 1);

        // A new receipt after the settlement is the only thing the next remittance covers.
        store
            .collect_receipt(
                "receipt-late",
                "job-late",
                200,
                ReceiptFees {
                    mint_fee_sats: 2,
                    fee_bps: 1000,
                    fee_sats: 20,
                },
                102,
            )
            .expect("collect late");
        fake.melt_results = vec![Ok((18, 2))];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 103);
        assert!(is_paid(&outcome), "{out}");
        assert!(out.contains("gross discharged: 20 sats"), "{out}");
        assert_eq!(fake.melts.len(), 2);
        assert_eq!(store.accrued_fees().expect("read").remitted_fee_sats, 35);
        let _ = std::fs::remove_dir_all(&root);
    }

    // The mint's melt fee is zero (some mints charge none): one invoice, one quote, the whole gross
    // goes to the destination.
    #[test]
    fn a_zero_fee_reserve_invoices_the_gross_once() {
        let (store, root) = store_with_fees("zero-reserve", &[10]);
        let mut fake = Fake::new(|_| 0);
        fake.melt_results = vec![Ok((10, 0))];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 100);
        assert!(is_paid(&outcome), "{out}");
        assert_eq!(
            fake.invoices,
            vec![10],
            "no second invoice when the reserve is zero"
        );
        assert_eq!(fake.melts, vec!["lnbc-fake-10-1".to_owned()]);
        assert!(
            out.contains("net paid to maxplayer@agi.cash: 10 sats"),
            "{out}"
        );
        assert!(
            out.contains("stays in your wallet (unused reserve): 0 sats"),
            "{out}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // §3.2 / addendum rule 4: below the resolved minSendable ⇒ refuse with the shortfall printed, no
    // invoice requested, nothing journaled — not a remittance row, not an attempt row. This is the
    // steady state for small sellers, not an error, from every trigger.
    #[test]
    fn below_the_resolved_minimum_refuses_with_the_shortfall_and_journals_nothing() {
        // Two 10-sat jobs at 10% owe 1 sat each ⇒ gross 2; the destination wants 5000 msat = 5 sats.
        let (store, root) = store_with_fees("below-min", &[1, 1]);
        let mut fake = Fake::new(|_| 0);
        fake.min_msat = 5000;
        for trigger in [
            RemitTrigger::DryRun,
            RemitTrigger::Command,
            RemitTrigger::Collect,
        ] {
            let (outcome, out) = run_remit(&store, &mut fake, trigger, 100);
            assert_eq!(
                outcome,
                RemitOutcome::Refused(Refusal::BelowMinimum {
                    unremitted: 2,
                    min_sats: 5,
                }),
                "{out}"
            );
            assert!(
                out.contains("REFUSED — unremitted 2 sats is below the destination's minimum of 5 sats (3 sats short)."),
                "{out}"
            );
            assert!(
                out.contains("accumulates until it clears the minimum"),
                "{out}"
            );
        }
        assert!(
            fake.invoices.is_empty(),
            "no invoice is requested below the minimum"
        );
        assert!(fake.melts.is_empty());
        assert!(store.remittances().expect("rows").is_empty());
        assert!(
            store
                .recent_remit_attempts(10)
                .expect("attempts")
                .is_empty(),
            "the threshold is the steady state, not an attempt"
        );
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 2);

        // A minimum that is not a whole sat rounds UP: 1500 msat ⇒ 2 sats; gross 2 clears it, gross 1 does not.
        fake.min_msat = 1500;
        fake.melt_results = vec![Ok((2, 0))];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 101);
        assert!(is_paid(&outcome), "{out}");
        let (store1, root1) = store_with_fees("below-min-1", &[1]);
        let (outcome, out) = run_remit(&store1, &mut fake, RemitTrigger::Command, 102);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::BelowMinimum {
                unremitted: 1,
                min_sats: 2,
            }),
            "{out}"
        );
        assert!(
            out.contains("below the destination's minimum of 2 sats (1 sats short)"),
            "{out}"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root1);
    }

    // §3.3: the fee reserve is taken OUT of the gross — and when what is left is below the minimum,
    // or nothing at all, the attempt refuses rather than paying more than the seller accrued. These
    // refusals ARE journaled: an operator should see a mint whose fees eat the fee.
    #[test]
    fn a_fee_reserve_that_leaves_too_little_or_nothing_is_refused_and_journaled() {
        // Gross 3, reserve 2 ⇒ net 1, below a 2-sat minimum.
        let (store, root) = store_with_fees("reserve-below-min", &[3]);
        let mut fake = Fake::new(|_| 2);
        fake.min_msat = 2000;
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 100);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::ReserveDoesNotFit {
                gross: 3,
                reserve: 2,
            }),
            "{out}"
        );
        assert!(
            out.contains("REFUSED — after the mint's melt fee reserve (2 sats) the 3 sats unremitted leaves 1 sats, below the destination's minimum of 2 sats (1 sats short)."),
            "{out}"
        );
        assert_eq!(fake.invoices, vec![3], "only the probe was requested");
        assert!(fake.melts.is_empty());
        assert!(store.remittances().expect("rows").is_empty());
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].outcome, RemitAttemptOutcome::Refused);
        assert_eq!(attempts[0].unremitted_sats, 3);
        assert_eq!(
            attempts[0].detail,
            "the mint's melt fee reserve (2 sats) does not fit inside the 3 sats accrued"
        );
        assert_eq!(attempts[0].remittance_id, None);

        // Gross 2, reserve 2 ⇒ nothing left.
        let (store2, root2) = store_with_fees("reserve-eats-all", &[2]);
        let mut fake = Fake::new(|_| 2);
        let (outcome, out) = run_remit(&store2, &mut fake, RemitTrigger::Command, 100);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::ReserveDoesNotFit {
                gross: 2,
                reserve: 2,
            }),
            "{out}"
        );
        assert!(
            out.contains("needs a melt fee reserve of 2 sats to pay 2 sats, which leaves nothing for the destination"),
            "{out}"
        );
        assert!(fake.melts.is_empty());

        // A reserve that GROWS on the smaller invoice (non-monotone mint) so net + reserve > gross
        // is refused: the seller would pay more than it accrued.
        let (store3, root3) = store_with_fees("reserve-non-monotone", &[15]);
        let mut fake = Fake::new(|amount| if amount == 15 { 2 } else { 3 });
        let (outcome, out) = run_remit(&store3, &mut fake, RemitTrigger::Command, 100);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::ReserveDoesNotFit {
                gross: 15,
                reserve: 3,
            }),
            "{out}"
        );
        assert!(
            out.contains("quotes a 3 sats fee reserve on 13 sats, so up to 16 sats would leave the wallet against 15 sats accrued"),
            "{out}"
        );
        assert_eq!(fake.invoices, vec![15, 13]);
        assert!(fake.melts.is_empty());
        assert!(store3.remittances().expect("rows").is_empty());
        for root in [root, root2, root3] {
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    // Above the resolved maxSendable: refuse (whole balance or nothing), name the bound.
    #[test]
    fn above_the_resolved_maximum_is_refused() {
        let (store, root) = store_with_fees("above-max", &[50]);
        let mut fake = Fake::new(|_| 0);
        fake.max_msat = 20_000;
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 100);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::AboveMaximum {
                unremitted: 50,
                max_sats: 20,
            }),
            "{out}"
        );
        assert!(
            out.contains("exceeds the destination's maximum of 20 sats"),
            "{out}"
        );
        assert!(fake.invoices.is_empty() && fake.melts.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // §3.4: an interrupted remittance is RECOVERABLE, not repeatable. The melt errors after the plan
    // was journaled: the row stays planned and the attempt is journaled FAILED naming it. The next
    // run asks the mint — PAID ⇒ settled with no second melt; the receipts stay discharged.
    #[test]
    fn interrupted_after_the_plan_is_reconciled_as_paid_without_a_second_melt() {
        let (store, root) = store_with_fees("interrupted-paid", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Err("connection reset during confirm".to_owned())];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Collect, 100);
        assert_eq!(
            outcome,
            RemitOutcome::MeltFailed {
                remittance_id: "hash-9-2".to_owned(),
                error: "connection reset during confirm".to_owned(),
            },
            "{out}"
        );
        assert!(
            out.contains("melt failed: connection reset during confirm"),
            "{out}"
        );
        assert!(
            out.contains("remittance hash-9-2 stays journaled as planned"),
            "{out}"
        );
        assert!(
            !out.contains("Recent attempts"),
            "the collect path does not print the journal into the node log:\n{out}"
        );
        let in_flight = store
            .in_flight_remittance()
            .expect("query")
            .expect("a planned row");
        assert_eq!(in_flight.state, RemittanceState::Planned);
        assert_eq!(in_flight.bolt11, "lnbc-fake-9-2");
        assert_eq!(store.accrued_fees().expect("read").in_flight_fee_sats, 10);
        assert_eq!(fake.melts.len(), 1);
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(
            (
                attempts[0].trigger,
                attempts[0].outcome,
                attempts[0].unremitted_sats,
                attempts[0].remittance_id.as_deref()
            ),
            (
                RemitAttemptTrigger::Collect,
                RemitAttemptOutcome::Failed,
                10,
                Some("hash-9-2")
            )
        );
        assert_eq!(
            attempts[0].detail,
            "melt failed: connection reset during confirm"
        );

        // Next run, the mint says PAID: settle, keep the receipts discharged, then find nothing left.
        fake.status = Ok(Some(MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: "paid-quote-lnbc-fake-9-2".to_owned(),
            state: MeltQuoteState::Paid,
            amount_sats: 9,
            fee_reserve_sats: 1,
        }));
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 101);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::NothingUnremitted),
            "{out}"
        );
        assert!(
            out.contains("unix 100: automatic (after collect) attempt saw 10 sats unremitted — FAILED: melt failed: connection reset during confirm [remittance hash-9-2]"),
            "{out}"
        );
        assert!(
            out.contains("Reconciling in-flight remittance hash-9-2 (planned at unix 100 by fake-owner, lease until unix 400: 9 sats to maxplayer@agi.cash, gross 10 sats)"),
            "{out}"
        );
        assert!(
            out.contains("reports melt quote paid-quote-lnbc-fake-9-2 PAID — recorded as settled by reconciliation: 9 sats reached maxplayer@agi.cash; melt fee at most 1 sats (the quote's reserve"),
            "{out}"
        );
        assert!(out.contains("Nothing to remit."), "{out}");
        assert_eq!(fake.status_calls, vec!["lnbc-fake-9-2".to_owned()]);
        assert_eq!(fake.melts.len(), 1, "reconciliation never melts");
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, RemittanceState::Settled);
        assert_eq!(rows[0].melt_fee_sats, None, "unobserved, not invented");
        assert_eq!(
            rows[0].settled_by,
            Some(RemittanceState::Settled)
                .map(|_| crate::seller_node::store::SettledBy::Reconciliation),
            "the row says it was settled by reconciliation"
        );
        assert_eq!(
            rows[0].melt_fee_reserve_sats,
            Some(1),
            "the paying quote's reserve — the fee's ceiling — is recorded"
        );
        assert_eq!(rows[0].net_sats, 9);
        assert_eq!(
            rows[0].melt_quote_id,
            Some("paid-quote-lnbc-fake-9-2".to_owned())
        );
        assert_eq!(rows[0].settled_at_unix, Some(101));
        let accrued = store.accrued_fees().expect("read");
        assert_eq!(
            (
                accrued.remitted_fee_sats,
                accrued.unremitted_fee_sats,
                accrued.in_flight_fee_sats
            ),
            (10, 0, 0)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // The other reconciliation outcomes: UNPAID/FAILED or no quote at all ⇒ the row fails, the
    // receipts are released and the SAME run proceeds to a fresh plan (so a dry run prints it and a
    // confirm pays it once, on a NEW invoice); PENDING ⇒ refuse this run, keep the row, melt nothing.
    #[test]
    fn an_unpaid_or_pending_interrupted_attempt_is_reconciled_without_paying_twice() {
        let (store, root) = store_with_fees("interrupted-unpaid", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Err("insufficient funds for melt".to_owned())];
        let (outcome, _) = run_remit(&store, &mut fake, RemitTrigger::Command, 100);
        assert!(matches!(outcome, RemitOutcome::MeltFailed { .. }));

        // PENDING: refuse, keep the planned row, no melt, no new invoice; journaled as a refusal
        // naming the row.
        fake.status = Ok(Some(MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: "q-pending".to_owned(),
            state: MeltQuoteState::Pending,
            amount_sats: 9,
            fee_reserve_sats: 1,
        }));
        let invoices_before = fake.invoices.len();
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Collect, 101);
        assert_eq!(
            outcome,
            RemitOutcome::Refused(Refusal::Settling {
                remittance_id: "hash-9-2".to_owned(),
            }),
            "{out}"
        );
        assert!(
            out.contains(
                "reports melt quote q-pending PENDING: the payment is still settling. REFUSED"
            ),
            "{out}"
        );
        assert!(
            store.in_flight_remittance().expect("query").is_some(),
            "the row stays planned"
        );
        assert_eq!(fake.melts.len(), 1);
        assert_eq!(
            fake.invoices.len(),
            invoices_before,
            "no new invoice while pending"
        );
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 2, "the failed melt and the pending refusal");
        assert_eq!(attempts[0].outcome, RemitAttemptOutcome::Refused);
        assert_eq!(attempts[0].remittance_id.as_deref(), Some("hash-9-2"));
        assert_eq!(
            attempts[0].detail,
            "remittance hash-9-2 is still settling at the mint"
        );

        // UNPAID: fail, release, and continue into a fresh DRY RUN on a new invoice.
        fake.status = Ok(Some(MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: "q-unpaid".to_owned(),
            state: MeltQuoteState::Unpaid,
            amount_sats: 9,
            fee_reserve_sats: 1,
        }));
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::DryRun, 102);
        assert_eq!(outcome, RemitOutcome::DryRun, "{out}");
        assert!(out.contains("reports melt quote q-unpaid UNPAID — no sats left the wallet; the row is this process's own earlier attempt, which is over; released 10 sats back to unremitted"), "{out}");
        assert!(out.contains("10 sats unremitted"), "{out}");
        assert!(out.contains("DRY RUN — nothing moved."), "{out}");
        assert_eq!(fake.melts.len(), 1);
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, RemittanceState::Failed);
        assert_eq!(rows[0].receipts, 0);
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 10);

        // Now a confirm pays ONCE on a fresh invoice; the failed row's invoice is never reused.
        fake.status = Ok(None);
        fake.melt_results = vec![Ok((9, 1))];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Command, 103);
        assert!(is_paid(&outcome), "{out}");
        assert_eq!(fake.melts.len(), 2);
        assert_ne!(
            fake.melts[0], fake.melts[1],
            "a fresh invoice, not the failed one"
        );
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].state, RemittanceState::Settled);
        assert_eq!(store.accrued_fees().expect("read").remitted_fee_sats, 10);

        // No quote ever raised (the melt died before quoting): fail and release on the next run.
        let (store2, root2) = store_with_fees("interrupted-noquote", &[10]);
        let mut fake2 = Fake::new(|_| 1);
        fake2.melt_results = vec![Err("mint unreachable".to_owned())];
        assert!(matches!(
            run_remit(&store2, &mut fake2, RemitTrigger::Command, 100).0,
            RemitOutcome::MeltFailed { .. }
        ));
        fake2.status = Ok(None);
        let (outcome, out) = run_remit(&store2, &mut fake2, RemitTrigger::DryRun, 101);
        assert_eq!(outcome, RemitOutcome::DryRun, "{out}");
        assert!(out.contains("the wallet never raised a melt quote for its invoice — no sats left the wallet; the row is this process's own earlier attempt, which is over; released 10 sats back to unremitted"), "{out}");
        assert_eq!(
            store2.remittances().expect("rows")[0].state,
            RemittanceState::Failed
        );
        assert_eq!(fake2.melts.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    // A status query that itself fails is an error that changes nothing: the row stays planned, no
    // melt is attempted — and the attempt is journaled FAILED against the in-flight row.
    #[test]
    fn a_reconciliation_that_cannot_reach_the_mint_leaves_the_row_planned() {
        let (store, root) = store_with_fees("reconcile-error", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Err("boom".to_owned())];
        assert!(matches!(
            run_remit(&store, &mut fake, RemitTrigger::Command, 100).0,
            RemitOutcome::MeltFailed { .. }
        ));
        fake.status = Err("mint unreachable".to_owned());
        let mut out = Vec::new();
        let error = remit(&store, &mut fake, RemitTrigger::Command, 101, &mut out)
            .expect_err("status failure surfaces");
        assert_eq!(error, "mint unreachable");
        assert!(store.in_flight_remittance().expect("query").is_some());
        assert_eq!(fake.melts.len(), 1);
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].outcome, RemitAttemptOutcome::Failed);
        assert_eq!(attempts[0].detail, "mint unreachable");
        assert_eq!(attempts[0].remittance_id.as_deref(), Some("hash-9-2"));
        assert_eq!(
            attempts[0].unremitted_sats, 0,
            "the balance is in flight, not unremitted"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // Addendum gate 2b, the effects half: the LNURL host is unreachable. `remit_best_effort` returns
    // a report — never an error the collect path has to handle — the balance is intact, and the
    // failure is journaled so the read-out shows it. (The collect half — receipt journaled, job
    // marked paid — is `seller_node::run`'s test on the real collect write.)
    #[test]
    fn best_effort_catches_a_failed_attempt_and_leaves_the_balance_intact() {
        let (store, root) = store_with_fees("best-effort-fails", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.pay_request_error = Some("agi.cash: connection refused".to_owned());
        let report = remit_best_effort(&store, &mut fake, RemitTrigger::Collect, 100);
        assert_eq!(
            report.outcome,
            Err("agi.cash: connection refused".to_owned())
        );
        assert!(!report.is_quiet());
        assert_eq!(
            report.summary(),
            "attempt FAILED (agi.cash: connection refused); the balance stays unremitted and the node retries with backoff while it runs"
        );
        assert!(
            report.is_failure(),
            "an unreachable host is a pacing failure"
        );
        assert!(
            report.lines.iter().any(|line| line
                == "Accrued platform fee: 10 sats all-time — 0 sats remitted, 10 sats unremitted"),
            "{:?}",
            report.lines
        );
        assert!(fake.invoices.is_empty() && fake.melts.is_empty());
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 10);
        assert!(store.remittances().expect("rows").is_empty());
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].trigger, RemitAttemptTrigger::Collect);
        assert_eq!(attempts[0].outcome, RemitAttemptOutcome::Failed);
        assert_eq!(attempts[0].unremitted_sats, 10);
        assert_eq!(attempts[0].detail, "agi.cash: connection refused");

        // The next attempt (a collect here; the retry tick is the other trigger) succeeds: the whole
        // balance, old and new, is paid once.
        store
            .collect_receipt(
                "receipt-next",
                "job-next",
                50,
                ReceiptFees {
                    mint_fee_sats: 1,
                    fee_bps: 1000,
                    fee_sats: 5,
                },
                101,
            )
            .expect("collect");
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Ok((14, 1))];
        let report = remit_best_effort(&store, &mut fake, RemitTrigger::Collect, 102);
        assert_eq!(
            report.outcome,
            Ok(RemitOutcome::Paid {
                remittance_id: "hash-14-2".to_owned(),
                net_sats: 14,
                melt_fee_sats: 1,
            })
        );
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 0);
        assert_eq!(store.accrued_fees().expect("read").remitted_fee_sats, 15);

        // And a collect that lands below the threshold is quiet: no attempt journaled.
        let attempts_before = store.recent_remit_attempts(10).expect("attempts").len();
        store
            .collect_receipt(
                "receipt-tiny",
                "job-tiny",
                5,
                ReceiptFees {
                    mint_fee_sats: 1,
                    fee_bps: 1000,
                    fee_sats: 0,
                },
                103,
            )
            .expect("collect");
        let mut fake = Fake::new(|_| 1);
        let report = remit_best_effort(&store, &mut fake, RemitTrigger::Collect, 104);
        assert_eq!(
            report.outcome,
            Ok(RemitOutcome::Refused(Refusal::NothingUnremitted))
        );
        assert!(report.is_quiet());
        assert_eq!(fake.pay_requests, 0, "no network below the threshold");
        assert_eq!(
            store.recent_remit_attempts(10).expect("attempts").len(),
            attempts_before
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // Addendum gate 2c: two attempts race against the same balance — two threads, two connections
    // to the same store, released together — and EXACTLY ONE remittance is recorded and exactly one
    // melt happens. The loser is refused by the store's plan (a row already in flight, or nothing
    // left once the winner settled), never paid a second time.
    #[test]
    fn two_racing_attempts_against_the_same_balance_record_exactly_one_remittance() {
        for round in 0..8 {
            let (store, root) = store_with_fees(&format!("race-{round}"), &[10, 5]);
            let db = root.join(STATE_DB_FILE);
            let melts = Arc::new(AtomicUsize::new(0));
            let barrier = Arc::new(Barrier::new(2));
            let mut handles = Vec::new();
            for thread in 0..2 {
                let db = db.clone();
                let melts = Arc::clone(&melts);
                let barrier = Arc::clone(&barrier);
                handles.push(std::thread::spawn(move || {
                    // A connection of its own, as two collect handlers on two threads would have.
                    let store = SellerStore::open(&db).expect("open");
                    let mut fake = Fake::new(|_| 2);
                    // Two real LNURL calls never hand out the same invoice: distinct hashes, so
                    // the loser cannot be masked by a DuplicateInvoice on the winner's hash.
                    fake.invoice_tag = format!("-t{thread}");
                    fake.melt_results = vec![Ok((13, 1))];
                    fake.melt_counter = Some(melts);
                    barrier.wait();
                    let mut out = Vec::new();
                    let outcome = remit(
                        &store,
                        &mut fake,
                        RemitTrigger::Collect,
                        100 + thread,
                        &mut out,
                    );
                    (outcome, String::from_utf8_lossy(&out).into_owned())
                }));
            }
            let results: Vec<_> = handles
                .into_iter()
                .map(|handle| handle.join().expect("thread"))
                .collect();

            assert_eq!(
                melts.load(Ordering::SeqCst),
                1,
                "exactly one melt: {results:?}"
            );
            let paid = results
                .iter()
                .filter(|(outcome, _)| matches!(outcome, Ok(RemitOutcome::Paid { .. })))
                .count();
            assert_eq!(paid, 1, "exactly one attempt paid: {results:?}");
            for (outcome, out) in &results {
                match outcome {
                    Ok(RemitOutcome::Paid { .. }) => {}
                    Ok(RemitOutcome::Refused(Refusal::PlanRefused(reason))) => assert!(
                        reason.contains("still in flight")
                            || reason.contains("nothing to remit")
                            || reason.contains("unremitted total moved"),
                        "the loser is refused by the store's plan: {reason}\n{out}"
                    ),
                    Ok(RemitOutcome::Refused(Refusal::NothingUnremitted)) => {}
                    other => panic!("unexpected outcome {other:?}\n{out}"),
                }
            }
            let rows = store.remittances().expect("rows");
            assert_eq!(rows.len(), 1, "exactly one remittance row: {rows:?}");
            assert_eq!(rows[0].state, RemittanceState::Settled);
            assert_eq!((rows[0].gross_sats, rows[0].net_sats), (15, 13));
            let accrued = store.accrued_fees().expect("read");
            assert_eq!(
                (
                    accrued.remitted_fee_sats,
                    accrued.unremitted_fee_sats,
                    accrued.in_flight_fee_sats
                ),
                (15, 0, 0)
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    // ---- addendum 3 §1: the money hold, at the moment of spending (gate 2f) --------------------

    // Gate 2f: gross 15, the ESTIMATE quotes a 2-sat reserve (invoice 13, ceiling 15 holds), but the
    // quote the mint raises FOR THE PAYMENT carries a 4-sat reserve: 13 + 4 = 17 > 15. The melt is
    // REFUSED before any proof is spent — zero debits — the attempt is journaled FAILED naming the
    // row, the row is released, and the accrued balance is exactly what it was. Then the same seller
    // with a payment-time reserve that FITS pays exactly once.
    #[test]
    fn a_reserve_that_grows_between_estimate_and_payment_is_refused_before_spending() {
        let (store, root) = store_with_fees("ceiling-refused", &[10, 5]);
        let mut fake = Fake::new(|_| 2);
        fake.live_reserve_for = Some(Box::new(|_| 4));
        fake.melt_results = vec![Ok((13, 1))];
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Collect, 100);
        match &outcome {
            RemitOutcome::MeltRefused {
                remittance_id,
                reason,
            } => {
                assert_eq!(remittance_id, "hash-13-2");
                assert!(
                    reason.contains("would debit 17 sats (13 sats invoice + 4 sats fee reserve; planned invoice 13 sats) against a ceiling of 15 sats; nothing left the wallet"),
                    "{reason}"
                );
            }
            other => panic!("expected MeltRefused, got {other:?}\n{out}"),
        }
        assert!(
            fake.melts.is_empty(),
            "ZERO debits: the refusal happens before the proofs are touched"
        );
        assert_eq!(fake.ceiling_refusals.len(), 1);
        assert_eq!(
            fake.melt_results.len(),
            1,
            "the scripted payment was never consumed"
        );
        assert!(
            out.contains("REFUSED before spending — melt refused before spending"),
            "{out}"
        );
        assert!(
            out.contains("A seller never pays more than it accrued: the ceiling is 15 sats, enforced against the quote the mint raised for the payment. Nothing left the wallet; released 15 sats back to unremitted."),
            "{out}"
        );
        // Journaled as a FAILED attempt naming the row; the row is failed and its receipts released.
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].outcome, RemitAttemptOutcome::Failed);
        assert_eq!(attempts[0].remittance_id.as_deref(), Some("hash-13-2"));
        assert!(
            attempts[0]
                .detail
                .starts_with("refused before spending: melt refused before spending"),
            "{}",
            attempts[0].detail
        );
        assert_eq!(attempts[0].unremitted_sats, 15);
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, RemittanceState::Failed);
        assert_eq!(rows[0].receipts, 0, "released");
        let accrued = store.accrued_fees().expect("read");
        assert_eq!(
            (
                accrued.remitted_fee_sats,
                accrued.in_flight_fee_sats,
                accrued.unremitted_fee_sats
            ),
            (0, 0, 15),
            "the accrued balance is unchanged: nothing paid, nothing in flight"
        );
        // It is a failure for the pacing: the backoff escalates.
        let report = RemitReport {
            outcome: Ok(outcome),
            lines: Vec::new(),
        };
        assert!(report.is_failure());
        assert!(
            report
                .summary()
                .starts_with("melt REFUSED before spending (")
        );

        // The payment-time reserve fits (2: 13 + 2 = 15 ≤ 15): pays exactly once, and the
        // settlement records the PAYING quote's reserve beside the actual fee.
        fake.live_reserve_for = Some(Box::new(|_| 2));
        let (outcome, out) = run_remit(&store, &mut fake, RemitTrigger::Retry, 101);
        assert!(is_paid(&outcome), "{out}");
        assert_eq!(fake.melts.len(), 1, "exactly one debit, ever");
        assert!(
            out.contains("melt fee taken by the mint: 1 sats (quote paid-quote-lnbc-fake-13-4 reserved 2 sats; ceiling 15 sats held at the moment of spending)"),
            "{out}"
        );
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].state, RemittanceState::Settled);
        assert_eq!(rows[1].melt_fee_sats, Some(1));
        assert_eq!(rows[1].melt_fee_reserve_sats, Some(2));
        assert_eq!(
            rows[1].settled_by,
            Some(crate::seller_node::store::SettledBy::Melt)
        );
        assert_eq!(
            rows[1].melt_quote_id,
            Some("paid-quote-lnbc-fake-13-4".to_owned()),
            "the quote that actually paid is the one recorded"
        );
        assert_eq!(store.accrued_fees().expect("read").remitted_fee_sats, 15);

        // Exactly at the ceiling is admitted; one sat over is not (the pure rule the melt applies).
        let ceiling = MeltCeiling {
            max_debit_sats: 15,
            invoice_sats: 13,
            planned_quote_id: None,
        };
        assert!(ceiling.admits(13, 2));
        assert!(!ceiling.admits(13, 3));
        assert!(
            !ceiling.admits(12, 0),
            "a quote for a different amount than planned is refused too"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- addendum 3 §2: ownership — never release a live payer's intent (gate 2g) ---------------

    fn status(state: MeltQuoteState, quote_id: &str) -> MeltQuoteStatus {
        MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: quote_id.to_owned(),
            state,
            amount_sats: 13,
            fee_reserve_sats: 2,
        }
    }

    fn planned_row(owner: Option<&str>, lease_until: Option<i64>) -> FeeRemittance {
        FeeRemittance {
            remittance_id: "x".to_owned(),
            gross_sats: 15,
            melt_fee_sats: None,
            melt_fee_reserve_sats: Some(2),
            net_sats: 13,
            destination: PLATFORM_FEE_ADDRESS.to_owned(),
            melt_quote_id: Some("q".to_owned()),
            payment_hash: "x".to_owned(),
            bolt11: "ln-x".to_owned(),
            state: RemittanceState::Planned,
            created_at_unix: 100,
            settled_at_unix: None,
            settled_by: None,
            owner: owner.map(str::to_owned),
            lease_until_unix: lease_until,
            receipts: 2,
        }
    }

    // The release rule as a table: PAID settles; PENDING/UNKNOWN hold; FAILED releases whoever owns
    // the row; UNPAID / no quote release only for the row's own process or after the lease — and
    // HOLD while another process's lease stands. A pre-v11 row (no owner, no lease) reads as expired.
    #[test]
    fn reconciliation_releases_only_terminal_quotes_own_rows_or_expired_leases() {
        let theirs = planned_row(Some("proc-b"), Some(400));
        let mine = planned_row(Some("proc-a"), Some(400));
        let legacy = planned_row(None, None);
        let paid = status(MeltQuoteState::Paid, "q-paid");
        let pending = status(MeltQuoteState::Pending, "q-pending");
        let unknown = status(MeltQuoteState::Unknown, "q-unknown");
        let failed = status(MeltQuoteState::Failed, "q-failed");
        let unpaid = status(MeltQuoteState::Unpaid, "q-unpaid");

        assert_eq!(
            reconcile_decision(&theirs, Some(&paid), "proc-a", 200),
            Reconcile::Settle
        );
        assert_eq!(
            reconcile_decision(&theirs, Some(&pending), "proc-a", 200),
            Reconcile::Hold(Refusal::Settling {
                remittance_id: "x".to_owned()
            })
        );
        assert_eq!(
            reconcile_decision(&mine, Some(&unknown), "proc-a", 200),
            Reconcile::Hold(Refusal::Settling {
                remittance_id: "x".to_owned()
            }),
            "unknown is not terminal, even on our own row"
        );
        assert!(matches!(
            reconcile_decision(&theirs, Some(&failed), "proc-a", 200),
            Reconcile::Release(reason) if reason.contains("FAILED is terminal")
        ));
        // UNPAID / no quote, another process's live lease: HOLD — "not yet" is not "abandoned".
        assert_eq!(
            reconcile_decision(&theirs, Some(&unpaid), "proc-a", 200),
            Reconcile::Hold(Refusal::HeldByOwner {
                remittance_id: "x".to_owned(),
                owner: "proc-b".to_owned(),
                lease_until_unix: 400,
            })
        );
        assert_eq!(
            reconcile_decision(&theirs, None, "proc-a", 399),
            Reconcile::Hold(Refusal::HeldByOwner {
                remittance_id: "x".to_owned(),
                owner: "proc-b".to_owned(),
                lease_until_unix: 400,
            }),
            "one second before the lease ends it still holds"
        );
        // …and RELEASE once the lease has run out (the owner is provably gone or not spending).
        assert!(matches!(
            reconcile_decision(&theirs, Some(&unpaid), "proc-a", 400),
            Reconcile::Release(reason) if reason.contains("lease ran out at unix 400")
        ));
        assert!(matches!(
            reconcile_decision(&theirs, None, "proc-a", 401),
            Reconcile::Release(reason) if reason.contains("never raised a melt quote")
        ));
        // Our own row: release on UNPAID / none at any time — our earlier attempt is over.
        assert!(matches!(
            reconcile_decision(&mine, Some(&unpaid), "proc-a", 101),
            Reconcile::Release(reason) if reason.contains("this process's own earlier attempt")
        ));
        assert!(matches!(
            reconcile_decision(&mine, None, "proc-a", 101),
            Reconcile::Release(_)
        ));
        // A pre-v11 row: nobody's, lease expired ⇒ releasable on UNPAID / none, settled on PAID.
        assert!(matches!(
            reconcile_decision(&legacy, Some(&unpaid), "proc-a", 101),
            Reconcile::Release(reason) if reason.contains("owner none recorded")
        ));
        assert_eq!(
            reconcile_decision(&legacy, Some(&paid), "proc-a", 101),
            Reconcile::Settle
        );
        assert!(
            !Refusal::HeldByOwner {
                remittance_id: "x".to_owned(),
                owner: "proc-b".to_owned(),
                lease_until_unix: 400,
            }
            .is_threshold()
        );
    }

    /// The paused side's (outcome, output) and every `meanwhile` run's (outcome, output).
    type PausedRun = (
        (Result<RemitOutcome, String>, String),
        Vec<(RemitOutcome, String)>,
    );

    /// Two processes against one store: `first` is paused at `gate` (after its plan is journaled),
    /// `second` runs whatever the test scripts meanwhile. Returns each side's outcome and output.
    fn run_paused(
        db: &PathBuf,
        mut first: Fake,
        first_now: i64,
        gate: Arc<super::test_support::Gate>,
        melts: Arc<AtomicUsize>,
        meanwhile: impl FnOnce(&SellerStore) -> Vec<(RemitOutcome, String)>,
    ) -> PausedRun {
        first.plan_gate = Some(Arc::clone(&gate));
        first.melt_counter = Some(Arc::clone(&melts));
        let db_a = db.clone();
        let a = std::thread::spawn(move || {
            let store = SellerStore::open(&db_a).expect("open A");
            let mut out = Vec::new();
            let outcome = remit(
                &store,
                &mut first,
                RemitTrigger::Collect,
                first_now,
                &mut out,
            );
            (outcome, String::from_utf8_lossy(&out).into_owned())
        });
        gate.wait_arrived(Duration::from_secs(10));
        let store_b = SellerStore::open(db).expect("open B");
        let b = meanwhile(&store_b);
        gate.release();
        let a = a.join().expect("A's thread");
        (a, b)
    }

    // Gate 2g, the live owner: process A journals its plan (invoice X) and PAUSES before spending.
    // Process B — another owner, another connection, distinct invoices — runs reconciliation and
    // then `--confirm`: it finds X planned by a LIVE owner with the mint saying UNPAID, and HOLDS.
    // It plans nothing, pays nothing. A resumes, passes the pre-spend gate, pays X once. Exactly one
    // debit (melts counted, not settlement reports); one settled row; B's refusals journaled.
    #[test]
    fn a_second_process_cannot_release_a_live_owners_planned_row_and_exactly_one_debit_happens() {
        let (store, root) = store_with_fees("live-owner", &[10, 5]);
        drop(store);
        let db = root.join(STATE_DB_FILE);
        let melts = Arc::new(AtomicUsize::new(0));
        let mut a = Fake::new(|_| 2);
        a.owner = "proc-a".to_owned();
        a.invoice_tag = "-a".to_owned();
        a.melt_results = vec![Ok((13, 1))];
        let (a_result, b_results) = run_paused(
            &db,
            a,
            100,
            super::test_support::Gate::new(),
            Arc::clone(&melts),
            |store_b| {
                let mut results = Vec::new();
                for (trigger, now) in [(RemitTrigger::DryRun, 110), (RemitTrigger::Command, 111)] {
                    let mut b = Fake::new(|_| 2);
                    b.owner = "proc-b".to_owned();
                    b.invoice_tag = "-b".to_owned();
                    b.melt_results = vec![Ok((13, 1))];
                    b.melt_counter = Some(Arc::clone(&melts));
                    // The mint's honest answer about A's planned invoice while A is paused: UNPAID.
                    b.status = Ok(Some(status(
                        MeltQuoteState::Unpaid,
                        "quote-lnbc-fake-13-2-a",
                    )));
                    let (outcome, out) = run_remit(store_b, &mut b, trigger, now);
                    assert!(b.melts.is_empty(), "B must not pay: {out}");
                    assert!(
                        b.invoices.is_empty(),
                        "B must not even plan on top of a held row: {out}"
                    );
                    results.push((outcome, out));
                }
                results
            },
        );
        for (outcome, out) in &b_results {
            assert_eq!(
                outcome,
                &RemitOutcome::Refused(Refusal::HeldByOwner {
                    remittance_id: "hash-13-2-a".to_owned(),
                    owner: "proc-a".to_owned(),
                    lease_until_unix: 400,
                }),
                "{out}"
            );
            assert!(
                out.contains("is planned by another live process (proc-a, lease until unix 400) and its quote is not terminal; not releasing a live payer's intent. REFUSED"),
                "{out}"
            );
        }
        let (a_outcome, a_out) = a_result;
        assert!(
            matches!(a_outcome, Ok(RemitOutcome::Paid { .. })),
            "A pays once it resumes: {a_outcome:?}\n{a_out}"
        );
        assert_eq!(melts.load(Ordering::SeqCst), 1, "exactly one actual debit");
        let store = SellerStore::open(&db).expect("open");
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 1, "one row, A's: {rows:?}");
        assert_eq!(rows[0].remittance_id, "hash-13-2-a");
        assert_eq!(rows[0].state, RemittanceState::Settled);
        assert_eq!(rows[0].owner.as_deref(), Some("proc-a"));
        let accrued = store.accrued_fees().expect("read");
        assert_eq!(
            (
                accrued.remitted_fee_sats,
                accrued.in_flight_fee_sats,
                accrued.unremitted_fee_sats
            ),
            (15, 0, 0)
        );
        // B's --confirm refusal is journaled (the dry run is not an attempt).
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        assert_eq!(attempts.len(), 2, "{attempts:?}");
        assert_eq!(attempts[0].outcome, RemitAttemptOutcome::Paid);
        assert_eq!(attempts[1].trigger, RemitAttemptTrigger::Command);
        assert_eq!(attempts[1].outcome, RemitAttemptOutcome::Refused);
        assert_eq!(attempts[1].remittance_id.as_deref(), Some("hash-13-2-a"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // Gate 2g, the gone owner: A journals X and pauses; B runs AFTER A's lease has run out, with the
    // mint saying UNPAID — B may release X (the owner is provably not spending: it refuses to start
    // inside the margin), plans a DISTINCT invoice Y and pays it. A then resumes: its pre-spend gate
    // finds X no longer planned and REFUSES without touching the wallet. Exactly one debit — B's.
    #[test]
    fn an_owner_that_outlives_its_lease_is_released_and_then_refuses_to_spend() {
        let (store, root) = store_with_fees("expired-owner", &[10, 5]);
        drop(store);
        let db = root.join(STATE_DB_FILE);
        let melts = Arc::new(AtomicUsize::new(0));
        let mut a = Fake::new(|_| 2);
        a.owner = "proc-a".to_owned();
        a.invoice_tag = "-a".to_owned();
        a.melt_results = vec![Ok((13, 1))];
        let (a_result, b_results) = run_paused(
            &db,
            a,
            100,
            super::test_support::Gate::new(),
            Arc::clone(&melts),
            |store_b| {
                let mut b = Fake::new(|_| 2);
                b.owner = "proc-b".to_owned();
                b.invoice_tag = "-b".to_owned();
                b.melt_results = vec![Ok((13, 1))];
                b.melt_counter = Some(Arc::clone(&melts));
                b.status = Ok(Some(status(
                    MeltQuoteState::Unpaid,
                    "quote-lnbc-fake-13-2-a",
                )));
                // 100 + REMIT_LEASE (300) = 400: the lease has run out.
                let (outcome, out) = run_remit(store_b, &mut b, RemitTrigger::Command, 400);
                assert!(is_paid(&outcome), "B pays Y once X is released: {out}");
                assert_eq!(b.melts, vec!["lnbc-fake-13-2-b".to_owned()]);
                assert!(
                    out.contains("its owner's lease ran out at unix 400 (owner proc-a); released 15 sats back to unremitted"),
                    "{out}"
                );
                vec![(outcome, out)]
            },
        );
        let (a_outcome, a_out) = a_result;
        match a_outcome {
            Ok(RemitOutcome::Refused(Refusal::OwnershipLost {
                remittance_id,
                reason,
            })) => {
                assert_eq!(remittance_id, "hash-13-2-a");
                assert!(
                    reason.contains(
                        "the row is no longer planned (now failed): another process reconciled it"
                    ),
                    "{reason}"
                );
            }
            other => panic!("A must refuse at the pre-spend gate, got {other:?}\n{a_out}"),
        }
        assert!(
            a_out.contains("REFUSED before spending — the row is no longer planned (now failed)"),
            "{a_out}"
        );
        assert_eq!(
            melts.load(Ordering::SeqCst),
            1,
            "exactly one actual debit — B's"
        );
        assert_eq!(b_results.len(), 1);
        let store = SellerStore::open(&db).expect("open");
        let rows = store.remittances().expect("rows");
        assert_eq!(
            rows.iter()
                .map(|r| (r.remittance_id.as_str(), r.state))
                .collect::<Vec<_>>(),
            vec![
                ("hash-13-2-a", RemittanceState::Failed),
                ("hash-13-2-b", RemittanceState::Settled)
            ]
        );
        let accrued = store.accrued_fees().expect("read");
        assert_eq!(
            (
                accrued.remitted_fee_sats,
                accrued.in_flight_fee_sats,
                accrued.unremitted_fee_sats
            ),
            (15, 0, 0)
        );
        // A's refusal at the gate is journaled as a refused attempt naming X.
        let attempts = store.recent_remit_attempts(10).expect("attempts");
        let a_attempt = attempts
            .iter()
            .find(|a| a.trigger == RemitAttemptTrigger::Collect)
            .expect("A's attempt");
        assert_eq!(a_attempt.outcome, RemitAttemptOutcome::Refused);
        assert_eq!(a_attempt.remittance_id.as_deref(), Some("hash-13-2-a"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // The owner's own margin at the store boundary: with less than SPEND_MARGIN of lease left, the
    // ownership check refuses (the remit path then releases its own row rather than spend).
    #[test]
    fn an_owner_with_too_little_lease_left_is_refused_at_the_gate() {
        let (store, root) = store_with_fees("lease-margin", &[10, 5]);
        // Plan at 100 ⇒ lease until 400. At 341 there are 59 s left: under the 60 s margin.
        let plan = RemittancePlan {
            payment_hash: "manual".to_owned(),
            gross_sats: 15,
            net_sats: 13,
            melt_fee_reserve_sats: 2,
            destination: PLATFORM_FEE_ADDRESS.to_owned(),
            bolt11: "ln-manual".to_owned(),
            melt_quote_id: None,
        };
        store
            .plan_remittance(&plan, "fake-owner", 400, 100)
            .expect("plan");
        assert_eq!(
            store
                .confirm_remittance_ownership("manual", "fake-owner", 341, lease_secs(SPEND_MARGIN))
                .expect("query"),
            Err(OwnershipLost::LeaseTooShort {
                lease_until_unix: Some(400),
                now_unix: 341,
                margin_secs: 60,
            })
        );
        assert!(
            store
                .confirm_remittance_ownership("manual", "fake-owner", 340, lease_secs(SPEND_MARGIN))
                .expect("query")
                .is_ok(),
            "exactly the margin left is enough"
        );
        assert_eq!(lease_secs(REMIT_LEASE), 300);
        assert_eq!(lease_secs(SPEND_MARGIN), 60);
        let _ = std::fs::remove_dir_all(&root);
    }

    // The attempts journal is bounded and newest-first, and a limit of zero returns nothing.
    #[test]
    fn recent_attempts_are_newest_first_and_bounded() {
        let (store, root) = store_with_fees("attempts-order", &[]);
        for at in 1..=7 {
            store
                .record_remit_attempt(&RemitAttempt {
                    attempt_id: 0,
                    started_at_unix: at,
                    trigger: RemitAttemptTrigger::Collect,
                    unremitted_sats: 3,
                    outcome: RemitAttemptOutcome::Failed,
                    detail: format!("failure {at}"),
                    remittance_id: None,
                })
                .expect("record");
        }
        let recent = store
            .recent_remit_attempts(RECENT_ATTEMPTS_SHOWN)
            .expect("read");
        assert_eq!(
            recent.iter().map(|a| a.started_at_unix).collect::<Vec<_>>(),
            vec![7, 6, 5, 4, 3]
        );
        assert!(store.recent_remit_attempts(0).expect("read").is_empty());
        let mut out = Vec::new();
        print_recent_attempts(&store, &mut out).expect("print");
        let text = String::from_utf8(out).expect("utf8");
        assert!(
            text.starts_with("Recent attempts (newest first, last 5):\n  unix 7: automatic (after collect) attempt saw 3 sats unremitted — FAILED: failure 7\n"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- retry pacing (addendum 2, gate 2d) ----------------------------------------------------

    fn failed(error: &str) -> RemitReport {
        RemitReport {
            outcome: Err(error.to_owned()),
            lines: Vec::new(),
        }
    }

    fn paid(net_sats: u64) -> RemitReport {
        RemitReport {
            outcome: Ok(RemitOutcome::Paid {
                remittance_id: "r".to_owned(),
                net_sats,
                melt_fee_sats: 1,
            }),
            lines: Vec::new(),
        }
    }

    fn refused(refusal: Refusal) -> RemitReport {
        RemitReport {
            outcome: Ok(RemitOutcome::Refused(refusal)),
            lines: Vec::new(),
        }
    }

    // Gate 2d: consecutive failures GROW the delay from the base, doubling, and it is CAPPED; the
    // jittered delay never exceeds the computed delay for that streak and never falls below zero; a
    // success RESETS it to base. Driven on the pure computation — nothing here sleeps.
    #[test]
    fn retry_backoff_doubles_from_base_caps_at_thirty_minutes_and_resets_on_success() {
        let mut pacing = RemitBackoff::new();
        assert_eq!(pacing.computed_delay(), Duration::from_secs(30));
        assert_eq!(RETRY_BASE, Duration::from_secs(30));
        assert_eq!(RETRY_CAP, Duration::from_secs(30 * 60));

        assert_eq!(
            pacing.observe(&failed("host down"), 1_000),
            Pacing::FirstFailure
        );
        let mut expected = vec![Duration::from_secs(30)];
        let mut seen_cap_transition = 0;
        for attempt in 2..=16u32 {
            let delay = pacing.computed_delay();
            expected.push(delay);
            // Never above the computed delay, never below zero, for the extremes and the middle.
            for entropy in [0, 1, u64::MAX / 3, u64::MAX / 2, u64::MAX - 1, u64::MAX] {
                let slept = pacing_jitter_probe(&pacing, entropy);
                assert!(
                    slept <= delay,
                    "streak {}: {slept:?} > {delay:?}",
                    pacing.streak()
                );
                assert!(slept >= Duration::ZERO);
            }
            assert_eq!(pacing_jitter_probe(&pacing, u64::MAX), delay);
            assert_eq!(pacing_jitter_probe(&pacing, 0), Duration::ZERO);
            let drawn = pacing.next_delay();
            assert!(
                drawn <= delay,
                "the RNG draw stays under the computed delay"
            );
            match pacing.observe(&failed("host down"), 1_000 + i64::from(attempt) * 60) {
                Pacing::RepeatFailure {
                    streak,
                    entered_cap,
                } => {
                    assert_eq!(streak, attempt);
                    if entered_cap {
                        seen_cap_transition += 1;
                    }
                }
                other => panic!("attempt {attempt}: expected a repeat failure, got {other:?}"),
            }
        }
        // 30, 60, 120, 240, 480, 960, 1800 (cap), 1800, 1800, ...
        assert_eq!(
            expected
                .iter()
                .take(8)
                .map(Duration::as_secs)
                .collect::<Vec<_>>(),
            vec![30, 60, 120, 240, 480, 960, 1800, 1800]
        );
        assert!(
            expected.iter().all(|delay| *delay <= RETRY_CAP),
            "the doubling is capped: {expected:?}"
        );
        assert!(
            expected.iter().skip(6).all(|delay| *delay == RETRY_CAP),
            "once capped it stays capped: {expected:?}"
        );
        assert_eq!(
            seen_cap_transition, 1,
            "the transition into the cap is reported exactly once"
        );
        assert!(pacing.at_cap());

        // A success resets to base and reports the streak it ended.
        assert_eq!(
            pacing.observe(&paid(10), 1_000 + 17 * 60),
            Pacing::Recovered {
                failed_attempts: 16,
                owed_for_secs: 17 * 60,
            }
        );
        assert_eq!(pacing.streak(), 0);
        assert_eq!(pacing.computed_delay(), RETRY_BASE);
        assert!(!pacing.at_cap());
        // A success with no streak behind it is just a payment.
        assert_eq!(pacing.observe(&paid(10), 2_000), Pacing::Paid);
        assert_eq!(pacing.computed_delay(), RETRY_BASE);
    }

    fn pacing_jitter_probe(pacing: &RemitBackoff, entropy: u64) -> Duration {
        jittered(pacing.computed_delay(), entropy)
    }

    // Rules 4 and 5: below the threshold is NOT a failure, and a zero balance is not one either —
    // neither escalates the streak, neither resets it, and neither is logged as a failure.
    #[test]
    fn a_balance_under_the_threshold_or_at_zero_neither_escalates_nor_resets_the_backoff() {
        let mut pacing = RemitBackoff::new();
        let below = refused(Refusal::BelowMinimum {
            unremitted: 0,
            min_sats: 1,
        });
        let zero = refused(Refusal::NothingUnremitted);
        assert!(!below.is_failure() && !zero.is_failure());
        assert!(below.is_quiet() && zero.is_quiet());

        // Healthy node: idle at the base interval.
        assert_eq!(pacing.observe(&below, 1), Pacing::Idle);
        assert_eq!(pacing.observe(&zero, 2), Pacing::Idle);
        assert_eq!(pacing.computed_delay(), RETRY_BASE);
        assert_eq!(pacing.streak(), 0);

        // Mid-streak: the threshold outcomes leave the streak exactly where it was — they are not a
        // success, so they do not reset it (rule 3: success and nothing else), and they are not a
        // failure, so they do not lengthen it.
        pacing.observe(&failed("a"), 10);
        pacing.observe(&failed("b"), 11);
        let before = pacing.clone();
        assert_eq!(pacing.observe(&below, 12), Pacing::Idle);
        assert_eq!(pacing.observe(&zero, 13), Pacing::Idle);
        assert_eq!(pacing, before);
        assert_eq!(pacing.computed_delay(), Duration::from_secs(120));

        // A refusal that is NOT at the threshold left the fee owed for a reason retrying every 30 s
        // cannot fix: it paces like a failure.
        let reserve = refused(Refusal::ReserveDoesNotFit {
            gross: 2,
            reserve: 2,
        });
        assert!(reserve.is_failure());
        assert_eq!(
            pacing.observe(&reserve, 14),
            Pacing::RepeatFailure {
                streak: 3,
                entered_cap: false
            }
        );
        // A melt that failed after the plan is a failure too.
        let melt_failed = RemitReport {
            outcome: Ok(RemitOutcome::MeltFailed {
                remittance_id: "r".to_owned(),
                error: "mint timeout".to_owned(),
            }),
            lines: Vec::new(),
        };
        assert!(melt_failed.is_failure());
        // A dry run is nothing to the pacing.
        let dry = RemitReport {
            outcome: Ok(RemitOutcome::DryRun),
            lines: Vec::new(),
        };
        assert!(!dry.is_failure());
        assert_eq!(pacing.observe(&dry, 15), Pacing::Idle);
    }

    // Explicit bounds (for the loop test that cannot sleep 30 minutes): the cap clamps to the base,
    // and the arithmetic is the same.
    #[test]
    fn retry_backoff_honours_explicit_bounds() {
        let mut pacing =
            RemitBackoff::with_bounds(Duration::from_millis(20), Duration::from_millis(50));
        assert_eq!(pacing.computed_delay(), Duration::from_millis(20));
        pacing.observe(&failed("x"), 0);
        assert_eq!(pacing.computed_delay(), Duration::from_millis(40));
        pacing.observe(&failed("x"), 0);
        assert_eq!(pacing.computed_delay(), Duration::from_millis(50));
        assert!(pacing.at_cap());
        let clamped = RemitBackoff::with_bounds(Duration::from_secs(5), Duration::from_secs(1));
        assert_eq!(clamped.computed_delay(), Duration::from_secs(5));
        assert!(clamped.at_cap());
    }

    // Addendum 3 RULING 1: the first attempt after boot fires no earlier than one base delay, with
    // an additive jitter in [0, base] — [30 s, 60 s]. Zero is never a legal first delay.
    #[test]
    fn the_boot_delay_is_never_less_than_the_base_and_at_most_twice_it() {
        assert_eq!(boot_delay_for(RETRY_BASE, 0), Duration::from_secs(30));
        assert_eq!(
            boot_delay_for(RETRY_BASE, u64::MAX),
            Duration::from_secs(60)
        );
        let mid = boot_delay_for(RETRY_BASE, u64::MAX / 2);
        assert!(mid > Duration::from_secs(44) && mid < Duration::from_secs(46));
        for entropy in [0, 1, u64::MAX / 3, u64::MAX / 2, u64::MAX - 1, u64::MAX] {
            let delay = boot_delay_for(RETRY_BASE, entropy);
            assert!(delay >= RETRY_BASE, "{delay:?} is below the base");
            assert!(delay <= RETRY_BASE * 2, "{delay:?} is above twice the base");
        }
        assert_eq!(boot_delay_for(Duration::MAX, u64::MAX), Duration::MAX);
        let pacing = RemitBackoff::new();
        for _ in 0..64 {
            let drawn = pacing.boot_delay();
            assert!(drawn >= RETRY_BASE && drawn <= RETRY_BASE * 2, "{drawn:?}");
        }
        // Explicit bounds: the boot delay follows the base the test set.
        let short =
            RemitBackoff::with_bounds(Duration::from_millis(25), Duration::from_millis(100));
        let drawn = short.boot_delay();
        assert!(drawn >= Duration::from_millis(25) && drawn <= Duration::from_millis(50));
    }

    // Full jitter is a uniform point in [0, computed]: the two extremes are exact, and a huge
    // computed delay does not overflow.
    #[test]
    fn full_jitter_stays_inside_zero_to_computed() {
        let computed = Duration::from_secs(1800);
        assert_eq!(jittered(computed, 0), Duration::ZERO);
        assert_eq!(jittered(computed, u64::MAX), computed);
        let half = jittered(computed, u64::MAX / 2);
        assert!(half > Duration::from_secs(899) && half < Duration::from_secs(901));
        assert!(jittered(Duration::MAX, u64::MAX) <= Duration::MAX);
        assert_eq!(jittered(Duration::ZERO, u64::MAX), Duration::ZERO);
    }

    // Addendum 2 §3: one attempt in flight, ever; a second taker skips (gets `None`) and does not
    // block; the slot frees when the permit drops.
    #[test]
    fn single_flight_admits_one_attempt_and_frees_the_slot_on_drop() {
        let flight = RemitFlight::new();
        assert!(!flight.in_flight());
        let permit = flight.try_acquire().expect("the slot starts free");
        assert!(flight.in_flight());
        assert!(
            flight.try_acquire().is_none(),
            "a second taker skips while the first holds the slot"
        );
        let sibling = flight.clone();
        assert!(
            sibling.try_acquire().is_none(),
            "clones share the one slot — the loop and the collect thread see the same guard"
        );
        drop(permit);
        assert!(!flight.in_flight());
        let again = sibling.try_acquire();
        assert!(
            again.is_some(),
            "the slot is free again once the permit dropped"
        );
        // A permit dropped by a panicking thread frees the slot too: Drop runs on unwind.
        let flight_for_thread = flight.clone();
        drop(again);
        let outcome = std::thread::spawn(move || {
            let _permit = flight_for_thread.try_acquire().expect("free");
            panic!("attempt died");
        })
        .join();
        assert!(outcome.is_err());
        assert!(!flight.in_flight(), "the slot is not leaked by a panic");
    }
}
