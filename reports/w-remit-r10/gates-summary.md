# Round 10 (addendum 11) — §4 I gates at executable head 31deed8

SKELETON — filled in when `/tmp/w-remit-r10/logs/progress.txt` reads "ALL DONE". Until then every cell below is `pending`.

Runner: `reports/w-remit-r10/run-gates-r10.sh` (committed 41ea2e6), launched 2026-09-08 ~20:37Z from the worktree as
`(zsh reports/w-remit-r10/run-gates-r10.sh > /tmp/w-remit-r10/runner.out 2>&1 < /dev/null &)`, pid 33960, never restarted.
Raw logs are NOT committed. Per the evidence-location ruling (maxie, `/Users/forge/forge/v2/maxie/runs/979-r10-evidence-location-ruling-20260908.md`,
1,048 B, sha256 `b0b39e5e…cd39599`, read 20:41Z — supersedes only the `/tmp/w-remit-r10/` path in Errata 1 and the plan ruling)
they are preserved at `/Users/forge/forge/v2/worker/reports/w-remit-r10/logs/` (seat folder, outside the product repo, uncommitted),
copied from the runner's `/tmp/w-remit-r10/logs/` with `cp -p` and verified byte-identical with `cmp` — the runner was not
restarted and no test was rerun to move a file (`EVIDENCE-LEDGER.md` there records each copy). One log per gate, `*-31deed8.log`; each log's line 1 is
`git rev-parse HEAD` at run time (the reports-only commit 41ea2e6), line 2 states `.rs diff 31deed8..HEAD` (must be empty),
line 3 the command, last line `exit=N`. This file records, per log: the result line, exit code, byte size and sha256 so any
one log can be requested and checked.

Executable head: `31deed8` — the last commit that touches a `.rs` file (fee_remit.rs, seller_node/store.rs: comments only;
plus docs/SELLER-QUICKSTART.md). Code commits this round: 1df95d8 (§1 B/N1), 5647417 (fmt), 98272ba (§2 D/2c), 31deed8 (§7.2 H1 docs).

## 1. Suites (`--no-fail-fast`)

| gate | command | result line | exit | log sha256 |
|---|---|---|---|---|
| core wallet | `cargo test -p maxplayer-core --features wallet --no-fail-fast` | lib `ok. 1431 passed; 0 failed; 2 ignored` (+ 8 integration bins all ok) | 0 | `core-wallet-nff-31deed8.log` 136,402 B `ea3b57c35a71db45…` (line 1 `41ea2e6`) |
| CLI default | `cargo test -p maxplayer --no-fail-fast` | bin `ok. 143 passed; 0 failed` (+ 4 integration bins ok) | 0 | `cli-default-31deed8.log` 16,866 B `bd19b0fbd902697d…` (line 1 `41ea2e6`) |
| CLI acp,wallet | `cargo test -p maxplayer --features acp,wallet --no-fail-fast` | bin `ok. 180 passed; 0 failed; 1 ignored` (+ 5 integration bins ok) | 0 | `cli-acp-wallet-31deed8.log` 18,585 B `8db5ff670a93337a…` (line 1 `ab9260f`) |

No failures in any suite. Line 1 of the logs moved from `41ea2e6` to `ab9260f` mid-run (reports-only commits landed while the
runner ran); line 2 of every log states `.rs diff 31deed8..HEAD` = empty, so the code under test is 31deed8 throughout.

## 2. Money-path ×3 (CI's exact command, `.github/workflows/ci.yml:233`)

`cargo test -p maxplayer-core --release --no-default-features --features gateway,git-delivery,wallet,live-mints --locked`

| run | result line | failures (names) | fee_remit tests | exit |
|---|---|---|---|---|
| 1 | `FAILED. 1434 passed; 1 failed; 2 ignored` (152.97 s) | `credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it` (known loopback flake, rounds 5–9; not a remitter test) | 44/44 `fee_remit::tests` ok | 101 — `money-path-ci-1-31deed8.log` 130,874 B `aaff9a64723ced37…` (line 1 `ab9260f`) |
| 2 | `FAILED. 1434 passed; 1 failed; 2 ignored` (145.52 s) | `credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it` (same known flake) | 44/44 `fee_remit::tests` ok | 101 — `money-path-ci-2-31deed8.log` 130,762 B `39dbbb144783360f…` (line 1 `c7ebbb2`) |
| 3 | `FAILED. 1434 passed; 1 failed; 2 ignored` (147.49 s) | `credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it` (same known flake) | 44/44 `fee_remit::tests` ok | 101 — `money-path-ci-3-31deed8.log` 130,762 B `08270d43ced3fb82…` (line 1 `a476a9b`) |

All three runs: 1434 passed, the single failure is the pre-existing `credential_proxy` loopback flake (rounds 5–9; not on our
added lines, not a remitter test); every `fee_remit::tests` case (44) passed in each run. `job_lifecycle` live-mint did not flake this time.
Runner finished 20:50:17Z ("ALL DONE"); final copy pass to the seat folder: 58 files, all cmp-identical; `SHA256SUMS.txt` there (71 rows) is the manifest.

## 3. fmt / clippy mapped onto the added set a6217328..31deed8

Added set re-measured at 31deed8 (`git diff --stat a6217328..31deed8 -- '*.rs'`): **10 `.rs` files, 176 hunks, +16,410 / −278**
(round 9 at 240240c: +15,884 / −278). Product files incl. `docs/SELLER-QUICKSTART.md`: 11, **+16,559 / −293** (numstat sum).
Maps: `reports/w-remit-r9/fmt-overlap.py` → `fmt-overlap-31deed8.txt`, `clippy-map.py` → `clippy-map-31deed8.txt` (both in the seat logs folder; both ran at HEAD `ab9260f`, .rs-identical to 31deed8).

