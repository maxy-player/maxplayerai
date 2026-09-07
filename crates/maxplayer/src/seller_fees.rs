//! `maxplayer seller fees` — the seller-facing read-out of the platform fee journal — and
//! `maxplayer seller fees remit`, the ONE command in this product that pays the accrued fee out.
//!
//! `seller fees` prints, per collected job and as a total, the four figures a seller needs to see
//! without doing arithmetic or reading source: what the buyer paid (the offer amount), the mint's
//! swap fee, the platform fee (rate and sats), and what the seller keeps — plus how much of the
//! platform fee has been remitted, how much is unremitted, and every remittance so far. Read-only as
//! to money: it opens `seller.sqlite` (applying the store's additive schema migration if the file
//! predates the current version), reads, prints, and exits.
//!
//! `seller fees remit` resolves the platform's Lightning address over LNURL-pay, takes a melt quote
//! for the unremitted balance, and prints the plan. **Without `--confirm` that is all it does** (a
//! dry run is the default). With `--confirm` it journals the attempt, pays the invoice from the
//! seller's ecash through `wallet_ops::melt_blocking` — the same gated melt `maxplayer wallet melt`
//! uses, honouring `allow_real_mints` — and records the settlement so the same sats are never paid
//! twice. The command is idempotent: a second `--confirm` after a settled remittance finds nothing
//! unremitted and pays nothing; an interrupted one is reconciled with the mint on the next run, never
//! repeated. **Nothing else in this binary calls the remit path** — no timer, no startup sweep,
//! nothing on the payment path.

use std::io::Write;
use std::path::PathBuf;

const SUCCESS: i32 = 0;
const USAGE_ERROR: i32 = 1;
const RUNTIME_ERROR: i32 = 2;
/// `remit` declined and moved nothing: nothing unremitted, a balance below the destination's
/// minimum, a fee reserve that does not fit, or a payment still settling at the mint. Distinct from
/// `SUCCESS` so a script cannot read a refusal as a payment.
const REFUSED: i32 = 3;

/// What the arguments asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Command {
    /// `maxplayer seller fees [--home <dir>]`
    Ledger { home: Option<PathBuf> },
    /// `maxplayer seller fees remit [--home <dir>] [--dry-run | --confirm]`
    Remit {
        home: Option<PathBuf>,
        confirm: bool,
    },
}

/// Entry from `sell::run` for `maxplayer seller fees ...`.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if crate::cli::is_help_request(args) {
        usage(out);
        return SUCCESS;
    }
    let command = match parse_command(args) {
        Ok(command) => command,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            usage(err);
            return USAGE_ERROR;
        }
    };

    #[cfg(not(feature = "wallet"))]
    {
        let _ = (command, out);
        let _ = writeln!(
            err,
            "maxplayer seller fees requires the wallet feature (rebuild with default features)"
        );
        USAGE_ERROR
    }

    #[cfg(feature = "wallet")]
    {
        let result = match command {
            Command::Ledger { home } => print_ledger(home, out).map(|()| SUCCESS),
            Command::Remit { home, confirm } => remit_live(home, confirm, out),
        };
        match result {
            Ok(code) => code,
            Err(message) => {
                let _ = writeln!(err, "{message}");
                RUNTIME_ERROR
            }
        }
    }
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    let (remit, options) = match args.first().map(String::as_str) {
        Some("remit") => (true, &args[1..]),
        _ => (false, args),
    };
    let mut home = None;
    let mut confirm = false;
    let mut dry_run = false;
    let mut idx = 0;
    while idx < options.len() {
        match options[idx].as_str() {
            "--home" => {
                idx += 1;
                home = Some(PathBuf::from(
                    options.get(idx).ok_or("--home requires a value")?,
                ));
            }
            "--confirm" if remit => confirm = true,
            "--dry-run" if remit => dry_run = true,
            other => {
                return Err(format!(
                    "unknown seller fees{} option: {other}",
                    if remit { " remit" } else { "" }
                ));
            }
        }
        idx += 1;
    }
    if confirm && dry_run {
        return Err("--confirm and --dry-run contradict each other; pass one".to_owned());
    }
    Ok(if remit {
        Command::Remit { home, confirm }
    } else {
        Command::Ledger { home }
    })
}

fn usage(w: &mut dyn Write) {
    let _ = writeln!(
        w,
        "Usage:\n  maxplayer seller fees [--home <dir>]\n  maxplayer seller fees remit [--home <dir>] [--dry-run | --confirm]\n\n`seller fees` prints, for every job this seat has collected payment on, what the buyer paid, the\nmint's fee, the platform fee (rate and sats) and what you keep, then the totals — the platform fee\nbroken out by the rate each job was recorded at and split into remitted / unremitted — and every\nremittance so far. Moves no sats (it only opens, reads and prints the seller store).\n\n`seller fees remit` pays the UNREMITTED platform fee to the platform's Lightning address\n(fixed in the product; not configurable). The default is a DRY RUN: it resolves the address,\nquotes the mint's melt fee, prints the plan and moves nothing. Only `--confirm` pays, and it\npays at most the unremitted total — the mint's melt fee comes out of that amount, never on top.\nIt refuses (exit 3, nothing moved) when nothing is unremitted, when the balance is below the\ndestination's minimum (small balances accumulate until they clear it), or when an earlier\nattempt is still settling. Running it again after a payment pays nothing: the receipts it\ndischarged are recorded, and an interrupted attempt is reconciled with the mint, not repeated.\nExit 0 = dry run printed or payment made; 1 = usage; 2 = error; 3 = refused, nothing moved."
    );
}

#[cfg(feature = "wallet")]
fn open_store(
    home: &Option<PathBuf>,
) -> Result<
    (
        maxplayer_core::seller_node::store::SellerStore,
        PathBuf,
        PathBuf,
    ),
    String,
> {
    use maxplayer_core::seller_node::STATE_DB_FILE;
    use maxplayer_core::seller_node::store::SellerStore;

    let root = match home {
        Some(path) => path.clone(),
        None => maxplayer_core::home::default_home_dir()
            .map_err(|error| format!("resolve home: {error}"))?,
    };
    let db = root.join(STATE_DB_FILE);
    if !db.exists() {
        return Err(format!(
            "no seller store at {} — this seat has not run `maxplayer seller` yet",
            db.display()
        ));
    }
    let store =
        SellerStore::open(&db).map_err(|error| format!("open {}: {error}", db.display()))?;
    Ok((store, root, db))
}

#[cfg(feature = "wallet")]
fn print_ledger(home: Option<PathBuf>, out: &mut dyn Write) -> Result<(), String> {
    let (store, _root, db) = open_store(&home)?;
    let accrued = store
        .accrued_fees()
        .map_err(|error| format!("read receipts: {error}"))?;
    let remittances = store
        .remittances()
        .map_err(|error| format!("read remittances: {error}"))?;
    let _ = write!(
        out,
        "{}",
        render(&accrued, &remittances, &db.display().to_string())
    );
    Ok(())
}

