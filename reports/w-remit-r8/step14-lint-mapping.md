# Lint mapping after the step-14 fix (tree = b1c702a + fee_remit.rs lint fix, about to be committed)

Method (same as reports/w-remit-r7/fmt-map.py / clippy-mine): `git diff a6217328 -U0` → added ranges per
file (step14-added-ranges.diff); every `--> file:line` (clippy) / `Diff in file:line` (rustfmt) classified
INSIDE the added set or OUTSIDE.

## clippy — `cargo clippy --workspace --all-targets --features wallet` (step14-clippy.log, exit 0)
- total 125, INSIDE 0 (the one round-8 INSIDE warning, the constant `assert!(32 - 14 <= 20)` at
  fee_remit.rs:4562, is replaced by a measured `delta <= 20`).

## fmt — `cargo fmt --all -- --check` (step14-fmt.log, exit 1: repo-wide debt at base)
- total 2106 blocks, INSIDE 3 — all three are the SAME context-window overlaps round 7 adjudicated in
  fmt-mine-24ba082.txt (0 hunks reformat a line this branch added):
  - home.rs:1445 — reformats line 1448 `#[serde(default, skip_serializing_if = "BuyerReservationFloorConfig::is_default")]`,
    `git blame` author orveth; our `platform_fee` field sits above it (context only).
  - lib.rs:47 — pure reorder target of the :55 hunk; no removed lines; lib.rs:47 is our `pub mod fee_remit;`,
    :48 gudnuf, :50–51 orveth.
  - lib.rs:55 — removes/reorders lines 60–63 `pub mod long_poll; pub mod oplog; #[cfg(feature = "wallet")] pub mod buyer_fund;`,
    `git blame` author orveth on 58, 61–64; our lines 55–56 (`#[cfg(feature = "wallet")] pub mod lnurl_pay;`) are context.
  Reformatting them would rewrite other authors' pre-existing lines: repo-wide lint debt, addendum 9 §7
  out of scope. The 9 round-8 blocks in fee_remit.rs (1774, 2304, 4473, 4516, 4550, 4558, 4646, 4707, 4761)
  are applied by hand, exactly as rustfmt proposed, no whole-file rustfmt.
