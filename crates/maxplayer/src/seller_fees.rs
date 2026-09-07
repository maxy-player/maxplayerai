//! `maxplayer seller fees` — the seller-facing read-out of the platform fee journal — and
//! `maxplayer seller fees remit`, the operator's inspection and recovery path over the platform fee
//! remittance the seller node performs automatically after each collected payment.
//!
//! `seller fees` prints, per collected job and as a total, the four figures a seller needs to see
//! without doing arithmetic or reading source: what the buyer paid (the offer amount), the mint's
//! swap fee, the platform fee (rate and sats), and what the seller keeps — plus how much of the
//! platform fee has been remitted, how much is unremitted, and every remittance so far. Read-only as
//! to money: it opens `seller.sqlite` (applying the store's additive schema migration if the file
//! predates the current version), reads, prints, and exits.
//!
//! `seller fees remit` is the third of the three callers of `maxplayer_core::fee_remit::remit` (the
//! other two are the seller node's: its collect path, and the retry tick that backs a failed
//! remittance off and tries again while the node runs). It prints the recent remittance attempts and their
//! outcomes, reconciles an attempt interrupted mid-payment, resolves the platform's Lightning
//! address over LNURL-pay, takes a melt quote for the unremitted balance, and prints the plan.
//! **Without `--confirm` that is all it does** (a dry run is the default). With `--confirm` it forces
//! an attempt now — for an operator whose automatic path has been failing, or who has turned it off
//! with `[platform_fee] auto_remit = false` — paying the invoice from the seller's ecash through
//! `wallet_ops::melt_blocking`, the same gated melt `maxplayer wallet melt` uses (it honours
//! `allow_real_mints`), and recording the settlement so the same sats are never paid twice. Running
//! it again after a payment pays nothing.

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
        "Usage:\n  maxplayer seller fees [--home <dir>]\n  maxplayer seller fees remit [--home <dir>] [--dry-run | --confirm]\n\n`seller fees` prints, for every job this seat has collected payment on, what the buyer paid, the\nmint's fee, the platform fee (rate and sats) and what you keep, then the totals — the platform fee\nbroken out by the rate each job was recorded at and split into remitted / unremitted — and every\nremittance so far. Moves no sats (it only opens, reads and prints the seller store).\n\nThe seller node pays the platform fee AUTOMATICALLY: after each payment it collects, it remits the\nunremitted balance to the platform's Lightning address (fixed in the product; not configurable),\nbest-effort — a failed attempt is logged and tried again after the next payment. Set\n`[platform_fee] auto_remit = false` in config.toml to stop the automatic attempt; the fee still\naccrues and is still owed.\n\n`seller fees remit` is inspection and recovery. The default is a DRY RUN: it prints the recent\nattempts and their outcomes, resolves the address, quotes the mint's melt fee, prints the plan and\nmoves nothing. `--confirm` pays NOW (whether or not auto_remit is on), at most the unremitted total —\nthe mint's melt fee comes out of that amount, never on top. It refuses (exit 3, nothing moved) when\nnothing is unremitted, when the balance is below the destination's minimum (small balances\naccumulate until they clear it), or when an earlier attempt is still settling. Running it again\nafter a payment pays nothing: the receipts it discharged are recorded, and an interrupted attempt\nis reconciled with the mint, not repeated.\nExit 0 = dry run printed or payment made; 1 = usage; 2 = error; 3 = refused, nothing moved."
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

/// `maxplayer seller fees remit`: the operator's inspection and recovery path over the ONE remit
/// entry point in the product, [`maxplayer_core::fee_remit::remit`] — the same function the seller
/// node calls automatically after every collected payment and on its retry tick. Here it runs against the shipped effects
/// (LNURL over https, the packaged wallet at `home`, the home's default mint), as a dry run unless
/// `--confirm` was passed, and maps the outcome to an exit code so a script cannot read a refusal
/// as a payment. Note that `--confirm` pays regardless of `[platform_fee] auto_remit`: that switch
/// governs the automatic attempt only, and this command is how an operator pays when it is off.
#[cfg(feature = "wallet")]
fn remit_live(home: Option<PathBuf>, confirm: bool, out: &mut dyn Write) -> Result<i32, String> {
    use maxplayer_core::fee_remit::{LiveEffects, RemitOutcome, RemitTrigger, remit};

    let (store, root, db) = open_store(&home)?;
    let home = maxplayer_core::home::bootstrap(&root)
        .map_err(|error| format!("open home {}: {error}", root.display()))?;
    let auto_remit = home.config.platform_fee.auto_remit;
    let mut effects = LiveEffects::new(home)?;
    let now_unix = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| format!("clock: {error}"))?
            .as_secs(),
    )
    .map_err(|error| format!("clock: {error}"))?;
    let _ = writeln!(out, "Platform fee remittance — {}", db.display());
    let _ = writeln!(
        out,
        "Automatic remittance after each collected payment: {}",
        if auto_remit {
            "ON ([platform_fee] auto_remit = true, the default)"
        } else {
            "OFF ([platform_fee] auto_remit = false) — the fee still accrues and is owed; this command pays it"
        }
    );
    let trigger = if confirm {
        RemitTrigger::Command
    } else {
        RemitTrigger::DryRun
    };
    let outcome = remit(&store, &mut effects, trigger, now_unix, out)?;
    Ok(match outcome {
        RemitOutcome::DryRun | RemitOutcome::Paid { .. } => SUCCESS,
        RemitOutcome::Refused(_) => REFUSED,
        RemitOutcome::MeltFailed { .. } => RUNTIME_ERROR,
    })
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
    use maxplayer_core::seller_node::store::{
        AccruedFees, FeeRemittance, JobFeeAccrual, ReceiptFees, RemittanceState, SellerStore,
    };

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

    // §4 gate 1 in code: the live path builds its effects on the packaged wallet through
    // `wallet_ops::melt_blocking` — but `run remit` on a store with NOTHING unremitted returns before
    // any network or wallet call, so this exercises the real CLI entry point offline, including the
    // line that tells the operator whether the automatic remittance is on.
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
            assert!(
                out.contains("Automatic remittance after each collected payment: ON ([platform_fee] auto_remit = true, the default)"),
                "{out}"
            );
            assert!(out.contains("Recent attempts: none journaled yet"), "{out}");
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
