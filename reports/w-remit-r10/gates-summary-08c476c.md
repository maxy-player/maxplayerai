# Round 10 (addendum 11) — §4 I gates at executable head 08c476c (second exec head)

SKELETON — filled as `/tmp/w-remit-r10/logs-b/progress.txt` reports each gate done; complete when it reads "ALL DONE".

## Why two executable heads this round

| exec head | what it is | gates |
|---|---|---|
| `31deed8` | last code commit of steps 1–3 (1df95d8 §1 B/N1, 5647417 fmt, 98272ba §2 D/2c, 31deed8 §7.2 H1 comments/docs) | `gates-summary.md` + `exact-summary-31deed8.txt`: suites 1431/143/180 green, 47/47 exact, fmt 0 lines of ours rewritten, money-path ×3 credential_proxy flake only — **clippy INSIDE = 1**: `fee_remit.rs:5642` `clippy::identity_op` on the literal `1 + 0 + 1 + 1` in record 45's delta assert (step 1 test code) |
| `08c476c` | one commit after 31deed8: that literal becomes `3`, message "invoice 1 + Lightning 0 + ACTUAL input 1 + swap 1 = 3"; no other change (`git diff --numstat 31deed8..08c476c -- '*.rs'` = 1 file, +2 −3; the same range also carries 5 reports-only files, 285 lines) | this file + `exact-summary-08c476c.txt` |

The fix touches a test assertion literal only — no money code, no expectation change (record 45 still asserts delta = 3 = the
gross). It moves the executable head because a `.rs` byte changed, so the full §4 I gate set is re-run at 08c476c by a second
runner invocation (`run-gates-r10b.sh 08c476c`, committed c94dbd4, launched 20:52:19Z, pid 53734) — the 31deed8 runner was
never restarted (it completed 20:50:17Z).

Raw logs: `/Users/forge/forge/v2/worker/reports/w-remit-r10/logs-b/` (seat folder, outside the product repo, uncommitted per the
evidence-location ruling sha `b0b39e5e…cd39599`), copied from the runner's `/tmp/w-remit-r10/logs-b/` with `cp -p` after each
gate's done line and verified with `cmp`; manifest `SHA256SUMS-b.txt`; each copy logged in `EVIDENCE-LEDGER.md`. Each log:
line 1 `git rev-parse HEAD` at run time (c94dbd4 or a later reports-only commit), line 2 `.rs diff 08c476c..HEAD` (must be
empty), line 3 the command, last line `exit=N`.

## 1. Suites (`--no-fail-fast`)

| gate | command | result line (largest binary; all binaries) | exit | log (bytes, sha256, line 1) |
|---|---|---|---|---|
| core wallet | `cargo test -p maxplayer-core --features wallet --no-fail-fast` | lib `ok. 1431 passed; 0 failed`; 10 result lines, 1,445 passed, **0 failed** | 0 | `core-wallet-nff-08c476c.log` 136,402 B `4bb361b15f78cb03` · line 1 `c94dbd4` |
| CLI default | `cargo test -p maxplayer --no-fail-fast` | lib `ok. 143 passed; 0 failed`; 5 result lines, 157 passed, **0 failed** | 0 | `cli-default-08c476c.log` 16,866 B `e6148fe9e3785005` · line 1 `a44c0be` |
| CLI acp,wallet | `cargo test -p maxplayer --features acp,wallet --no-fail-fast` | lib `ok. 180 passed; 0 failed`; 6 result lines, 199 passed, **0 failed** | 0 | `cli-acp-wallet-08c476c.log` 18,585 B `e615448676dc5931` · line 1 `a44c0be` |

Same totals as 31deed8 (1431 / 143 / 180). Line 2 of each log: `exec head 08c476c; .rs diff to HEAD:` — empty, so every
reports-only commit the runner saw is `.rs`-identical to 08c476c.

## 2. Money-path ×3 (CI's exact command, `.github/workflows/ci.yml:233`)