| gate | command | total | INSIDE added lines | touched files, not added | OUTSIDE |
|---|---|---|---|---|---|
| rustfmt | `cargo fmt --all -- --check` (exit=1, `fmt-31deed8.log` 1,460,668 B `018b4d6bfedfeff2…`) → `fmt-overlap.py` | 2,087 blocks | 4 blocks overlap our hunks, 3 start on an added line; **lines we added that rustfmt would rewrite: 0** (below) | — | 2,083 |
| clippy | `cargo clippy --workspace --all-targets --features wallet` (exit=0, `clippy-31deed8.log` 77,376 B `f88d00dbe7db6f1e…`) → `clippy-map.py` | 126 | **1 — `fee_remit.rs:5642` `clippy::identity_op` (`1 + 0 + 1 + 1` in record 45's delta assert, step 1 code 1df95d8)** | (in the 125) | 125 (same pre-existing debt as r8/r9: 125 elsewhere) |

The four fmt overlaps are the same four as round 9 (`lint-mapping.md` there), re-blamed at 31deed8:
- `home.rs:1445` — our hunk ends on 1445 (blame `0ac30e51` ours); the only line rustfmt rewrites is 1448 `#[serde(default, skip_serializing_if = "BuyerReservationFloorConfig::is_default")]` → blame `ff3918e4` orveth.
- `lib.rs:47` — rustfmt re-sorts `pub mod` pairs; `-` lines: none.
- `lib.rs:55` — rewritten lines `pub mod long_poll;` / `pub mod oplog;` / `#[cfg(feature = "wallet")] pub mod buyer_fund;` → blame `db63eb96`, `b741eaf3`, `7d80595a` orveth.
- `lib.rs:68` — starts on a line we did not add; rewritten `pub mod wallet_ops;` (pre-existing module line).

**clippy INSIDE = 1 is a round-10 regression against the "0 inside" bar** (rounds 8 and 9 had 0). It is a test literal, not
money code: fixed in the next code commit (`1 + 0 + 1 + 1` → the same sum written without the `+ 0`, message unchanged in meaning),
which moves the executable head; the §4 I gates are then re-run at that head (second runner invocation, not a restart of this one).

## 4. `--exact` records (47 run: 44 carried + 45–48 new; #33 retired in round 9)

Per-record result lines (with each log's line-1 HEAD, byte size and sha256) are in `exact-summary-31deed8.txt` (same directory).
**Counts: 47 passed (`ok. 1 passed; 0 failed` each) / 0 failed.** All 47 logs carry line 1 `ab9260f` and an empty `.rs` diff to 31deed8.

New this round:
- 45 `a_3_sat_gross_with_no_reserve_at_1000_ppk_the_gross_probe_would_veto_pays_invoice_1_once` (§1 B/N1)
- 46 `an_8_sat_gross_with_a_5_sat_reserve_at_1000_ppk_pays_invoice_1_once_within_the_gross` (§1 B/N1)
- 47 `a_racer_that_starts_after_the_winners_fence_is_held_on_the_winners_bound_quote_and_the_winner_pays_once` (§2 D/2c)
- 48 `a_racer_that_starts_after_the_winner_settled_finds_nothing_unremitted_and_pays_nothing` (§2 D/2c)

## 5. Module-only runs during the round (already in /tmp/w-remit-r10)

| step | head | `fee_remit::tests` | log |
|---|---|---|---|
| 2 | 98272ba | 44 passed / 0 failed | step2-module.log |
| 3 | 31deed8 | 44 passed / 0 failed | step3-module.log |

## 6. CI at the pushed heads

`gh run list --repo MakePrisms/maxplayerai --branch feat/seller-fee-remit` read 20:44Z (run id · conclusion):

| head | run | conclusion |
|---|---|---|
| 68fa0eb (r9 pin record; code = 1ee5cb2) | 34265888505 | **failure** — job "Test the full shipped feature combo (acp + wallet)": `fee_remit::tests::two_racing_attempts_against_the_same_balance_record_exactly_one_remittance` panicked at fee_remit.rs:4640 (`unexpected outcome`) — the D/2c race oracle addendum 11 §2 orders fixed; other 6 jobs success |
| 8efa93f (r10 base, reports) | 34268570728 | success |
| b73bf1c (r10 plan) | 34272784640 | success |
| 1df95d8 (§1 B/N1) | 34273382506 | **failure** — same job: `seller_node::run::tests::an_accept_naming_another_seats_claim_never_binds_the_loser` panicked at run.rs:12659 "the loser must claim the open-pool offer" — blame `1d510786`/`b90f5ff` orveth 2026-08-09, not a remitter test, not on our added lines; other 6 jobs (incl. Money-path) success |
| 5647417 (fmt) | 34273464721 | success |
| 98272ba (§2 D/2c) | 34274840748 | success |
| 31deed8 (exec head, H1 docs) | 34275523983 | in_progress at 20:44Z — re-read before the body pin |
| 41ea2e6 / ab9260f / c7ebbb2 (reports) | — | queued/not yet listed at 20:44Z; re-read at pin time |

The 68fa0eb failure is the strongest evidence for §2: the r9 race test flaked in CI on the head the round-9 body was pinned to.
It is fixed at 98272ba (record 02 at 31deed8: ok; the two ordered tests 47/48: ok). CI must be green at the body pin AND the branch head (ruling) — re-read at step 5.