/// The ledger as text. Pure, so a test can assert on the exact words a seller reads.
#[cfg(feature = "wallet")]
pub(crate) fn render(
    accrued: &maxplayer_core::seller_node::store::AccruedFees,
    remittances: &[maxplayer_core::seller_node::store::FeeRemittance],
    db: &str,
) -> String {
    use maxplayer_core::platform_fee::bps_to_percent_label;
    use maxplayer_core::seller_node::store::RemittanceState;

    let mut text = String::new();
    text.push_str(&format!("Seller fee ledger — {db}\n"));
    if accrued.by_job.is_empty() {
        text.push_str("No collected payments yet.\n");
        return text;
    }
    text.push_str(&format!(
        "{} collected job{}, oldest first:\n",
        accrued.by_job.len(),
        if accrued.by_job.len() == 1 { "" } else { "s" }
    ));
    for row in &accrued.by_job {
        let mint_fee = match row.mint_fee_sats {
            Some(fee) => format!("{fee} sats"),
            None => "not recorded (collected before this version tracked it)".to_owned(),
        };
        let kept = match row.kept_sats() {
            Some(kept) => format!("{kept} sats"),
            None => "unknown (mint fee not recorded)".to_owned(),
        };
        let remitted = match &row.remittance_id {
            Some(id) => format!("remittance {id}"),
            None => "unremitted".to_owned(),
        };
        text.push_str(&format!(
            "  job {}\n    what the buyer paid: {} sats\n    mint fee: {}\n    platform fee ({}): {} sats — {}\n    you keep: {}\n",
            row.job_id,
            row.amount_sats,
            mint_fee,
            bps_to_percent_label(row.fee_bps),
            row.fee_sats,
            remitted,
            kept
        ));
    }
    text.push_str("Totals:\n");
    text.push_str(&format!(
        "  what buyers paid: {} sats\n",
        accrued.total_amount_sats
    ));
    if accrued.rows_without_mint_fee == 0 {
        text.push_str(&format!(
            "  mint fees: {} sats\n",
            accrued.total_mint_fee_sats
        ));
    } else {
        text.push_str(&format!(
            "  mint fees: {} sats across the jobs that recorded one, plus {} job{} whose mint fee was not recorded\n",
            accrued.total_mint_fee_sats,
            accrued.rows_without_mint_fee,
            if accrued.rows_without_mint_fee == 1 { "" } else { "s" }
        ));
    }
    let in_flight = if accrued.in_flight_fee_sats == 0 {
        String::new()
    } else {
        format!(
            ", {} sats in flight (a remittance is settling)",
            accrued.in_flight_fee_sats
        )
    };
    text.push_str(&format!(
        "  platform fee: {} sats accrued — {} sats remitted, {} sats unremitted{}\n",
        accrued.total_fee_sats, accrued.remitted_fee_sats, accrued.unremitted_fee_sats, in_flight
    ));
    // The rate the total was taken at, never assumed: rows can carry different recorded rates (a
    // store that collected before the rate was set holds 0% rows beside 10% rows), so the total is
    // broken out by the rate each row was written at, and no single rate is invented over the mix.
    for rate in platform_fee_by_rate(&accrued.by_job) {
        text.push_str(&format!(
            "    at {}: {} sats on {} sats paid, {} job{}\n",
            bps_to_percent_label(rate.fee_bps),
            rate.fee_sats,
            rate.amount_sats,
            rate.jobs,
            if rate.jobs == 1 { "" } else { "s" }
        ));
    }
    match accrued.total_kept_sats() {
        Some(kept) if accrued.rows_without_mint_fee == 0 => {
            text.push_str(&format!("  you keep: {kept} sats\n"));
        }
        Some(kept) => {
            text.push_str(&format!(
                "  you keep: {kept} sats across the jobs that recorded a mint fee; the rest is unknown\n"
            ));
        }
        None => {
            text.push_str("  you keep: unknown (no job recorded its mint fee)\n");
        }
    }
    text.push_str("Remittances:\n");
    if remittances.is_empty() {
        text.push_str(
            "  none yet — `maxplayer seller fees remit` shows the plan; `--confirm` pays the unremitted balance\n",
        );
    }
    for row in remittances {
        let melt_fee = match row.melt_fee_sats {
            Some(fee) => format!("{fee} sats"),
            None => "not observed".to_owned(),
        };
        let state = match row.state {
            RemittanceState::Planned => "PLANNED (settling — re-run remit to reconcile)",
            RemittanceState::Settled => "settled",
            RemittanceState::Failed => "failed (no sats left; receipts released)",
        };
        text.push_str(&format!(
            "  {}: {} sats to {} — gross {} sats, melt fee {}, invoice {}, {} receipt{}, planned at unix {}{}\n",
            state,
            row.net_sats,
            row.destination,
            row.gross_sats,
            melt_fee,
            row.payment_hash,
            row.receipts,
            if row.receipts == 1 { "" } else { "s" },
            row.created_at_unix,
            match row.settled_at_unix {
                Some(at) => format!(", resolved at unix {at}"),
                None => String::new(),
            }
        ));
    }
    text
}

/// The platform fee total for one recorded rate: what it came to, on how much paid, over how many
/// jobs. Produced by [`platform_fee_by_rate`].
#[cfg(feature = "wallet")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FeeAtRate {
    pub(crate) fee_bps: u32,
    pub(crate) fee_sats: u64,
    pub(crate) amount_sats: u64,
    pub(crate) jobs: usize,
}

/// Group the journaled rows by the rate each was recorded at, ascending by rate. Pure aggregation
/// of stored figures — the fee per row is read as written, never recomputed here.
#[cfg(feature = "wallet")]
pub(crate) fn platform_fee_by_rate(
    by_job: &[maxplayer_core::seller_node::store::JobFeeAccrual],
) -> Vec<FeeAtRate> {
    let mut rates: Vec<FeeAtRate> = Vec::new();
    for row in by_job {
        match rates.iter_mut().find(|rate| rate.fee_bps == row.fee_bps) {
            Some(rate) => {
                rate.fee_sats = rate.fee_sats.saturating_add(row.fee_sats);
                rate.amount_sats = rate.amount_sats.saturating_add(row.amount_sats);
                rate.jobs += 1;
            }
            None => rates.push(FeeAtRate {
                fee_bps: row.fee_bps,
                fee_sats: row.fee_sats,
                amount_sats: row.amount_sats,
                jobs: 1,
            }),
        }
    }
    rates.sort_by_key(|rate| rate.fee_bps);
    rates
}

// ---- remit ------------------------------------------------------------------------------------