| run | result line | failures (names) | fee_remit tests | exit / log |
|---|---|---|---|---|
| 1 | `FAILED. 1434 passed; 1 failed; 2 ignored` in 142.25s | **1, and it is the known flake**: `credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it` — `reqwest` `ConnectionReset` (os 54) writing the body to its own loopback stub at `credential_proxy.rs:3520`, no fee/remit code on the path | 44 named `fee_remit` tests ran, all in the 1,434 passed | exit=101 · `money-path-ci-1-08c476c.log` 130,874 B `2c80da572f6daaf3` · line 1 `a44c0be` |
| 2 | pending | pending | pending | pending |
| 3 | pending | pending | pending | pending |

## 3. fmt / clippy mapped onto the added set a6217328..08c476c

Added set re-measured at 08c476c by me (`git diff --stat a6217328..08c476c -- '*.rs'`, hunks via `git diff -U0 … | grep -c '^@@'`):
**10 `.rs` files, 176 hunks, +16,409 / −278** — one added line fewer than 31deed8 (+16,410), which is exactly the fix
(`git diff --numstat 31deed8..08c476c -- '*.rs'` = +2 / −3). Product files incl. `docs/SELLER-QUICKSTART.md`: 11, +16,558 / −293.

| gate | command | total | INSIDE added lines | OUTSIDE |
|---|---|---|---|---|
| rustfmt | `cargo fmt --all -- --check` (exit=1, `fmt-08c476c.log` 1,460,668 B `113fc75998aac188`) → `fmt-overlap.py` → `fmt-overlap-08c476c.txt` | 2,087 blocks | 4 blocks overlap our hunks, 3 start on an added line; **lines we added that rustfmt would rewrite: 0** | 2,083 |
| clippy | `cargo clippy --workspace --all-targets --features wallet` (exit=0, `clippy-08c476c.log` 76,998 B `dc03c3c579b6f92c`) → `clippy-map.py` → `clippy-map-08c476c.txt` | 125 | **0 — the bar is met; 31deed8's single `fee_remit.rs:5642` `identity_op` is gone** | 125 |

The clippy count fell 126 → 125 and INSIDE 1 → 0: exactly the one diagnostic the fix targeted, nothing else moved.

The fmt file is byte-for-byte the same map as 31deed8 apart from the two numbers the fix changes
(`diff` of the two `fmt-overlap` files minus their header line: 2 lines differ — `fee_remit.rs added=8614→8613` and the
total `16410→16409`). So the four overlaps are the same four, with the same blame, already itemised in `gates-summary.md` §3:
`home.rs:1445` (rewritten line is orveth's `ff3918e4`), `lib.rs:47` / `:55` / `:68` (rewritten lines are pre-existing `pub mod`
lines, blame `db63eb96` / `b741eaf3` / `7d80595a` orveth). No line this branch added is rewritten by rustfmt.

## 4. `--exact` records (47: 44 carried + 45–48 new; #33 retired)

`exact-summary-08c476c.txt` (same directory, 51 lines): per-record log line 1, bytes, sha256, result line.
**Counts: 47 passed (`ok. 1 passed; 0 failed` each) / 0 failed / 0 not-1-passed.** All 47 logs carry line 2
`exec head 08c476c; .rs diff to HEAD:` with an empty diff (checked by me on all 47, not sampled). Records 45–48 are the
round-10 additions (§1 B/N1 ×2, §2 D/2c ×2); #33 stays retired from round 9.

Record 45 is the one whose assert literal the fix rewrote (`1 + 0 + 1 + 1` → `3`): it still passes and still asserts the same
delta of 3 sats.

## 5. CI at the pushed heads

pending — `gh run list --repo MakePrisms/maxplayerai --branch feat/seller-fee-remit`; 31deed8, 08c476c, c94dbd4 and later
reports heads read at pin time; a failure is named with its job and test (the acp+wallet job flaked at 1df95d8 on orveth's
`an_accept_naming_another_seats_claim_never_binds_the_loser` and at 68fa0eb on the r9 race test that §2 fixed).
