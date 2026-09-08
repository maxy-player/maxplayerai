# Round 10 (addendum 11) — §4 I gates at executable head 08c476c (second exec head)

SKELETON — filled as `/tmp/w-remit-r10/logs-b/progress.txt` reports each gate done; complete when it reads "ALL DONE".

## Why two executable heads this round

| exec head | what it is | gates |
|---|---|---|
| `31deed8` | last code commit of steps 1–3 (1df95d8 §1 B/N1, 5647417 fmt, 98272ba §2 D/2c, 31deed8 §7.2 H1 comments/docs) | `gates-summary.md` + `exact-summary-31deed8.txt`: suites 1431/143/180 green, 47/47 exact, fmt 0 lines of ours rewritten, money-path ×3 credential_proxy flake only — **clippy INSIDE = 1**: `fee_remit.rs:5642` `clippy::identity_op` on the literal `1 + 0 + 1 + 1` in record 45's delta assert (step 1 test code) |
| `08c476c` | one commit after 31deed8: that literal becomes `3`, message "invoice 1 + Lightning 0 + ACTUAL input 1 + swap 1 = 3"; no other change (`git diff --stat 31deed8..08c476c` = 1 file, +2 −3) | this file + `exact-summary-08c476c.txt` |

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

| gate | command | result line | exit | log (bytes, sha256, line 1) |
|---|---|---|---|---|
| core wallet | `cargo test -p maxplayer-core --features wallet --no-fail-fast` | pending | pending | pending |
| CLI default | `cargo test -p maxplayer --no-fail-fast` | pending | pending | pending |
| CLI acp,wallet | `cargo test -p maxplayer --features acp,wallet --no-fail-fast` | pending | pending | pending |

## 2. Money-path ×3 (CI's exact command, `.github/workflows/ci.yml:233`)

| run | result line | failures (names) | fee_remit tests | exit / log |
|---|---|---|---|---|
| 1 | pending | pending | pending | pending |
| 2 | pending | pending | pending | pending |
| 3 | pending | pending | pending | pending |

## 3. fmt / clippy mapped onto the added set a6217328..08c476c

Added set: pending (expected identical to 31deed8's 10 files / 176 hunks / +16,410 / −278 — the fix replaces 3 lines with 3).

| gate | total | INSIDE added lines | OUTSIDE |
|---|---|---|---|
| rustfmt → `fmt-overlap.py` | pending | pending (bar: lines we added that rustfmt would rewrite = 0) | pending |
| clippy → `clippy-map.py` | pending | pending (bar: 0; 31deed8 had 1) | pending |

## 4. `--exact` records (47: 44 carried + 45–48 new; #33 retired)

`exact-summary-08c476c.txt` — pending. Bar: 47 × `ok. 1 passed; 0 failed`.

## 5. CI at the pushed heads

pending — `gh run list --repo MakePrisms/maxplayerai --branch feat/seller-fee-remit`; 31deed8, 08c476c, c94dbd4 and later
reports heads read at pin time; a failure is named with its job and test (the acp+wallet job flaked at 1df95d8 on orveth's
`an_accept_naming_another_seats_claim_never_binds_the_loser` and at 68fa0eb on the r9 race test that §2 fixed).
