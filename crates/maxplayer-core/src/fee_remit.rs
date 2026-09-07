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
//! `--confirm` pays regardless. There is no startup sweep, no detached task, and no other call site.
//!
//! ## What one attempt does
//!
//! Reconcile any in-flight attempt first; read the unremitted balance; resolve
//! [`PLATFORM_FEE_ADDRESS`] over LNURL-pay ([`crate::lnurl_pay`], fail-closed) and refuse below its
//! minimum — the expected steady state for small sellers, not an error; probe the mint's melt fee
//! reserve on the gross and invoice the **net**, so the fee comes OUT of the accrued amount and a
//! seller never pays more than it accrued; journal the plan (which pins the receipts and refuses a
//! duplicate — the idempotency that makes two concurrent collects pay at most once); melt through
//! [`crate::wallet_ops::melt_blocking`], the same gated melt `maxplayer wallet melt` uses (it honours
//! `allow_real_mints`); settle. Every attempt that meant to pay is journaled with its outcome
//! (`fee_remit_attempts`), so a payout that keeps failing is visible in the read-out rather than
//! silent.
//!
//! Every effect on the world goes through [`RemitEffects`], so the decision logic is tested against
//! scripted effects without a network or a mint. Exactly one method of that trait spends:
//! [`RemitEffects::melt`].

use std::fmt;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::home::MaxplayerHome;
use crate::lnurl_pay::{self, HttpsFetch, LightningAddress, PayRequest, ResolvedInvoice};
use crate::platform_fee::PLATFORM_FEE_ADDRESS;
use crate::seller_node::store::{
    PlanRefused, RemitAttempt, RemitAttemptOutcome, RemitAttemptTrigger, RemittancePlan,
    SellerStore,
};
use crate::wallet_ops::{self, MeltEstimate, MeltOutcome, MeltQuoteState, MeltQuoteStatus};

/// How many journaled attempts the command prints, newest first.
pub const RECENT_ATTEMPTS_SHOWN: usize = 5;

/// The remit path's effects on the world, behind a trait so the decision logic — what is paid,
/// when, and what is refused — is tested without a network or a mint. Exactly one method moves
/// money: [`Self::melt`]. Everything else reads.
pub trait RemitEffects {
    /// LNURL step 1–2: the destination's payRequest (callback + sendable bounds).
    fn pay_request(&mut self, address: &LightningAddress) -> Result<PayRequest, String>;
    /// LNURL step 3–4: an invoice for exactly `amount_sats`.
    fn invoice(&mut self, pay: &PayRequest, amount_sats: u64) -> Result<ResolvedInvoice, String>;
    /// A melt quote for the invoice — the mint's fee reserve — WITHOUT paying.
    fn melt_estimate(&mut self, bolt11: &str) -> Result<MeltEstimate, String>;
    /// **The payment.** Pays the invoice from the seller's ecash. The only method here that spends.
    fn melt(&mut self, bolt11: &str) -> Result<MeltOutcome, String>;
    /// What the mint says about the melt quote(s) this wallet raised for the invoice, if any —
    /// used to reconcile an interrupted attempt.
    fn melt_status(&mut self, bolt11: &str) -> Result<Option<MeltQuoteStatus>, String>;
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
    fn pay_request(&mut self, address: &LightningAddress) -> Result<PayRequest, String> {
        lnurl_pay::fetch_pay_request(&self.fetch, address).map_err(|error| error.to_string())
    }

    fn invoice(&mut self, pay: &PayRequest, amount_sats: u64) -> Result<ResolvedInvoice, String> {
        lnurl_pay::request_invoice(&self.fetch, pay, amount_sats).map_err(|error| error.to_string())
    }

    fn melt_estimate(&mut self, bolt11: &str) -> Result<MeltEstimate, String> {
        wallet_ops::melt_quote_blocking(&self.home, bolt11, None).map_err(|error| error.to_string())
    }

