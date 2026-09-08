# Round 9 lint mapping at executable head 240240c (whole delivery a6217328..240240c)

Logs: `reports/w-remit-r9/logs/exec2-fmt.log`, `logs/exec2-clippy.log` (line 1 of each = `git rev-parse HEAD`
= `d0be4a0`, a reports-only commit; `.rs` diff 240240c..d0be4a0 is empty, stated on line 2 of the clippy log).
Maps produced by `reports/w-remit-r9/fmt-overlap.py` → `logs/exec2-fmt-overlap.txt` and
`reports/w-remit-r9/clippy-map.py` → `logs/exec2-clippy-map.txt` (round 3 lineage, unchanged since r8).

Added set: 10 `.rs` files, 176 hunks, 15,884 added lines (`git diff --stat a6217328 HEAD -- '*.rs'`).
Method: every clippy `--> file:line` / rustfmt `Diff in file:line` classified INSIDE the added ranges or
OUTSIDE; for fmt INSIDE blocks, each line rustfmt would REMOVE/rewrite is `git blame`d.

## clippy — `cargo clippy --workspace --all-targets --features wallet` (exec2-clippy.log, last line `exit=0`)

- total 125; INSIDE added lines: **0**; in touched files on lines we did not add: 17; OUTSIDE: 108
  (pre-existing repo lint debt, addendum 9 §7). Same 125 / 0 / 17 / 108 split as round 8 at 1a5c7c5.
- crate tallies (log tail): core lib 60, core lib test 100 (54 dup), core `no_system_git` 6, `maxplayer`
  bin 9 (+8 dup in test), `maxplayer-relay-write-policy` 2 (+2 dup). No `error` lines.

### INSIDE
(none)

### touched files, not on added lines
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/home.rs:1776`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/wallet_ops.rs:1102`
  (round 8 reported this one at `:903`; both blame `d30e690c` orveth 2026-07-22 — the same pre-existing `if`,
  shifted by the round-9 insertions above it)
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:588`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:708`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:4333`
- warning: this function has too many arguments (8/7) — `crates/maxplayer-core/src/seller_node/run.rs:5376`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:5497`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:5557`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:5558`
- warning: unnecessary use of `get(&nostr_sdk::SingleLetterTag::lowercase(nostr_sdk::Alphabet::P))` — `crates/maxplayer-core/src/seller_node/run.rs:9785`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:10340`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:10369`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:11005`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:11575`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:12938`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:13094`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:13107`

## fmt — `cargo fmt --all -- --check` (exec2-fmt.log, last line `exit=1`; the repo is fmt-dirty at base a6217328)

- blocks total **2,087** (round 8: 2,106 at 1a5c7c5); blocks overlapping our hunks: 4; blocks starting
  INSIDE the added set: **3**, the same three round 8 adjudicated
  (`reports/w-remit-r8/exec2-lint-mapping.md:37–39`); the round-9 `.rs` commits added no fmt block.

### INSIDE blocks, with blame of every line rustfmt would rewrite

- `crates/maxplayer-core/src/home.rs:1445` (exec2-fmt.log:15559) — our hunk `+1442,4` ends on 1445
  (`pub platform_fee: PlatformFeeConfig,`); the only line rewritten is 1448
  `#[serde(default, skip_serializing_if = "BuyerReservationFloorConfig::is_default")]` → blame `ff3918e4` orveth.
- `crates/maxplayer-core/src/lib.rs:47` (exec2-fmt.log:17463) — our hunk `+42,6`; rustfmt only re-sorts the
  `git_transport` / `job_lifecycle` `pub mod` pairs (all `+` lines in the block, no `-`): lines rewritten: none.
- `crates/maxplayer-core/src/lib.rs:55` (exec2-fmt.log:17476) — our hunk `+53,4`; rustfmt moves
  `pub mod long_poll;` / `pub mod oplog;` / `#[cfg(feature = "wallet")] pub mod buyer_fund;` = lines 60–63 →
  blame `db63eb96`, `8547fc16`, `b741eaf3`, `7d80595a` — all orveth.
- (the fourth overlap, `lib.rs:68–96`, starts on line 68 which is NOT an added line — counted as overlap only.)

**Lines this branch added that rustfmt would rewrite: 0.**

The INSIDE blocks whose rewritten lines belong to other authors are context-window adjacency (our added lines
sit next to them); reformatting them would rewrite pre-existing lines — repo-wide lint debt, addendum 9 §7,
out of scope. Same adjudication as `reports/w-remit-r7/fmt-mine-24ba082.txt`,
`reports/w-remit-r8/step14-lint-mapping.md`, `reports/w-remit-r8/exec2-lint-mapping.md`.

## Files in the added set (`git diff --name-only a6217328 HEAD -- '*.rs'`)
- crates/maxplayer-core/src/fee_remit.rs (added 8,089)
- crates/maxplayer-core/src/home.rs (144)
- crates/maxplayer-core/src/lib.rs (14)
- crates/maxplayer-core/src/lnurl_pay.rs (1,287)
- crates/maxplayer-core/src/platform_fee.rs (66)
- crates/maxplayer-core/src/seller_node/run.rs (1,314)
- crates/maxplayer-core/src/seller_node/store.rs (2,571)
- crates/maxplayer-core/src/wallet_ops.rs (1,640)
- crates/maxplayer/src/sell.rs (5)
- crates/maxplayer/src/seller_fees.rs (754)
