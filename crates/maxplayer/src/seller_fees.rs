//! `maxplayer seller fees` — the seller-facing read-out of the platform fee journal.
//!
//! Prints, per collected job and as a total, the four figures a seller needs to see without doing
//! arithmetic or reading source: what the buyer paid (the offer amount), the mint's swap fee, the
//! platform fee (rate and sats), and what the seller keeps. Read-only: it opens `seller.sqlite`,
//! reads, prints, and exits. It moves no sats — the platform fee it shows is recorded, not paid out,
//! because no payout destination exists in the product.

use std::io::Write;
use std::path::PathBuf;

const SUCCESS: i32 = 0;
const USAGE_ERROR: i32 = 1;
const RUNTIME_ERROR: i32 = 2;

/// Entry from `sell::run` for `maxplayer seller fees ...`.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if crate::cli::is_help_request(args) {
        usage(out);
        return SUCCESS;
    }
    let home = match parse_home(args) {
        Ok(home) => home,
        Err(message) => {
            let _ = writeln!(err, "{message}");
            usage(err);
            return USAGE_ERROR;
        }
    };

    #[cfg(not(feature = "wallet"))]
    {
        let _ = (home, out);
        let _ = writeln!(
            err,
            "maxplayer seller fees requires the wallet feature (rebuild with default features)"
        );
        USAGE_ERROR
    }

    #[cfg(feature = "wallet")]
    {
        match print_ledger(home, out) {
            Ok(()) => SUCCESS,
            Err(message) => {
                let _ = writeln!(err, "{message}");
                RUNTIME_ERROR
            }
        }
    }
}

fn parse_home(args: &[String]) -> Result<Option<PathBuf>, String> {
    let mut home = None;
    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--home" => {
                idx += 1;
                home = Some(PathBuf::from(
                    args.get(idx).ok_or("--home requires a value")?,
                ));
            }
            other => return Err(format!("unknown seller fees option: {other}")),
        }
        idx += 1;
    }
    Ok(home)
}

fn usage(w: &mut dyn Write) {
    let _ = writeln!(
        w,
        "Usage:\n  maxplayer seller fees [--home <dir>]\n\nPrints, for every job this seat has collected payment on, what the buyer paid, the mint's fee,\nthe platform fee (rate and sats) and what you keep, then the totals. Read-only. The platform fee\nis recorded, not paid out: there is no payout destination yet, so nothing here is a bill due."
    );
}

#[cfg(feature = "wallet")]
fn print_ledger(home: Option<PathBuf>, out: &mut dyn Write) -> Result<(), String> {
    use maxplayer_core::seller_node::STATE_DB_FILE;
    use maxplayer_core::seller_node::store::SellerStore;

    let root = match home {
        Some(path) => path,
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
    let accrued = store
        .accrued_fees()
        .map_err(|error| format!("read receipts: {error}"))?;
    let _ = write!(out, "{}", render(&accrued, &db.display().to_string()));
    Ok(())
}

/// The ledger as text. Pure, so a test can assert on the exact words a seller reads.
#[cfg(feature = "wallet")]
pub(crate) fn render(
    accrued: &maxplayer_core::seller_node::store::AccruedFees,
    db: &str,
) -> String {
    use maxplayer_core::platform_fee::bps_to_percent_label;

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
        text.push_str(&format!(
            "  job {}\n    what the buyer paid: {} sats\n    mint fee: {}\n    platform fee ({}): {} sats\n    you keep: {}\n",
            row.job_id,
            row.amount_sats,
            mint_fee,
            bps_to_percent_label(row.fee_bps),
            row.fee_sats,
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
    text.push_str(&format!(
        "  platform fee: {} sats — recorded, not paid out (no payout destination exists yet)\n",
        accrued.total_fee_sats
    ));
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
    text
}

#[cfg(all(test, feature = "wallet"))]
mod tests {
    use super::*;
    use maxplayer_core::seller_node::store::{AccruedFees, JobFeeAccrual};

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
            by_job: vec![row("job-a", 100, Some(1), 1000, 10)],
        };
        let text = render(&accrued, "/tmp/x/seller.sqlite");
        for needle in [
            "what the buyer paid: 100 sats",
            "mint fee: 1 sats",
            "platform fee (10%): 10 sats",
            "you keep: 89 sats",
            "recorded, not paid out",
        ] {
            assert!(text.contains(needle), "missing {needle:?} in:\n{text}");
        }
        assert!(
            !text.contains("you keep: 90"),
            "10% of the face (10), not of the net (9), is the platform fee:\n{text}"
        );
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
            by_job: vec![
                row("old-job", 21, None, 0, 0),
                row("new-job", 100, Some(1), 1000, 10),
            ],
        };
        let text = render(&accrued, "db");
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
    }

    #[test]
    fn render_with_no_rows_says_so() {
        let text = render(&AccruedFees::default(), "db");
        assert!(text.contains("No collected payments yet."), "{text}");
    }

    // End to end through `run`: a real seller store at `--home`, one collected job (face 100, mint
    // fee 1, platform fee 10% = 10), printed with the four labels and `you keep: 89 sats`.
    #[test]
    fn run_reads_a_real_store_at_home_and_prints_the_ledger() {
        use maxplayer_core::seller_node::STATE_DB_FILE;
        use maxplayer_core::seller_node::store::{ReceiptFees, SellerStore};

        let root = std::env::temp_dir().join(format!(
            "maxplayer-seller-fees-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp home");
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
            "platform fee (10%): 10 sats",
            "you keep: 89 sats",
            "platform fee: 10 sats — recorded, not paid out",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }

        // A home with no store is a clear error, not a crash and not an empty ledger.
        let empty = root.join("nothing-here");
        std::fs::create_dir_all(&empty).expect("empty home");
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = run(
            &["--home".to_owned(), empty.display().to_string()],
            &mut out,
            &mut err,
        );
        assert_eq!(code, RUNTIME_ERROR);
        assert!(
            String::from_utf8_lossy(&err).contains("no seller store at"),
            "{}",
            String::from_utf8_lossy(&err)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_home_accepts_only_home() {
        assert_eq!(parse_home(&[]).expect("empty"), None);
        assert_eq!(
            parse_home(&["--home".to_owned(), "/x".to_owned()]).expect("home"),
            Some(PathBuf::from("/x"))
        );
        assert!(parse_home(&["--home".to_owned()]).is_err());
        assert!(parse_home(&["--rate-sats".to_owned()]).is_err());
    }
}