    fn melt(&mut self, bolt11: &str) -> Result<MeltOutcome, String> {
        wallet_ops::melt_blocking(&self.home, bolt11, None).map_err(|error| error.to_string())
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
///    nothing may be planned on top of it. PAID ⇒ settle; UNPAID/FAILED/no quote ⇒ fail and release;
///    PENDING ⇒ refuse this run.
/// 2. Read the unremitted total. Zero ⇒ refuse (nothing to do), before any network.
/// 3. Resolve the destination; refuse below its minimum with the shortfall (expected for small
///    sellers, not an error).
/// 4. Probe the melt fee reserve on an invoice for the GROSS, then invoice for `gross − reserve` so
///    the fee comes out of the accrued amount — a seller never pays more than it accrued — and check
///    the second quote still fits.
/// 5. Print the plan. A dry run stops here.
/// 6. Journal the plan (pins the receipts; refuses a duplicate), then melt, then settle. A melt
///    error leaves the row `planned` for step 1 of the next run.
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

    // 1. Reconcile an in-flight attempt before anything else.
    if let Some(active) = store
        .in_flight_remittance()
        .map_err(|error| format!("read remittances: {error}"))?
    {
        trace.remittance_id = Some(active.remittance_id.clone());
        let _ = writeln!(
            out,
            "Reconciling in-flight remittance {} (planned at unix {}: {} sats to {}, gross {} sats)",
            active.remittance_id,
            active.created_at_unix,
            active.net_sats,
            active.destination,
            active.gross_sats
        );
        match effects.melt_status(&active.bolt11)? {
            None => {
                store
                    .fail_remittance(&active.remittance_id, now_unix)
                    .map_err(|error| format!("record failed remittance: {error}"))?;
                let _ = writeln!(
                    out,
                    "  the wallet never raised a melt quote for its invoice — no sats left the wallet; released {} sats back to unremitted",
                    active.gross_sats
                );
            }
            Some(status) => match status.state {
                MeltQuoteState::Paid => {
                    store
                        .settle_remittance(
                            &active.remittance_id,
                            None,
                            None,
                            Some(&status.quote_id),
                            now_unix,
                        )
                        .map_err(|error| format!("record settled remittance: {error}"))?;
                    let _ = writeln!(
                        out,
                        "  mint {} reports melt quote {} PAID — recorded as settled: {} sats reached {} (melt fee not observed by this run)",
                        status.mint_url, status.quote_id, active.net_sats, active.destination
                    );
                }
                MeltQuoteState::Unpaid | MeltQuoteState::Failed => {
                    store
                        .fail_remittance(&active.remittance_id, now_unix)
                        .map_err(|error| format!("record failed remittance: {error}"))?;
                    let _ = writeln!(
                        out,
                        "  mint {} reports melt quote {} {} — no sats left the wallet; released {} sats back to unremitted",
                        status.mint_url, status.quote_id, status.state, active.gross_sats
                    );
                }
                MeltQuoteState::Pending | MeltQuoteState::Unknown => {
                    let _ = writeln!(
                        out,
                        "  mint {} reports melt quote {} {}: the payment is still settling. REFUSED — nothing moved by this run; re-run later to reconcile.",
                        status.mint_url, status.quote_id, status.state
                    );
                    return Ok(RemitOutcome::Refused(Refusal::Settling {
                        remittance_id: active.remittance_id,
                    }));
                }
            },
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

    // 6. Journal, pay, settle.
    let plan = RemittancePlan {
        payment_hash: invoice.payment_hash.clone(),
        gross_sats: gross,
        net_sats: net,
        destination: address.to_string(),
        bolt11: invoice.bolt11.clone(),
        melt_quote_id: Some(estimate.quote_id.clone()),
    };
    let planned = match store.plan_remittance(&plan, now_unix) {
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
        "Journaled remittance {} covering {} receipt{}; paying...",
        planned.remittance_id,
        planned.receipts,
        if planned.receipts == 1 { "" } else { "s" }
    );
    match effects.melt(&invoice.bolt11) {
        Ok(outcome) => {
            let settled = store
                .settle_remittance(
                    &planned.remittance_id,
                    Some(outcome.paid_sats),
                    Some(outcome.fee_sats),
                    Some(&outcome.quote_id),
                    now_unix,
                )
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
                "PAID — remittance {} settled\n  gross discharged: {} sats\n  melt fee taken by the mint: {} sats\n  net paid to {}: {} sats\n  stays in your wallet (unused reserve): {} sats\n  wallet balance now: {} sats at {}\n  receipts discharged: {}",
                settled.remittance_id,
                settled.gross_sats,
                outcome.fee_sats,
                settled.destination,
                outcome.paid_sats,
                gross.saturating_sub(debit),
                outcome.balance_sats,
                outcome.mint_url,
                settled.receipts
            );
            if debit > gross {
                let _ = writeln!(
                    out,
                    "WARNING: the mint debited {debit} sats against {gross} sats accrued — more than the quoted ceiling. Recorded as settled; report this."
                );
            }
            Ok(RemitOutcome::Paid {
                remittance_id: settled.remittance_id,
                net_sats: outcome.paid_sats,
                melt_fee_sats: outcome.fee_sats,
            })
        }
        Err(error) => {
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
            Err(error) => format!(
                "attempt FAILED ({error}); the balance stays unremitted and the node retries with backoff while it runs"
            ),
        }
    }

    /// Whether this attempt counts as a FAILURE for pacing ([`RemitBackoff::observe`]): it meant to
    /// pay and did not, for a reason that is not the steady state. `Err` (an effect failed),
    /// `MeltFailed`, and every refusal that is not at the threshold — the balance stays owed and
    /// hammering the same host or mint every 30 s would not change that. A threshold refusal, a
    /// payment and a dry run are not failures.
    pub fn is_failure(&self) -> bool {
        match &self.outcome {
            Err(_) | Ok(RemitOutcome::MeltFailed { .. }) => true,
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

/// The retry tick's base delay: the first attempt after boot waits this long (jittered), and a streak
/// of failures doubles it from here.
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
/// `[0, computed_delay]`, not the delay plus a small wobble ([`Self::next_delay`]), and the first
/// attempt after boot draws from `[0, base]` for the same reason: a fleet restarting together must
/// not all attempt at once.
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
        let mut bytes = [0u8; 8];
        let entropy = match getrandom::fill(&mut bytes) {
            Ok(()) => u64::from_le_bytes(bytes),
            Err(_) => u64::MAX,
        };
        jittered(self.computed_delay(), entropy)
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::RemitEffects;
    use crate::lnurl_pay::{LightningAddress, PayRequest, ResolvedInvoice, Url};
    use crate::wallet_ops::{MeltEstimate, MeltOutcome, MeltQuoteStatus};

    /// Scripted effects. `reserve_for(amount)` is the mint's fee reserve policy; `melt_results` are
    /// consumed in order; `status` answers the reconciliation query; `pay_request_error` makes the
    /// LNURL host unreachable. Every call is logged so a test can assert what was — and was not —
    /// touched; `melt_counter`, when set, counts melts across Fakes on different threads.
    pub(crate) struct Fake {
        pub(crate) min_msat: u64,
        pub(crate) max_msat: u64,
        pub(crate) reserve_for: Box<dyn Fn(u64) -> u64>,
        pub(crate) melt_results: Vec<Result<(u64, u64), String>>,
        pub(crate) status: Result<Option<MeltQuoteStatus>, String>,
        pub(crate) pay_request_error: Option<String>,
        pub(crate) pay_requests: usize,
        pub(crate) invoices: Vec<u64>,
        pub(crate) estimates: Vec<String>,
        pub(crate) melts: Vec<String>,
        pub(crate) status_calls: Vec<String>,
        pub(crate) melt_counter: Option<Arc<AtomicUsize>>,
    }

    impl Fake {
        pub(crate) fn new(reserve_for: impl Fn(u64) -> u64 + 'static) -> Self {
            Self {
                min_msat: 1000,
                max_msat: 1_000_000_000,
                reserve_for: Box::new(reserve_for),
                melt_results: Vec::new(),
                status: Ok(None),
                pay_request_error: None,
                pay_requests: 0,
                invoices: Vec::new(),
                estimates: Vec::new(),
                melts: Vec::new(),
                status_calls: Vec::new(),
                melt_counter: None,
            }
        }

        pub(crate) fn bolt11_for(amount_sats: u64, sequence: usize) -> String {
            format!("lnbc-fake-{amount_sats}-{sequence}")
        }

        pub(crate) fn hash_for(amount_sats: u64, sequence: usize) -> String {
            format!("hash-{amount_sats}-{sequence}")
        }
    }

    impl RemitEffects for Fake {
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
                bolt11: Self::bolt11_for(amount_sats, sequence),
                payment_hash: Self::hash_for(amount_sats, sequence),
                amount_sats,
                amount_msat: amount_sats * 1000,
            })
        }

        fn melt_estimate(&mut self, bolt11: &str) -> Result<MeltEstimate, String> {
            self.estimates.push(bolt11.to_owned());
            let amount_sats: u64 = bolt11
                .split('-')
                .nth(2)
                .and_then(|raw| raw.parse().ok())
                .expect("fake bolt11 carries its amount");
            Ok(MeltEstimate {
                mint_url: "https://mint.example".to_owned(),
                quote_id: format!("quote-{bolt11}"),
                amount_sats,
                fee_reserve_sats: (self.reserve_for)(amount_sats),
            })
        }

        fn melt(&mut self, bolt11: &str) -> Result<MeltOutcome, String> {
            self.melts.push(bolt11.to_owned());
            if let Some(counter) = &self.melt_counter {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            let (paid, fee) = self.melt_results.remove(0)?;
            Ok(MeltOutcome {
                mint_url: "https://mint.example".to_owned(),
                paid_sats: paid,
                fee_sats: fee,
                balance_sats: 1_000,
                quote_id: format!("paid-quote-{bolt11}"),
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
            "Journaled remittance hash-13-2 covering 2 receipts; paying...",
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
            out.contains("Reconciling in-flight remittance hash-9-2 (planned at unix 100: 9 sats to maxplayer@agi.cash, gross 10 sats)"),
            "{out}"
        );
        assert!(
            out.contains("reports melt quote paid-quote-lnbc-fake-9-2 PAID — recorded as settled: 9 sats reached maxplayer@agi.cash (melt fee not observed by this run)"),
            "{out}"
        );
        assert!(out.contains("Nothing to remit."), "{out}");
        assert_eq!(fake.status_calls, vec!["lnbc-fake-9-2".to_owned()]);
        assert_eq!(fake.melts.len(), 1, "reconciliation never melts");
        let rows = store.remittances().expect("rows");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, RemittanceState::Settled);
        assert_eq!(rows[0].melt_fee_sats, None, "unobserved, not invented");
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
        assert!(out.contains("reports melt quote q-unpaid UNPAID — no sats left the wallet; released 10 sats back to unremitted"), "{out}");
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
        assert!(out.contains("the wallet never raised a melt quote for its invoice — no sats left the wallet; released 10 sats back to unremitted"), "{out}");
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
