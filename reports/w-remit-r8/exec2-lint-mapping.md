# Final lint mapping at executable head 1a5c7c5 (whole delivery a6217328..1a5c7c5)

Added set: 11 files, 160 hunks (exec2-added-ranges.diff). Method: every clippy `--> file:line` / rustfmt `Diff in file:line` classified INSIDE the added ranges or OUTSIDE; for fmt INSIDE blocks, each line rustfmt would REMOVE/rewrite is git-blamed.

## clippy — `cargo clippy --workspace --all-targets --features wallet` (exec2-clippy.log, last line `exit=0`)

- total 125; INSIDE added lines: 0; in touched files on lines we did not add: 17; OUTSIDE: 108 (pre-existing repo lint debt, addendum 9 §7).

### INSIDE
(none)

### touched files, not on added lines
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/home.rs:1776`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/wallet_ops.rs:903`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:588`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:708`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:4333`
- warning: this function has too many arguments (8/7) — `crates/maxplayer-core/src/seller_node/run.rs:5376`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:5497`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:5557`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:5558`
- warning: unnecessary use of `get(&nostr_sdk::SingleLetterTag::lowercase(nostr_sdk::Alphabet::P)) — `crates/maxplayer-core/src/seller_node/run.rs:9785`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:10340`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:10369`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:11005`
- warning: this `if` statement can be collapsed — `crates/maxplayer-core/src/seller_node/run.rs:11575`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:12938`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:13094`
- warning: this expression creates a reference which is immediately dereferenced by the compiler — `crates/maxplayer-core/src/seller_node/run.rs:13107`

## fmt — `cargo fmt --all -- --check` (exec2-fmt.log, last line `exit=1`; the repo is fmt-dirty at base a6217328)

- blocks total 2106; blocks starting INSIDE the added set: 3.

### INSIDE blocks, with blame of every line rustfmt would rewrite

- `crates/maxplayer-core/src/home.rs:1445` — lines rewritten: 1448 (orveth)
- `crates/maxplayer-core/src/lib.rs:47` — lines rewritten: none (pure insertion/reorder target)
- `crates/maxplayer-core/src/lib.rs:55` — lines rewritten: 60 (orveth), 61 (orveth), 62 (orveth), 63 (orveth)

**Lines this branch added that rustfmt would rewrite: 0**

The INSIDE blocks whose rewritten lines belong to other authors are context-window adjacency (our added lines sit next to them); reformatting them would rewrite pre-existing lines — repo-wide lint debt, addendum 9 §7 out of scope. Same adjudication as reports/w-remit-r7/fmt-mine-24ba082.txt and reports/w-remit-r8/step14-lint-mapping.md.

## Files in the added set
- crates/maxplayer-core/src/fee_remit.rs
- crates/maxplayer-core/src/home.rs
- crates/maxplayer-core/src/lib.rs
- crates/maxplayer-core/src/lnurl_pay.rs
- crates/maxplayer-core/src/platform_fee.rs
- crates/maxplayer-core/src/seller_node/run.rs
- crates/maxplayer-core/src/seller_node/store.rs
- crates/maxplayer-core/src/wallet_ops.rs
- crates/maxplayer/src/sell.rs
- crates/maxplayer/src/seller_fees.rs
- docs/SELLER-QUICKSTART.md