/// The remit command's effects on the world, behind a trait so the decision logic — what is paid,
/// when, and what is refused — is tested without a network or a mint. Exactly one method moves
/// money: [`Self::melt`]. Everything else reads.
#[cfg(feature = "wallet")]
pub(crate) trait RemitEffects {
    /// LNURL step 1–2: the destination's payRequest (callback + sendable bounds).
    fn pay_request(
        &mut self,
        address: &maxplayer_core::lnurl_pay::LightningAddress,
    ) -> Result<maxplayer_core::lnurl_pay::PayRequest, String>;
    /// LNURL step 3–4: an invoice for exactly `amount_sats`.
    fn invoice(
        &mut self,
        pay: &maxplayer_core::lnurl_pay::PayRequest,
        amount_sats: u64,
    ) -> Result<maxplayer_core::lnurl_pay::ResolvedInvoice, String>;
    /// A melt quote for the invoice — the mint's fee reserve — WITHOUT paying.
    fn melt_estimate(
        &mut self,
        bolt11: &str,
    ) -> Result<maxplayer_core::wallet_ops::MeltEstimate, String>;
    /// **The payment.** Pays the invoice from the seller's ecash. The only method here that spends.
    fn melt(&mut self, bolt11: &str) -> Result<maxplayer_core::wallet_ops::MeltOutcome, String>;
    /// What the mint says about the melt quote(s) this wallet raised for the invoice, if any —
    /// used to reconcile an interrupted attempt.
    fn melt_status(
        &mut self,
        bolt11: &str,
    ) -> Result<Option<maxplayer_core::wallet_ops::MeltQuoteStatus>, String>;
}

/// The shipped effects: LNURL over https, the packaged CDK wallet at `home`, the home's default
/// mint (the first accepted mint — where a seller's receipts land).
#[cfg(feature = "wallet")]
struct LiveEffects {
    home: maxplayer_core::home::MaxplayerHome,
    fetch: maxplayer_core::lnurl_pay::HttpsFetch,
}

#[cfg(feature = "wallet")]
impl RemitEffects for LiveEffects {
    fn pay_request(
        &mut self,
        address: &maxplayer_core::lnurl_pay::LightningAddress,
    ) -> Result<maxplayer_core::lnurl_pay::PayRequest, String> {
        maxplayer_core::lnurl_pay::fetch_pay_request(&self.fetch, address)
            .map_err(|error| error.to_string())
    }

    fn invoice(
        &mut self,
        pay: &maxplayer_core::lnurl_pay::PayRequest,
        amount_sats: u64,
    ) -> Result<maxplayer_core::lnurl_pay::ResolvedInvoice, String> {
        maxplayer_core::lnurl_pay::request_invoice(&self.fetch, pay, amount_sats)
            .map_err(|error| error.to_string())
    }

    fn melt_estimate(
        &mut self,
        bolt11: &str,
    ) -> Result<maxplayer_core::wallet_ops::MeltEstimate, String> {
        maxplayer_core::wallet_ops::melt_quote_blocking(&self.home, bolt11, None)
            .map_err(|error| error.to_string())
    }

    fn melt(&mut self, bolt11: &str) -> Result<maxplayer_core::wallet_ops::MeltOutcome, String> {
        maxplayer_core::wallet_ops::melt_blocking(&self.home, bolt11, None)
            .map_err(|error| error.to_string())
    }

    fn melt_status(
        &mut self,
        bolt11: &str,
    ) -> Result<Option<maxplayer_core::wallet_ops::MeltQuoteStatus>, String> {
        maxplayer_core::wallet_ops::melt_status_for_invoice_blocking(&self.home, bolt11, None)
            .map_err(|error| error.to_string())
    }
}

#[cfg(feature = "wallet")]
fn remit_live(home: Option<PathBuf>, confirm: bool, out: &mut dyn Write) -> Result<i32, String> {
    let (store, root, db) = open_store(&home)?;
    let home = maxplayer_core::home::bootstrap(&root)
        .map_err(|error| format!("open home {}: {error}", root.display()))?;
    let fetch = maxplayer_core::lnurl_pay::HttpsFetch::new().map_err(|error| error.to_string())?;
    let mut effects = LiveEffects { home, fetch };
    let now_unix = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("clock: {error}"))?
            .as_secs(),
    )
    .map_err(|error| format!("clock: {error}"))?;
    let _ = writeln!(out, "Platform fee remittance — {}", db.display());
    remit(&store, &mut effects, confirm, now_unix, out)
}

/// The remit decision logic. Returns the exit code; `Err` is a runtime error that is printed by the
/// caller. Every refusal prints its reason and returns [`REFUSED`] having moved nothing.
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
/// 5. Print the plan. Without `--confirm`, stop.
/// 6. Journal the plan (pins the receipts; refuses a duplicate), then melt, then settle. A melt
///    error leaves the row `planned` for step 1 of the next run.
#[cfg(feature = "wallet")]
pub(crate) fn remit(
    store: &maxplayer_core::seller_node::store::SellerStore,
    effects: &mut dyn RemitEffects,
    confirm: bool,
    now_unix: i64,
    out: &mut dyn Write,
) -> Result<i32, String> {
    use maxplayer_core::lnurl_pay::LightningAddress;
    use maxplayer_core::platform_fee::PLATFORM_FEE_ADDRESS;
    use maxplayer_core::seller_node::store::{PlanRefused, RemittancePlan};
    use maxplayer_core::wallet_ops::MeltQuoteState;

    // 1. Reconcile an in-flight attempt before anything else.
    if let Some(active) = store
        .in_flight_remittance()
        .map_err(|error| format!("read remittances: {error}"))?
    {
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
                    return Ok(REFUSED);
                }
            },
        }
    }

    // 2. What is owed.
    let accrued = store
        .accrued_fees()
        .map_err(|error| format!("read receipts: {error}"))?;
    let gross = accrued.unremitted_fee_sats;
    let _ = writeln!(
        out,
        "Accrued platform fee: {} sats all-time — {} sats remitted, {} sats unremitted",
        accrued.total_fee_sats, accrued.remitted_fee_sats, gross
    );
    if gross == 0 {
        let _ = writeln!(out, "Nothing to remit. REFUSED — nothing moved.");
        return Ok(REFUSED);
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
        return Ok(REFUSED);
    }
    if gross > max_sats {
        let _ = writeln!(
            out,
            "REFUSED — unremitted {gross} sats exceeds the destination's maximum of {max_sats} sats; this command remits the whole balance or nothing. Nothing moved."
        );
        return Ok(REFUSED);
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
        return Ok(REFUSED);
    }
    let net = gross - reserve;
    if net < min_sats {
        let _ = writeln!(
            out,
            "REFUSED — after the mint's melt fee reserve ({reserve} sats) the {gross} sats unremitted leaves {net} sats, below the destination's minimum of {min_sats} sats ({} sats short). The balance accumulates. Nothing moved.",
            min_sats - net
        );
        return Ok(REFUSED);
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
        return Ok(REFUSED);
    }

    // 5. The plan, in the seller's words.
    let _ = writeln!(
        out,
        "Plan:\n  unremitted platform fee (gross): {gross} sats\n  mint melt fee reserve (ceiling): {} sats — taken out of the gross, never on top\n  invoice amount ({address} receives): {net} sats\n  leaves your wallet: at most {debit_ceiling} sats (≤ {gross}); unused reserve returns as change\n  mint: {} (melt quote {})\n  invoice payment hash: {}",
        estimate.fee_reserve_sats, estimate.mint_url, estimate.quote_id, invoice.payment_hash
    );
    if !confirm {
        let _ = writeln!(
            out,
            "DRY RUN — nothing moved. Re-run with --confirm to pay {net} sats to {address}."
        );
        return Ok(SUCCESS);
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
            return Ok(REFUSED);
        }
    };
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
                         Remittance {} stays planned; re-run to reconcile with the mint before paying anything else.",
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
            Ok(SUCCESS)
        }
        Err(error) => {
            let _ = writeln!(
                out,
                "melt failed: {error}\n  remittance {} stays journaled as planned. The next `maxplayer seller fees remit` reconciles it with the mint: settled if the payment landed, released if it did not. Nothing else was attempted.",
                planned.remittance_id
            );
            Ok(RUNTIME_ERROR)
        }
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parse_command_reads_ledger_and_remit_forms_and_refuses_contradictions() {
        let strings = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse_command(&[]).expect("empty"),
            Command::Ledger { home: None }
        );
        assert_eq!(
            parse_command(&strings(&["--home", "/x"])).expect("home"),
            Command::Ledger {
                home: Some(PathBuf::from("/x"))
            }
        );
        assert_eq!(
            parse_command(&strings(&["remit"])).expect("remit"),
            Command::Remit {
                home: None,
                confirm: false
            },
            "dry run is the default"
        );
        assert_eq!(
            parse_command(&strings(&["remit", "--dry-run", "--home", "/x"])).expect("remit dry"),
            Command::Remit {
                home: Some(PathBuf::from("/x")),
                confirm: false
            }
        );
        assert_eq!(
            parse_command(&strings(&["remit", "--home", "/x", "--confirm"]))
                .expect("remit confirm"),
            Command::Remit {
                home: Some(PathBuf::from("/x")),
                confirm: true
            }
        );
        assert!(parse_command(&strings(&["--home"])).is_err());
        assert!(parse_command(&strings(&["--rate-sats"])).is_err());
        assert!(
            parse_command(&strings(&["--confirm"])).is_err(),
            "--confirm without remit is not a ledger option"
        );
        assert!(parse_command(&strings(&["remit", "--confirm", "--dry-run"])).is_err());
        assert!(parse_command(&strings(&["remit", "--yes"])).is_err());
        assert!(
            parse_command(&strings(&["remitt"])).is_err(),
            "a typo is not the ledger"
        );
    }
}

