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
| core wallet | `cargo test -p maxplayer-core --features wallet --no-fail-fast` | pending | pending | pending |
| CLI default | `cargo test -p maxplayer --no-fail-fast` | pending | pending | pending |
| CLI acp,wallet | `cargo test -p maxplayer --features acp,wallet --no-fail-fast` | pending | pending | pending |

Failures, if any, named per suite with the test path and whether the same test fails at base a6217328 (flake census).

## 2. Money-path ×3 (CI's exact command, `.github/workflows/ci.yml:233`)

`cargo test -p maxplayer-core --release --no-default-features --features gateway,git-delivery,wallet,live-mints --locked`

| run | result line | failures (names) | fee_remit tests | exit |
|---|---|---|---|---|
| 1 | pending | pending | pending | pending |
| 2 | pending | pending | pending | pending |
| 3 | pending | pending | pending | pending |

Known pre-existing flakes from rounds 5–9: `credential_proxy` loopback, `job_lifecycle` live-mint (minibits) — each run's
names are read from its log, not assumed.

## 3. fmt / clippy mapped onto the added set a6217328..31deed8

Added set re-measured at 31deed8: pending (`git diff --stat a6217328..31deed8 -- '*.rs'`: files / hunks / added lines).

| gate | command | total | INSIDE added lines | touched files, not added | OUTSIDE |
|---|---|---|---|---|---|
| rustfmt | `cargo fmt --all -- --check` → `fmt-overlap.py` | pending | pending | pending | pending |
| clippy | `cargo clippy --workspace --all-targets --features wallet` → `clippy-map.py` | pending | pending | pending | pending |

INSIDE fmt blocks, if any: each rewritten line `git blame`d; ours vs pre-existing adjacency listed here.

## 4. `--exact` records (47 run: 44 carried + 45–48 new; #33 retired in round 9)

Per-record result lines are in `exact-summary-31deed8.txt` (same directory). Counts: pending passed / pending failed.

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

pending — read with `gh run list --branch feat/seller-fee-remit` after the reports push; one line per head
(1df95d8, 5647417, 98272ba, b73bf1c, 31deed8, 41ea2e6, …), Money-path job included (~14 min).
