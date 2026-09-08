# Round 8 — PR #979 body plan (step 24, 2026-09-08)

## 0. State read first-hand
- Live body: `gh pr view 979 --json body -q .body` → `/tmp/pr979-body-live.md`, 65,309 B,
  sha256 `01236a65522d26e915d9150e7d9528b4f04a693e68a8d3211980f32c5be28bb2` = the round-7 readback
  pin (`reports/w-remit-r7/body-pin-da0ee92.txt`). Local source `body-da0ee92.md` 65,308 B,
  sha256 `c03e3b319c4abe0a4159a433bb69065bf806739ed5451fc0f088e84608e568b0`; the one-byte
  difference is GitHub's trailing newline. Nobody edited the body since round 7.
- GitHub cap 65,536 B: the r7 body sits 228 B under it. Round 8 adds a section, so the round-5/6/7
  narrative must be condensed further (as r7 part 3 did) — names, lines and results stay, narration goes.
- Executable head `1a5c7c5` (last `.rs` commit); reports head `036302b`; final head = the body-pin
  commit made in step 27. `git diff --stat 1a5c7c5..final` must show only `docs/`, `reports/`
  (already: 036302b is reports-only; the pin commit is reports-only).

## 1. Edits ordered by the verdict / addendum 9
### §4.2 — body line 196 (`## CI at 24ba082` paragraph)
Strike the sentence that final-head CI is not required. Replace with: CI at the round-7 heads
(`24ba082` run 34200030811 success; `da0ee92` — read and state its run) AND CI at the round-8
heads (`1a5c7c5`, `036302b`, final) — read each with `gh run list --commit <sha>` in step 27; the
final head's run must be green before DONE, or DONE names it as pending/failed.

### §4.3 — body line 10 (heads paragraph, "Why the executable head moved once")
Withdraw "green" for the `2d802a3` money-path trio; say: at `2d802a3` the core, CLI and money-path
suites were run and the money-path trio FAILED 2/3/2 on the three out-of-diff flakes (§191 stays as is).

### §4.4 — every fee/model/output claim restated against 1a5c7c5
Body lines to rewrite (r7 numbering): 12 (§5 bullet "The money hold": gross ceiling now = invoice +
reserve + ACTUAL proof input fee on the post-swap split + swap fee, proven confirmable before the
fence); 15 (Round-7 B4 fix paragraph — spend order now prepare → `confirm_would_succeed` → fence);
56 (B4 defect: keep, add that round 7's fake still admitted SDK-impossible successes); 58 (fix:
add step 1c); 60 (fake fidelity: `layout` is gone, replaced by CDK-shaped `confirm_melt`: swap to
target split, swap fee charged even on later failure, actual input fee recomputed, refusal after
swap, `fee_paid` = proofs − invoice − change); 62 (regressions (i)/(ii)/(iii) renamed/re-numbered
— names below); 249 (disclosure "fake's fee model … exact-fit branch" → the round-8 model and
its bound); §5 PAID-print claims (fee counted once, "actual debit" line, "unknown" balance).
Every `fee_remit.rs:NNNN` / `wallet_ops.rs:NNNN` in those paragraphs remapped (method §3).

