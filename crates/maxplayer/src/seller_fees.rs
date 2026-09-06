//! `maxplayer seller fees` — the seller-facing read-out of the platform fee journal.
//!
//! Prints, per collected job and as a total, the four figures a seller needs to see without doing
//! arithmetic or reading source: what the buyer paid (the offer amount), the mint's swap fee, the
//! platform fee (rate and sats), and what the seller keeps. Read-only as to money: it opens
//! `seller.sqlite` (which applies the store's additive schema migration if the file predates the
//! current version), reads, prints, and exits. It moves no sats — the platform fee it shows is
//! recorded, not paid out, because no payout destination exists in the product.

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
        "Usage:\n  maxplayer seller fees [--home <dir>]\n\nPrints, for every job this seat has collected payment on, what the buyer paid, the mint's fee,\nthe platform fee (rate and sats) and what you keep, then the totals — the platform fee total is\nbroken out by the rate each job was recorded at. Moves no sats (it only opens, reads and prints\nthe seller store). The platform fee is recorded, not paid out: there is no payout destination\nyet, so nothing here is a bill due."
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
            // Round 3: the total carries its rate, as the usage text promises.
            "    at 10%: 10 sats on 100 sats paid, 1 job\n",
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
            by_job,
        };
        let text = render(&accrued, "db");
        let totals = text
            .split_once("Totals:\n")
            .map(|(_, totals)| totals)
            .expect("a totals block");
        assert!(
            totals.contains("  platform fee: 15 sats — recorded, not paid out"),
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
            "    at 10%: 10 sats on 100 sats paid, 1 job\n",
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

    // Round 3 — the case the three partial tests left uncovered: a REAL store written at schema v8
    // (fee columns present, no `mint_fee_sats` column) holding a receipt collected under that
    // schema; `maxplayer seller fees` opens it (the additive migration to v9 runs), and prints THAT
    // row through the seller read-out with its mint fee "not recorded" and "you keep" unknown —
    // never a measured 0, never a kept figure. The store is then re-read off disk at v9 to prove
    // the row printed was the migrated one, and a row collected on the migrated store prints beside
    // it with both figures known.
    #[test]
    fn run_prints_a_genuinely_migrated_v8_row_as_not_recorded_rather_than_zero() {
        use maxplayer_core::seller_node::STATE_DB_FILE;
        use maxplayer_core::seller_node::store::{ReceiptFees, SellerStore};

        let root = std::env::temp_dir().join(format!(
            "maxplayer-seller-fees-v8-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).expect("temp home");
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
            "platform fee (10%): 10 sats",
            "you keep: unknown (mint fee not recorded)",
            "mint fees: 0 sats across the jobs that recorded one, plus 1 job whose mint fee was not recorded",
            "platform fee: 10 sats — recorded, not paid out",
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

        // The row that printed was the migrated one: the file now reads schema 9 with the
        // `mint_fee_sats` column present and NULL on the v8 row.
        {
            let conn = rusqlite::Connection::open(&db).expect("reopen migrated store");
            let version: String = conn
                .query_row(
                    "SELECT value FROM seller_meta WHERE key = 'schema_version'",
                    [],
                    |row| row.get(0),
                )
                .expect("schema_version");
            assert_eq!(version, "9", "the open migrated the v8 file to v9");
            let mint_fee: Option<i64> = conn
                .query_row(
                    "SELECT mint_fee_sats FROM receipts WHERE receipt_id = 'v8-receipt'",
                    [],
                    |row| row.get(0),
                )
                .expect("migrated column readable");
            assert_eq!(mint_fee, None, "the v8 row's mint fee is NULL, not 0");
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
            "job v8-job\n    what the buyer paid: 100 sats\n    mint fee: not recorded (collected before this version tracked it)\n    platform fee (10%): 10 sats\n    you keep: unknown (mint fee not recorded)\n",
            "job v9-job\n    what the buyer paid: 100 sats\n    mint fee: 1 sats\n    platform fee (10%): 10 sats\n    you keep: 89 sats\n",
            "mint fees: 1 sats across the jobs that recorded one, plus 1 job whose mint fee was not recorded",
            "platform fee: 20 sats — recorded, not paid out",
            "    at 10%: 20 sats on 200 sats paid, 2 jobs\n",
            "you keep: 89 sats across the jobs that recorded a mint fee; the rest is unknown",
        ] {
            assert!(out.contains(needle), "missing {needle:?} in:\n{out}");
        }
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
