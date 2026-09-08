# Round 10 — step 5 body plan (body-r10.md from reports/w-remit-r9/body-r9-pin.md, 227 lines)

Written 2026-09-08 ~20:40Z while the §4 I gate runner runs (exec head 31deed8; reports head 41ea2e6).
Source of every correction: verdict at 1ee5cb2 (`/Users/forge/forge/v2/advisor/verdicts/20260908-pr979-round9-1ee5cb2.md`
§7.2 H1–H4 + "smaller issues"), addendum 11 §5, errata 1 + ruling 20:10Z. Each item below names the r9 body paragraph
(line number in body-r9-pin.md), the stale text as it stands, and the replacement. Every figure is RE-MEASURED at 31deed8
before it is written (grok-bot lesson: never copy a number an order hands me).

## A. Code-change sections (new, addendum 11 §1 / §2)
- **N1 / B** (§1 B): drop the gross-probe fee veto — 1df95d8. Reserve-only precheck at planning; `plan_confirmable_invoice`'s
  exact search alone decides `FeesDoNotFit`. Witnesses: records 45 (3/0/1000/[32] ⇒ invoice 1) and 46 (8/5/1000 ⇒ invoice 1).
  Cite the new module lines by grep at 31deed8, not from memory.
- **D / 2c** (§2): racing oracle accepts the held loser with row/owner/quote/pinned-sats assertions — 98272ba; two ordered
  tests (records 47, 48). State what the race test now asserts about the loser (SpendingHeld on the winner's bound quote,
  held_sats 15) and that no #[ignore]/rerun exists.
- **H1** (§7.2 H1): comments/docs only — 31deed8. List the sites (module doc :40–52/:96–99/:121, MeltFailure :253–258,
  LiveEffects :462–466, spend-order :1467–1486, 1a :1593, 1c :1806, belt :1980; store.rs :2273; SELLER-QUICKSTART.md
  :1132–1145; test comments :5794/:5894/:6113+:6141). One model everywhere: reserve-only precheck → optional replan ONCE →
  prepare (may GET keysets; posts nothing fee-bearing) → post-swap ACTUAL arithmetic → fence → confirm; refusal before the
  fence leaves the row Planned/unbound/pinned (next attempt's reconciliation releases it); bound = actual-vs-prepared.

## B. Corrections to r9 text carried forward (verdict §7.2)
| # | body-r9-pin.md line | stale text | replacement (re-measure first) |
|---|---|---|---|
| H2 | ¶38 | record 39 "… pool 32 → 13, delta 19 ≤ 19, `fee_paid` 4, no WARNING" | `fee_paid` **5** (Lightning 2 + actual input 3; fixture returns (13, 5) at fee_remit ~:5173, persisted Some(5)); 13 + 5 + 1 = 19. Re-read the fixture line at 31deed8. |
| H3 | ¶221 | "Their inputs are kept as record 37 (now refused before the fence) and record 30 (now invoice 13)" | Record 37 is a NEW drift fixture (reserve 3 unchanged, prepared-input override 0), not the old reserve 3→0 inputs; the old reserve-shrink schedule is a required SUCCESS in record 40 (12 → 15); record 30 restores the other case as invoice 13. Say the historical failures are preserved as failures in the round-7 verdict record, not as a relabelled fixture. |
| H4 | ¶141, ¶203 | "product files 11, **+15,634 / −293**" (×2) | re-measure `git diff --numstat a6217328..31deed8 -- '*.rs' docs/SELLER-QUICKSTART.md` — verdict measured +16,029 at 1ee5cb2; the r10 code commits change it again. Rust subtotal likewise (was +15,884). Report-inclusive total from `git diff --stat a6217328..<final> \| tail -1`. |
| census | ¶104 | "38 at 1a5c7c5 + 4 round-9 test fixtures" | round 8 had 39, r9 42 (three added); r10 adds records 45–48 — recount the `#[test]` literals per file at 31deed8 with the same past-boundary method and print the count, not the derivation. |
| same-model | moved-out rounds-5-8-sections.md:27 (permalink at 293e0e6) | "Module doc …, wallet_ops.rs type docs and docs/SELLER-QUICKSTART.md (~:1126–1153) carry the same model" | untrue at 1ee5cb2 (H1); true only at 31deed8. The moved-out file is immutable at its permalink: do NOT rewrite history — add a dated correction line in body-r10 §H1 ("the r8 same-model claim in rounds-5-8-sections.md:27 was false at 1ee5cb2; discharged at 31deed8") and, if a new moved-out commit is made, carry the corrected sentence there. |
| evidence | evidence-table.txt:58 | `exec2-runner.out 0 e3b0c4…` row with no committed blob | drop the row from the r10 evidence table, or commit the (still 0-byte) file — prefer drop + one sentence: "69 rows → 68; the zero-byte runner stdout was never committed". |
| pin URLs | body-pin-1ee5cb2…txt:13 "5 blob URLs" | inventory stale vs 17 occurrences / 13 unique targets | r10 pin record lists every URL: `grep -o 'https://github.com/[^ )>]*' body-r10.md \| sort \| uniq -c`. |
| length | body 62,505 chars | soft target 62,000 (acceptable per verdict) | aim ≤ 62,000 by moving the r9 N1–N4 detail paragraphs to a new moved-out file `rounds-9-sections.md`; hard cap 65,536. |

## C. Gates section (from step 4 logs, /tmp/w-remit-r10/logs)
- Exec head 31deed8 = last .rs commit; the runner's reports commit 41ea2e6 is line 1 of every log; line 2 states the empty
  .rs diff. Suites: core wallet nff, CLI default, CLI acp,wallet; fmt-overlap + clippy-map INSIDE/OUTSIDE at 31deed8 with the
  re-measured added set; 47 `--exact` records (44 + 4 new; #33 retired); money-path ×3 with flake names per run.
- Summary files (≤200 lines each): `gates-summary.md`, `exact-summary-31deed8.txt`. Body cites those files by permalink after
  the moved-out commit; raw logs are NOT committed (they live in /tmp) — say so, and give per-log `test result` lines + sha1
  of each log in `gates-summary.md` so the reader can ask for any one.

## D. Order of work
1. Wait for ALL DONE; write C summaries; commit + push (reports only).
2. Build body-r10.md: copy body-r9-pin.md → apply B table → add A sections → replace CI/gates/heads paragraphs → self-check
   every `file.rs:NNN` citation with a remap script (reuse reports/w-remit-r9/remap-citations-r9.py hunk method, base 1ee5cb2 → 31deed8).
3. `wc -c` < 65,536 (target ≤ 62,000); commit body + moved-out file (A'), then pin (`gh pr edit --body-file`), readback sha,
   pin record (C'), CI read at final + pin heads.
4. Done → hearth (agentId) AND maxie (sessionKey agent:maxie:discord:channel:1545487917575831684), full sha, what landed, owed.
