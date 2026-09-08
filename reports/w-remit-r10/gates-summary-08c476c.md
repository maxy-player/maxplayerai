# Round 10 (addendum 11) — §4 I gates at executable head 08c476c (second exec head)

COMPLETE — `/tmp/w-remit-r10/logs-b/progress.txt` read `STAGE3 money-path x3 done — ALL DONE` at 21:05:40Z and the runner
exited on its own; every row below is filled from a copied, `cmp`-verified log. §5 is filled and reports a real gap: the PR
now conflicts with main, so GitHub creates no CI run at this exec head.

## Why two executable heads this round

| exec head | what it is | gates |
|---|---|---|
| `31deed8` | last code commit of steps 1–3 (1df95d8 §1 B/N1, 5647417 fmt, 98272ba §2 D/2c, 31deed8 §7.2 H1 comments/docs) | `gates-summary.md` + `exact-summary-31deed8.txt`: suites 1431/143/180 green, 47/47 exact, fmt 0 lines of ours rewritten, money-path ×3 credential_proxy flake only — **clippy INSIDE = 1**: `fee_remit.rs:5642` `clippy::identity_op` on the literal `1 + 0 + 1 + 1` in record 45's delta assert (step 1 test code) |
| `08c476c` | the only `.rs` commit after 31deed8 (five reports-only commits sit between them: 41ea2e6, ab9260f, c7ebbb2, a476a9b, 3bbd04c): that literal becomes `3`, message "invoice 1 + Lightning 0 + ACTUAL input 1 + swap 1 = 3"; no other change (`git diff --numstat 31deed8..08c476c -- '*.rs'` = 1 file, +2 −3; the same range also carries 5 reports-only files, 285 lines) | this file + `exact-summary-08c476c.txt` |

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
| 2 | `FAILED. 1434 passed; 1 failed; 2 ignored` in 145.56s | **1, the same known flake as run 1**: `credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it` — same `reqwest` `ConnectionReset` (os 54) at `credential_proxy.rs:3520` writing the body to its own loopback stub (port 62805 this run), no fee/remit code on the path | 44 named `fee_remit` tests ran, all in the 1,434 passed | exit=101 · `money-path-ci-2-08c476c.log` 130,762 B `e6d895882833bee5` · line 1 `a44c0be` |
| 3 | `FAILED. 1434 passed; 1 failed; 2 ignored` in 144.52s | **1, the same known flake a third time**: `credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it` — same `reqwest` `ConnectionReset` (os 54) at `credential_proxy.rs:3520` writing the body to its own loopback stub (port 63481 this run), no fee/remit code on the path | 44 named `fee_remit` tests ran, all in the 1,434 passed | exit=101 · `money-path-ci-3-08c476c.log` 130,762 B `5e9edf401f8b166c` · line 1 `ff6e47a` |

All three runs: 1,434 passed / 1 failed / 2 ignored, the failure the *same* `credential_proxy` test with the *same*
`ConnectionReset` panic at the same line (only the ephemeral loopback port differs: 62173 / 62805 / 63481), and 44 `fee_remit`
tests green in each. No fee, remit, or money-path test failed in any run. Runner b finished on its own at 21:05:40Z
(`STAGE3 money-path x3 done — ALL DONE`); pid 53734 was never restarted and no second runner was launched.

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

Read 21:07Z with `gh run list --repo MakePrisms/maxplayerai --branch feat/seller-fee-remit --limit 14 --json
databaseId,headSha,status,conclusion,createdAt`, plus per-head `…/commits/<sha>/check-suites`.

| head | CI run | result |
|---|---|---|
| `1df95d8` (§1 B/N1) | 34273382506 20:11:56Z | **failure** — 6/7 jobs green; *Test the full shipped feature combo (acp + wallet)* (job 102220345699) `FAILED. 1560 passed; 1 failed; 3 ignored`, the one failure `seller_node::run::tests::an_accept_naming_another_seats_claim_never_binds_the_loser` — orveth's seat-claim test, not this branch's code. The same job flaked at `68fa0eb` (run 34265888505) on the r9 race test that §2 of this round fixed. Named, not hidden. |
| `5647417` (fmt) | 34273464721 20:12:46Z | success |
| `98272ba` (§2 D/2c) | 34274840748 20:26:46Z | success |
| `31deed8` (**first exec head**) | 34275523983 20:33:47Z | **success — 7/7 jobs**, including *Money-path tests* and *Test the full shipped feature combo (acp + wallet)*: the acp+wallet flake did not recur |
| `08c476c` (**second exec head**) | — | **no run exists** |
| `c94dbd4`, `a44c0be`, `ff6e47a`, `d420245` (reports-only) | — | **no run exists** |

**Why there is no CI at 08c476c — and it is not a skip or a queue.** `.github/workflows/ci.yml` triggers on `pull_request`
with no path filter, so every push to the PR should build. GitHub creates a `pull_request` run against the *merge* commit
`refs/pull/979/merge`; when that merge cannot be computed the event produces no run at all. PR #979 went
`mergeable: CONFLICTING` when main moved to `d55ceaf` at **20:35:58Z** — 2 minutes after 31deed8's run started, and the last
green head is exactly the last one pushed before that. Every head since (08c476c and the four reports commits) has **zero
check-suites**; the only check on them is Vercel's `Authorization required to deploy`, which fails on every head of this PR
and is not CI. Actions itself is healthy — an unrelated branch (`feat/seller-memory-truncate-warn`, e8963eb) started a CI run
at 20:58:49Z.

One file conflicts: `git merge-tree --write-tree upstream/main d420245` (merge-base `a621732`) auto-merges `home.rs`,
`lib.rs`, `seller_node/run.rs` and `docs/SELLER-QUICKSTART.md` and reports exactly one
`CONFLICT (content): crates/maxplayer/src/sell.rs`.

**Consequence, stated plainly:** the second exec head `08c476c` has no CI row and cannot get one until the conflict is
resolved. Resolving it is a rebase-or-merge, which is not mine to take — it is the same call maxie owes on #969. The local
§4 I gate set above *is* the evidence at 08c476c: 3 suites green, 47/47 `--exact`, fmt 0 lines of ours, clippy INSIDE 0,
money-path ×3 with only the credential_proxy flake. The delta 31deed8 → 08c476c is one test-assert literal
(+2 / −3 in one `.rs` file), and 31deed8 is CI-green 7/7 — so the CI-green head and the gated head differ by no money code.