### New section "## Round 8 — what discharges B / G / H (verdict at `da0ee92`, addendum 9)"
Placed after the Round 7 section; four blocks F1–F4 each: defect (advisor's words, one line) →
change (commit) → test name(s) with `--exact` record number → what stays undone (owed):
- F1 (B): `confirm_would_succeed` before the fence, `plan_confirmable_invoice`, CDK-shaped fake,
  §1.5 bound disclosed. Tests: self-check (record 27), record 28
  `a_fee_bearing_payment_whose_prepared_input_fee_differs_from_the_actual_pays_once_and_the_wallet_loses_at_most_the_gross`,
  record 37 `a_fee_bearing_schedule_whose_prepared_figures_fit_but_post_swap_arithmetic_does_not_is_refused_before_the_fence`,
  planner test now invoice 13 (record for `fee_aware_planning_…`), (iii) kept: fee-exceeds, never-fits, 2f.
- F2 (G): `spent = paid + fee_paid + swap_fee`; labels; `balance_after_sats: Option`; record 38
  `a_failed_balance_read_after_a_fee_bearing_payment_prints_unknown_and_no_false_warning`.
- F3 (H): cancel = best-effort local compensation; no reopen-recovers claim anywhere; recovery path OWED,
  `recover_incomplete_sagas` NOT wired (by instruction); prepare may GET metadata/keysets.
- F4 (H): `lnurl_pay.rs:27`, `seller_fees.rs:20/1086`, `wallet_ops.rs` — live edge named; §196 and
  §10 corrected as above.
- CDK citations (pinned 0.17.2, `melt/saga/mod.rs`): estimate `:383–399`, swap_fee `:403`,
  `from_prepared` swap_fee 0 `:574–595`, target `:678`, recompute + refuse `:704–712`, `fee_paid` `:136–148`;
  `swap/saga/mod.rs:285–301`; `fees.rs:35–48`.

### New evidence sections at `1a5c7c5` (replace the r7 "Full suites / Money-path / fmt-clippy / Evidence files" blocks)
- Full suites: core `--features wallet` 1422 pass / 1 fail (credential_proxy flake) / 2 ign + integration
  green (`exec2-core-wallet-nff.log`, `--no-fail-fast`); CLI default 143+2+3+3+6 (`exec2-cli-default.log`);
  CLI acp,wallet 180 (1 ign)+2+3+5+3+6 (`exec2-cli-acp-wallet.log`).
- Money-path ×3 (`money-path-ci-{1,2,3}-1a5c7c5.log`): 1427/0/2, 1424/3/2, 1424/3/2 — FAILED names listed,
  never "green".
- fmt/clippy mapped to `a6217328..1a5c7c5` (`exec2-fmt.log`, `exec2-clippy.log`, `exec2-lint-mapping.md`):
  clippy 125 elsewhere / 0 inside; fmt 0 of our added lines (3 context-adjacent hits rewrite base lines
  — adjudicated, not reformatted); denominators re-measured.
- Exact records 38/38 (`exact-records.txt`, `exact-NN-tag-1a5c7c5.log`), 36 kept + 2 new (28, 37, 38
  numbering per file — state exactly).
- Evidence table: `evidence-table.txt` (71 rows, bytes + sha256), committed in `036302b`.
- Heads paragraph: executable `1a5c7c5`, reports `036302b`, final `<pin commit>`, `git diff --stat 1a5c7c5..final`.
- Commits this round: 4d5a5f3, a7c5968, 1236efc, cd0322b, 8baaf3b, 060ebb7, b1c702a, 1a5c7c5, 036302b (+ pin).

### Prior-finding ledger (append rows)
| finding | round | status at 1a5c7c5 |
| B/F1 fake admits SDK-impossible success | 8 | closed — records 27, 28, 37 |
| G/F2 double-counted inclusive fee_paid | 8 | closed — record 38 + PAID print |
| H/F3 false recovery/cancel prose | 8 | closed (prose); recovery path OWED |
| H/F4 stale current-caller prose, §196, §10 | 8 | closed |

### Disclosures / out of scope
Keep r7 disclosures (name fix, moved head, pay_melt retained, flakes, no rebase); replace the fake-model
disclosure; add §1.5 bound; add "executable head moved once in round 8 (b1c702a → 1a5c7c5, lint only)";
out-of-scope list = addendum 9 §7 verbatim (incl. `recover_incomplete_sagas` wiring, force-release).

## 2. Build method (step 25)
Script `reports/w-remit-r8/build-body-r8.py` adapted from `build-body-r7*.py`: input `body-da0ee92.md`,
`once()` anchors (each must match exactly once), section replacement by heading find, output
`body-r8.md`; print byte size; fail if > 65,000 B (margin for the trailing newline).

## 3. Citation remap (step 26)
For each `file:line:snippet` in `reports/w-remit-r7/body-citation-inventory.txt`: `grep -nF` the snippet
in the file at `1a5c7c5`; exactly one hit → new line; zero hits → the line was rewritten this round,
re-cite by hand from the new code (record old→new in `body-citation-inventory-r8.txt`, header
`citations at 1a5c7c5 (rs) / <final> (docs)`). Then apply the map to every `file.rs:NNNN` token in
`body-r8.md` with a script that refuses unknown tokens. Self-check: re-grep every load-bearing citation
(§5 bullets, Round 7/8 sections) from the finished body.

## 4. Pin (step 27)
`gh pr edit 979 --body-file body-r8.md`; read back; sha256 of readback → `body-pin-<final>.txt` with both
the local and readback hashes; commit reports (`git -c user.name=… -c user.email=… commit -F`), push,
post `pushed <sha>`; `gh run list --commit <final>` until conclusion; DONE via sessions_send to hearth
and `agent:maxie:discord:channel:1545487917575831684`.