#[cfg(all(test, feature = "wallet"))]
mod tests {
    use super::*;
    use maxplayer_core::lnurl_pay::{LightningAddress, PayRequest, ResolvedInvoice, Url};
    use maxplayer_core::seller_node::store::{
        AccruedFees, FeeRemittance, JobFeeAccrual, ReceiptFees, RemittanceState, SellerStore,
    };
    use maxplayer_core::wallet_ops::{MeltEstimate, MeltOutcome, MeltQuoteState, MeltQuoteStatus};

    fn row(
        job: &str,
        face: u64,
        mint_fee: Option<u64>,
        fee_bps: u32,
        fee_sats: u64,
    ) -> JobFeeAccrual {
        JobFeeAccrual {
            job_id: job.to_owned(),
            amount_sats: face,
            mint_fee_sats: mint_fee,
            fee_bps,
            fee_sats,
            received_at_unix: 1,
            remittance_id: None,
        }
    }

    // Face 100, mint fee 1, platform fee 10% of the FACE = 10 ⇒ you keep 89. Every figure printed
    // with its plain label; nothing has to be computed by the reader.
    #[test]
    fn render_shows_paid_mint_fee_platform_fee_and_kept_with_plain_labels() {
        let accrued = AccruedFees {
            total_amount_sats: 100,
            total_mint_fee_sats: 1,
            rows_without_mint_fee: 0,
            total_fee_sats: 10,
            unremitted_fee_sats: 10,
            by_job: vec![row("job-a", 100, Some(1), 1000, 10)],
            ..AccruedFees::default()
        };
        let text = render(&accrued, &[], "/tmp/x/seller.sqlite");
        for needle in [
            "what the buyer paid: 100 sats",
            "mint fee: 1 sats",
            "platform fee (10%): 10 sats — unremitted",
            "you keep: 89 sats",
            "platform fee: 10 sats accrued — 0 sats remitted, 10 sats unremitted\n",
            // Round 3: the total carries its rate, as the usage text promises.
            "    at 10%: 10 sats on 100 sats paid, 1 job\n",
            "Remittances:\n  none yet",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        assert!(
            !text.contains("you keep: 90"),
            "10% of the face (10), not of the net (9), is the platform fee:\n{text}"
        );
    }

    // Round 3: a store holding rows recorded at two rates (the first commit of this branch journaled
    // at a placeholder 0%, later ones at 10%) breaks the platform fee total out per recorded rate and
    // never labels the mixed total with one invented rate.
    #[test]
    fn totals_break_the_platform_fee_out_by_recorded_rate_and_never_invent_one() {
        let by_job = vec![
            row("job-at-zero", 21, Some(1), 0, 0),
            row("job-at-ten", 100, Some(1), 1000, 10),
            row("job-at-ten-again", 50, Some(1), 1000, 5),
        ];
        assert_eq!(
            platform_fee_by_rate(&by_job),
            vec![
                FeeAtRate {
                    fee_bps: 0,
                    fee_sats: 0,
                    amount_sats: 21,
                    jobs: 1,
                },
                FeeAtRate {
                    fee_bps: 1000,
                    fee_sats: 15,
                    amount_sats: 150,
                    jobs: 2,
                },
            ]
        );
        let accrued = AccruedFees {
            total_amount_sats: 171,
            total_mint_fee_sats: 3,
            rows_without_mint_fee: 0,
            total_fee_sats: 15,
            unremitted_fee_sats: 15,
            by_job,
            ..AccruedFees::default()
        };
        let text = render(&accrued, &[], "db");
        let totals = text
            .split_once("Totals:\n")
            .map(|(_, totals)| totals)
            .expect("a totals block");
        assert!(
            totals
                .contains("  platform fee: 15 sats accrued — 0 sats remitted, 15 sats unremitted"),
            "{text}"
        );
        assert!(
            totals.contains("    at 0%: 0 sats on 21 sats paid, 1 job\n"),
            "{text}"
        );
        assert!(
            totals.contains("    at 10%: 15 sats on 150 sats paid, 2 jobs\n"),
            "{text}"
        );
        assert!(
            !totals.contains("platform fee (10%)") && !totals.contains("platform fee (0%)"),
            "a mixed-rate total must not be labelled with a single rate:\n{text}"
        );
        assert!(platform_fee_by_rate(&[]).is_empty());
    }

    // A row from before the mint fee was journaled says so, and never prints a measured 0 or a
    // kept figure it could not have computed. Totals name the gap instead of hiding it.
    #[test]
    fn render_says_not_recorded_for_a_legacy_row_instead_of_inventing_zero() {
        let accrued = AccruedFees {
            total_amount_sats: 121,
            total_mint_fee_sats: 1,
            rows_without_mint_fee: 1,
            total_fee_sats: 10,
            unremitted_fee_sats: 10,
            by_job: vec![
                row("old-job", 21, None, 0, 0),
                row("new-job", 100, Some(1), 1000, 10),
            ],
            ..AccruedFees::default()
        };
        let text = render(&accrued, &[], "db");
        assert!(text.contains("mint fee: not recorded"), "{text}");
        assert!(
            text.contains("you keep: unknown (mint fee not recorded)"),
            "{text}"
        );
        assert!(
            !text.contains("mint fee: 0 sats"),
            "a legacy row must not read as a measured zero:\n{text}"
        );
        assert!(
            text.contains("plus 1 job whose mint fee was not recorded"),
            "{text}"
        );
        assert!(
            text.contains("you keep: 89 sats across the jobs that recorded a mint fee"),
            "{text}"
        );
        assert!(text.contains("platform fee (0%): 0 sats"), "{text}");
        // Round 3: the totals name both recorded rates rather than one rate over the mix.
        assert!(
            text.contains("    at 0%: 0 sats on 21 sats paid, 1 job\n"),
            "{text}"
        );
        assert!(
            text.contains("    at 10%: 10 sats on 100 sats paid, 1 job\n"),
            "{text}"
        );
    }

    // Stage 2a: a discharged row names its remittance, the totals split remitted from unremitted,
    // and the remittance list prints every journaled figure — gross, melt fee, net, destination
    // literal, payment hash, state — so the seller can reconcile.
    #[test]
    fn render_names_the_remittance_on_discharged_rows_and_lists_remittances() {
        let mut discharged = row("job-a", 100, Some(1), 1000, 10);
        discharged.remittance_id = Some("abc123".to_owned());
        let accrued = AccruedFees {
            total_amount_sats: 150,
            total_mint_fee_sats: 2,
            rows_without_mint_fee: 0,
            total_fee_sats: 15,
            unremitted_fee_sats: 5,
            remitted_fee_sats: 10,
            in_flight_fee_sats: 0,
            by_job: vec![discharged, row("job-b", 50, Some(1), 1000, 5)],
        };
        let remittances = vec![
            FeeRemittance {
                remittance_id: "old".to_owned(),
                gross_sats: 7,
                melt_fee_sats: None,
                net_sats: 6,
                destination: "maxplayer@agi.cash".to_owned(),
                melt_quote_id: None,
                payment_hash: "old".to_owned(),
                bolt11: "ln-old".to_owned(),
                state: RemittanceState::Failed,
                created_at_unix: 5,
                settled_at_unix: Some(6),
                receipts: 0,
            },
            FeeRemittance {
                remittance_id: "abc123".to_owned(),
                gross_sats: 10,
                melt_fee_sats: Some(1),
                net_sats: 9,
                destination: "maxplayer@agi.cash".to_owned(),
                melt_quote_id: Some("q".to_owned()),
                payment_hash: "abc123".to_owned(),
                bolt11: "ln-abc".to_owned(),
                state: RemittanceState::Settled,
                created_at_unix: 7,
                settled_at_unix: Some(8),
                receipts: 1,
            },
        ];
        let text = render(&accrued, &remittances, "db");
        for needle in [
            "platform fee (10%): 10 sats — remittance abc123\n",
            "platform fee (10%): 5 sats — unremitted\n",
            "  platform fee: 15 sats accrued — 10 sats remitted, 5 sats unremitted\n",
            "Remittances:\n",
            "  failed (no sats left; receipts released): 6 sats to maxplayer@agi.cash — gross 7 sats, melt fee not observed, invoice old, 0 receipts, planned at unix 5, resolved at unix 6\n",
            "  settled: 9 sats to maxplayer@agi.cash — gross 10 sats, melt fee 1 sats, invoice abc123, 1 receipt, planned at unix 7, resolved at unix 8\n",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        assert!(!text.contains("none yet"), "{text}");
        // An in-flight figure is named when present.
        let in_flight = AccruedFees {
            in_flight_fee_sats: 5,
            unremitted_fee_sats: 0,
            ..accrued
        };
        let text = render(&in_flight, &remittances, "db");
        assert!(
            text.contains("0 sats unremitted, 5 sats in flight (a remittance is settling)"),
            "{text}"
        );
    }

    #[test]
    fn render_with_no_rows_says_so() {
        let text = render(&AccruedFees::default(), &[], "db");
        assert!(text.contains("No collected payments yet."), "{text}");
    }

    fn temp_home(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "maxplayer-seller-fees-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp home");
        root
    }

    // End to end through `run`: a real seller store at `--home`, one collected job (face 100, mint
    // fee 1, platform fee 10% = 10), printed with the four labels and `you keep: 89 sats`.
    #[test]
    fn run_reads_a_real_store_at_home_and_prints_the_ledger() {
        use maxplayer_core::seller_node::STATE_DB_FILE;

        let root = temp_home("ledger");
        {
            let store = SellerStore::open(root.join(STATE_DB_FILE)).expect("open store");
            store
                .collect_receipt(
                    "receipt-1",
                    "job-1",
                    100,
                    ReceiptFees {
                        mint_fee_sats: 1,
                        fee_bps: 1000,
                        fee_sats: 10,
                    },
                    1,
                )
                .expect("collect");
        }

        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &["--home".to_owned(), root.display().to_string()],
            &mut out,
            &mut err,
        );
        let out = String::from_utf8(out).expect("utf8");
        assert_eq!(code, SUCCESS, "stderr={}", String::from_utf8_lossy(&err));
        for needle in [
            "1 collected job, oldest first:",
            "job job-1",
            "what the buyer paid: 100 sats",
            "mint fee: 1 sats",
            "platform fee (10%): 10 sats — unremitted",
            "you keep: 89 sats",
            "platform fee: 10 sats accrued — 0 sats remitted, 10 sats unremitted",
            "    at 10%: 10 sats on 100 sats paid, 1 job\n",
            "Remittances:\n  none yet",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }

        // A home with no store is a clear error, not a crash and not an empty ledger — for both
        // forms of the command.
        let empty = root.join("nothing-here");
        std::fs::create_dir_all(&empty).expect("empty home");
        for args in [
            vec!["--home".to_owned(), empty.display().to_string()],
            vec![
                "remit".to_owned(),
                "--home".to_owned(),
                empty.display().to_string(),
            ],
        ] {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let code = run(&args, &mut out, &mut err);
            assert_eq!(code, RUNTIME_ERROR);
            assert!(
                String::from_utf8_lossy(&err).contains("no seller store at"),
                "{}",
                String::from_utf8_lossy(&err)
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // Round 3 — the case the three partial tests left uncovered: a REAL store written at schema v8
    // (fee columns present, no `mint_fee_sats` column) holding a receipt collected under that
    // schema; `maxplayer seller fees` opens it (the additive migration runs), and prints THAT row
    // through the seller read-out with its mint fee "not recorded" and "you keep" unknown — never a
    // measured 0, never a kept figure. The store is then re-read off disk to prove the row printed
    // was the migrated one, and a row collected on the migrated store prints beside it with both
    // figures known.
    #[test]
    fn run_prints_a_genuinely_migrated_v8_row_as_not_recorded_rather_than_zero() {
        use maxplayer_core::seller_node::STATE_DB_FILE;

        let root = temp_home("v8");
        let db = root.join(STATE_DB_FILE);
        {
            // The v8 shape verbatim: `receipts` with the platform-fee columns and WITHOUT
            // `mint_fee_sats`, schema_version 8, one receipt collected at face 100 / 10% / 10 sats.
            let conn = rusqlite::Connection::open(&db).expect("create v8 store");
            conn.execute_batch(
                "CREATE TABLE seller_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO seller_meta VALUES ('schema_version', '8');
                 CREATE TABLE receipts (
                     receipt_id      TEXT PRIMARY KEY,
                     job_id          TEXT NOT NULL,
                     amount_sats     INTEGER NOT NULL CHECK (amount_sats >= 0),
                     received_at_unix INTEGER NOT NULL,
                     fee_bps         INTEGER NOT NULL DEFAULT 0 CHECK (fee_bps >= 0 AND fee_bps <= 10000),
                     fee_sats        INTEGER NOT NULL DEFAULT 0 CHECK (fee_sats >= 0)
                 );
                 INSERT INTO receipts VALUES ('v8-receipt', 'v8-job', 100, 7, 1000, 10);",
            )
            .expect("v8 schema");
            let columns: Vec<String> = conn
                .prepare("SELECT name FROM pragma_table_info('receipts')")
                .expect("pragma")
                .query_map([], |row| row.get(0))
                .expect("columns")
                .collect::<Result<_, _>>()
                .expect("column names");
            assert!(
                !columns.iter().any(|name| name == "mint_fee_sats"),
                "fixture must predate the mint fee column: {columns:?}"
            );
        }

        // Print the ledger straight off the v8 file: the open migrates, the read-out renders.
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &["--home".to_owned(), root.display().to_string()],
            &mut out,
            &mut err,
        );
        let out = String::from_utf8(out).expect("utf8");
        assert_eq!(code, SUCCESS, "stderr={}", String::from_utf8_lossy(&err));
        for needle in [
            "1 collected job, oldest first:",
            "job v8-job",
            "what the buyer paid: 100 sats",
            "mint fee: not recorded (collected before this version tracked it)",
            "platform fee (10%): 10 sats — unremitted",
            "you keep: unknown (mint fee not recorded)",
            "mint fees: 0 sats across the jobs that recorded one, plus 1 job whose mint fee was not recorded",
            "platform fee: 10 sats accrued — 0 sats remitted, 10 sats unremitted",
            "    at 10%: 10 sats on 100 sats paid, 1 job\n",
            "you keep: unknown (no job recorded its mint fee)",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }
        assert!(
            !out.contains("mint fee: 0 sats"),
            "a migrated row must not print a measured zero:\n{out}"
        );
        assert!(
            !out.contains("you keep: 90 sats") && !out.contains("you keep: 100 sats"),
            "no kept figure may be derived without the mint fee:\n{out}"
        );

        // The row that printed was the migrated one: the file now reads the current schema with the
        // `mint_fee_sats` column present and NULL on the v8 row, and `remittance_id` NULL too.
        {
            let conn = rusqlite::Connection::open(&db).expect("reopen migrated store");
            let version: String = conn
                .query_row(
                    "SELECT value FROM seller_meta WHERE key = 'schema_version'",
                    [],
                    |row| row.get(0),
                )
                .expect("schema_version");
            assert_eq!(
                version,
                maxplayer_core::seller_node::store::SCHEMA_VERSION.to_string(),
                "the open migrated the v8 file to the current schema"
            );
            let (mint_fee, remittance): (Option<i64>, Option<String>) = conn
                .query_row(
                    "SELECT mint_fee_sats, remittance_id FROM receipts WHERE receipt_id = 'v8-receipt'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("migrated columns readable");
            assert_eq!(mint_fee, None, "the v8 row's mint fee is NULL, not 0");
            assert_eq!(remittance, None, "the v8 row is unremitted");
        }

        // A collection on the migrated store records its mint fee and prints beside the old row
        // with both figures known, while the old row still says "not recorded".
        {
            let store = SellerStore::open(&db).expect("open migrated store");
            store
                .collect_receipt(
                    "v9-receipt",
                    "v9-job",
                    100,
                    ReceiptFees {
                        mint_fee_sats: 1,
                        fee_bps: 1000,
                        fee_sats: 10,
                    },
                    8,
                )
                .expect("collect on migrated store");
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &["--home".to_owned(), root.display().to_string()],
            &mut out,
            &mut err,
        );
        let out = String::from_utf8(out).expect("utf8");
        assert_eq!(code, SUCCESS, "stderr={}", String::from_utf8_lossy(&err));
        for needle in [
            "2 collected jobs, oldest first:",
            "job v8-job\n    what the buyer paid: 100 sats\n    mint fee: not recorded (collected before this version tracked it)\n    platform fee (10%): 10 sats — unremitted\n    you keep: unknown (mint fee not recorded)\n",
            "job v9-job\n    what the buyer paid: 100 sats\n    mint fee: 1 sats\n    platform fee (10%): 10 sats — unremitted\n    you keep: 89 sats\n",
            "mint fees: 1 sats across the jobs that recorded one, plus 1 job whose mint fee was not recorded",
            "platform fee: 20 sats accrued — 0 sats remitted, 20 sats unremitted",
            "    at 10%: 20 sats on 200 sats paid, 2 jobs\n",
            "you keep: 89 sats across the jobs that recorded a mint fee; the rest is unknown",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- remit: the decision logic over scripted effects ----

    /// Scripted effects. `reserve_for(amount)` is the mint's fee reserve policy; `melt_results` are
    /// consumed in order; `status` answers the reconciliation query. Every call is logged so a test
    /// can assert what was — and was not — touched.
    struct Fake {
        min_msat: u64,
        max_msat: u64,
        reserve_for: Box<dyn Fn(u64) -> u64>,
        melt_results: Vec<Result<(u64, u64), String>>,
        status: Result<Option<MeltQuoteStatus>, String>,
        pay_requests: usize,
        invoices: Vec<u64>,
        estimates: Vec<String>,
        melts: Vec<String>,
        status_calls: Vec<String>,
    }

    impl Fake {
        fn new(reserve_for: impl Fn(u64) -> u64 + 'static) -> Self {
            Self {
                min_msat: 1000,
                max_msat: 1_000_000_000,
                reserve_for: Box::new(reserve_for),
                melt_results: Vec::new(),
                status: Ok(None),
                pay_requests: 0,
                invoices: Vec::new(),
                estimates: Vec::new(),
                melts: Vec::new(),
                status_calls: Vec::new(),
            }
        }

        fn bolt11_for(amount_sats: u64, sequence: usize) -> String {
            format!("lnbc-fake-{amount_sats}-{sequence}")
        }

        fn hash_for(amount_sats: u64, sequence: usize) -> String {
            format!("hash-{amount_sats}-{sequence}")
        }
    }

    impl RemitEffects for Fake {
        fn pay_request(&mut self, address: &LightningAddress) -> Result<PayRequest, String> {
            assert_eq!(address.to_string(), "maxplayer@agi.cash");
            self.pay_requests += 1;
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

    fn store_with_fees(label: &str, fees: &[u64]) -> (SellerStore, PathBuf) {
        use maxplayer_core::seller_node::STATE_DB_FILE;
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

    fn run_remit(store: &SellerStore, fake: &mut Fake, confirm: bool, now: i64) -> (i32, String) {
        let mut out = Vec::new();
        let code = remit(store, fake, confirm, now, &mut out).expect("remit runs");
        (code, String::from_utf8(out).expect("utf8"))
    }

    // §3.2: no flag ⇒ dry run. It resolves, quotes, prints every figure, and MOVES NOTHING: no melt,
    // no journal row, the unremitted balance untouched.
    #[test]
    fn remit_without_confirm_is_a_dry_run_that_prints_the_plan_and_moves_nothing() {
        let (store, root) = store_with_fees("dry-run", &[10, 5]);
        let mut fake = Fake::new(|_| 2);
        let (code, out) = run_remit(&store, &mut fake, false, 100);
        assert_eq!(code, SUCCESS, "{out}");
        for needle in [
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
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 15);
        let _ = std::fs::remove_dir_all(&root);
    }

    // §3.2 + §3.3 + §3.4: `--confirm` pays ONCE, the fee comes out of the gross, the settlement is
    // journaled with gross / melt fee / net / destination literal / payment hash / quote id, the
    // receipts are discharged — and a second `--confirm` pays nothing.
    #[test]
    fn remit_confirm_pays_once_takes_the_fee_out_of_the_gross_and_is_idempotent() {
        let (store, root) = store_with_fees("confirm", &[10, 5]);
        let mut fake = Fake::new(|_| 2);
        fake.melt_results = vec![Ok((13, 1))];
        let (code, out) = run_remit(&store, &mut fake, true, 100);
        assert_eq!(code, SUCCESS, "{out}");
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

        // Idempotent: a second --confirm finds nothing unremitted, touches no network, pays nothing.
        let pay_requests_before = fake.pay_requests;
        let (code, out) = run_remit(&store, &mut fake, true, 101);
        assert_eq!(code, REFUSED, "{out}");
        assert!(
            out.contains("Nothing to remit. REFUSED — nothing moved."),
            "{out}"
        );
        assert_eq!(
            fake.pay_requests, pay_requests_before,
            "no LNURL round trip"
        );
        assert_eq!(fake.melts.len(), 1, "still exactly one melt, ever");
        assert_eq!(store.remittances().expect("rows").len(), 1);

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
        let (code, out) = run_remit(&store, &mut fake, true, 103);
        assert_eq!(code, SUCCESS, "{out}");
        assert!(out.contains("gross discharged: 20 sats"), "{out}");
        assert_eq!(fake.melts.len(), 2);
        assert_eq!(store.accrued_fees().expect("read").remitted_fee_sats, 35);
        let _ = std::fs::remove_dir_all(&root);
    }

    // The mint's melt fee is zero (some mints charge none): one invoice, one quote, the whole gross
    // goes to the destination.
    #[test]
    fn remit_with_a_zero_fee_reserve_invoices_the_gross_once() {
        let (store, root) = store_with_fees("zero-reserve", &[10]);
        let mut fake = Fake::new(|_| 0);
        fake.melt_results = vec![Ok((10, 0))];
        let (code, out) = run_remit(&store, &mut fake, true, 100);
        assert_eq!(code, SUCCESS, "{out}");
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

    // §3.2: below the resolved minSendable ⇒ refuse with the shortfall printed, no invoice requested,
    // nothing journaled. This is the steady state for small sellers, not an error.
    #[test]
    fn remit_refuses_below_the_resolved_minimum_with_the_shortfall_and_requests_no_invoice() {
        // Two 10-sat jobs at 10% owe 1 sat each ⇒ gross 2; the destination wants 5000 msat = 5 sats.
        let (store, root) = store_with_fees("below-min", &[1, 1]);
        let mut fake = Fake::new(|_| 0);
        fake.min_msat = 5000;
        for confirm in [false, true] {
            let (code, out) = run_remit(&store, &mut fake, confirm, 100);
            assert_eq!(code, REFUSED, "{out}");
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
        assert_eq!(store.accrued_fees().expect("read").unremitted_fee_sats, 2);

        // A minimum that is not a whole sat rounds UP: 1500 msat ⇒ 2 sats; gross 2 clears it, gross 1 does not.
        fake.min_msat = 1500;
        fake.melt_results = vec![Ok((2, 0))];
        let (code, out) = run_remit(&store, &mut fake, true, 101);
        assert_eq!(code, SUCCESS, "{out}");
        let (store1, root1) = store_with_fees("below-min-1", &[1]);
        let (code, out) = run_remit(&store1, &mut fake, true, 102);
        assert_eq!(code, REFUSED, "{out}");
        assert!(
            out.contains("below the destination's minimum of 2 sats (1 sats short)"),
            "{out}"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root1);
    }

    // §3.3: the fee reserve is taken OUT of the gross — and when what is left is below the minimum,
    // or nothing at all, the command refuses rather than paying more than the seller accrued.
    #[test]
    fn remit_refuses_when_the_fee_reserve_leaves_too_little_or_nothing() {
        // Gross 3, reserve 2 ⇒ net 1, below a 2-sat minimum.
        let (store, root) = store_with_fees("reserve-below-min", &[3]);
        let mut fake = Fake::new(|_| 2);
        fake.min_msat = 2000;
        let (code, out) = run_remit(&store, &mut fake, true, 100);
        assert_eq!(code, REFUSED, "{out}");
        assert!(
            out.contains("REFUSED — after the mint's melt fee reserve (2 sats) the 3 sats unremitted leaves 1 sats, below the destination's minimum of 2 sats (1 sats short)."),
            "{out}"
        );
        assert_eq!(fake.invoices, vec![3], "only the probe was requested");
        assert!(fake.melts.is_empty());
        assert!(store.remittances().expect("rows").is_empty());

        // Gross 2, reserve 2 ⇒ nothing left.
        let (store2, root2) = store_with_fees("reserve-eats-all", &[2]);
        let mut fake = Fake::new(|_| 2);
        let (code, out) = run_remit(&store2, &mut fake, true, 100);
        assert_eq!(code, REFUSED, "{out}");
        assert!(
            out.contains("needs a melt fee reserve of 2 sats to pay 2 sats, which leaves nothing for the destination"),
            "{out}"
        );
        assert!(fake.melts.is_empty());

        // A reserve that GROWS on the smaller invoice (non-monotone mint) so net + reserve > gross
        // is refused: the seller would pay more than it accrued.
        let (store3, root3) = store_with_fees("reserve-non-monotone", &[15]);
        let mut fake = Fake::new(|amount| if amount == 15 { 2 } else { 3 });
        let (code, out) = run_remit(&store3, &mut fake, true, 100);
        assert_eq!(code, REFUSED, "{out}");
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
    fn remit_refuses_above_the_resolved_maximum() {
        let (store, root) = store_with_fees("above-max", &[50]);
        let mut fake = Fake::new(|_| 0);
        fake.max_msat = 20_000;
        let (code, out) = run_remit(&store, &mut fake, true, 100);
        assert_eq!(code, REFUSED, "{out}");
        assert!(
            out.contains("exceeds the destination's maximum of 20 sats"),
            "{out}"
        );
        assert!(fake.invoices.is_empty() && fake.melts.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // §3.4: an interrupted remittance is RECOVERABLE, not repeatable. The melt errors after the plan
    // was journaled: the row stays planned. The next run asks the mint — PAID ⇒ settled with no second
    // melt; the receipts stay discharged.
    #[test]
    fn remit_interrupted_after_the_plan_is_reconciled_as_paid_without_a_second_melt() {
        let (store, root) = store_with_fees("interrupted-paid", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Err("connection reset during confirm".to_owned())];
        let (code, out) = run_remit(&store, &mut fake, true, 100);
        assert_eq!(code, RUNTIME_ERROR, "{out}");
        assert!(
            out.contains("melt failed: connection reset during confirm"),
            "{out}"
        );
        assert!(
            out.contains("remittance hash-9-2 stays journaled as planned"),
            "{out}"
        );
        let in_flight = store
            .in_flight_remittance()
            .expect("query")
            .expect("a planned row");
        assert_eq!(in_flight.state, RemittanceState::Planned);
        assert_eq!(in_flight.bolt11, "lnbc-fake-9-2");
        assert_eq!(store.accrued_fees().expect("read").in_flight_fee_sats, 10);
        assert_eq!(fake.melts.len(), 1);

        // Next run, the mint says PAID: settle, keep the receipts discharged, then find nothing left.
        fake.status = Ok(Some(MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: "paid-quote-lnbc-fake-9-2".to_owned(),
            state: MeltQuoteState::Paid,
            amount_sats: 9,
            fee_reserve_sats: 1,
        }));
        let (code, out) = run_remit(&store, &mut fake, true, 101);
        assert_eq!(code, REFUSED, "{out}");
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
    fn remit_reconciles_an_unpaid_or_pending_interrupted_attempt_without_paying_twice() {
        let (store, root) = store_with_fees("interrupted-unpaid", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Err("insufficient funds for melt".to_owned())];
        let (code, _) = run_remit(&store, &mut fake, true, 100);
        assert_eq!(code, RUNTIME_ERROR);

        // PENDING: refuse, keep the planned row, no melt, no new invoice.
        fake.status = Ok(Some(MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: "q-pending".to_owned(),
            state: MeltQuoteState::Pending,
            amount_sats: 9,
            fee_reserve_sats: 1,
        }));
        let invoices_before = fake.invoices.len();
        let (code, out) = run_remit(&store, &mut fake, true, 101);
        assert_eq!(code, REFUSED, "{out}");
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

        // UNPAID: fail, release, and continue into a fresh DRY RUN on a new invoice.
        fake.status = Ok(Some(MeltQuoteStatus {
            mint_url: "https://mint.example".to_owned(),
            quote_id: "q-unpaid".to_owned(),
            state: MeltQuoteState::Unpaid,
            amount_sats: 9,
            fee_reserve_sats: 1,
        }));
        let (code, out) = run_remit(&store, &mut fake, false, 102);
        assert_eq!(code, SUCCESS, "{out}");
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
        let (code, out) = run_remit(&store, &mut fake, true, 103);
        assert_eq!(code, SUCCESS, "{out}");
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
        assert_eq!(run_remit(&store2, &mut fake2, true, 100).0, RUNTIME_ERROR);
        fake2.status = Ok(None);
        let (code, out) = run_remit(&store2, &mut fake2, false, 101);
        assert_eq!(code, SUCCESS, "{out}");
        assert!(out.contains("the wallet never raised a melt quote for its invoice — no sats left the wallet; released 10 sats back to unremitted"), "{out}");
        assert_eq!(
            store2.remittances().expect("rows")[0].state,
            RemittanceState::Failed
        );
        assert_eq!(fake2.melts.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    // A status query that itself fails is a runtime error that changes nothing: the row stays
    // planned and no melt is attempted.
    #[test]
    fn remit_leaves_the_row_planned_when_reconciliation_cannot_reach_the_mint() {
        let (store, root) = store_with_fees("reconcile-error", &[10]);
        let mut fake = Fake::new(|_| 1);
        fake.melt_results = vec![Err("boom".to_owned())];
        assert_eq!(run_remit(&store, &mut fake, true, 100).0, RUNTIME_ERROR);
        fake.status = Err("mint unreachable".to_owned());
        let mut out = Vec::new();
        let error =
            remit(&store, &mut fake, true, 101, &mut out).expect_err("status failure surfaces");
        assert_eq!(error, "mint unreachable");
        assert!(store.in_flight_remittance().expect("query").is_some());
        assert_eq!(fake.melts.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    // §4 gate 1 in code: the live path builds its effects on the packaged wallet through
    // `wallet_ops::melt_blocking` — but `run remit` on a store with NOTHING unremitted returns before
    // any network or wallet call, so this exercises the real entry point offline.
    #[test]
    fn run_remit_on_an_empty_store_refuses_before_any_network_or_wallet_call() {
        use maxplayer_core::seller_node::STATE_DB_FILE;
        let root = temp_home("run-remit-empty");
        drop(SellerStore::open(root.join(STATE_DB_FILE)).expect("open store"));
        for args in [
            vec![
                "remit".to_owned(),
                "--home".to_owned(),
                root.display().to_string(),
            ],
            vec![
                "remit".to_owned(),
                "--confirm".to_owned(),
                "--home".to_owned(),
                root.display().to_string(),
            ],
        ] {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let code = run(&args, &mut out, &mut err);
            let out = String::from_utf8(out).expect("utf8");
            assert_eq!(
                code,
                REFUSED,
                "stderr={} stdout={out}",
                String::from_utf8_lossy(&err)
            );
            assert!(out.contains("Platform fee remittance — "), "{out}");
            assert!(out.contains("0 sats unremitted"), "{out}");
            assert!(
                out.contains("Nothing to remit. REFUSED — nothing moved."),
                "{out}"
            );
        }
        // `--confirm --dry-run` is a usage error, before the store is even opened.
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &[
                "remit".to_owned(),
                "--confirm".to_owned(),
                "--dry-run".to_owned(),
            ],
            &mut out,
            &mut err,
        );
        assert_eq!(code, USAGE_ERROR);
        assert!(String::from_utf8_lossy(&err).contains("contradict"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
