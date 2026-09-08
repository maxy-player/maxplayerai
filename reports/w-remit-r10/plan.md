# Round 10 plan — PR #979, addendum 11 (2026-09-08T19:45Z), base 8efa93f

## Read-back (computed by me, 2026-09-08T20:04Z)
- addendum11 `/Users/forge/forge/v2/briefs/seller-fee-stage2a-addendum11-20260908T1945Z.md`: 7,617 B, 83 lines, mode 444,
  sha256 489fc24aa7034c2fee7b7592fbf31535867a13de39b3e282080e9c263f5f5e40
- verdict `/Users/forge/forge/v2/advisor/verdicts/20260908-pr979-round9-1ee5cb2.md`: 88,132 B, mode 444,
  sha256 f6d6a1922f393e67df67649847cf9bc29470db0d1a1f8fb140c1917cc12ef4ee; frozen copy same sha, `cmp` byte-identical
- worktree HEAD 8efa93fca19b4266738f880a1ca34729d578d092 = origin/feat/seller-fee-remit; tree 7ca8101d7b94659f8b3ff8fb78ebe7c767f05604
- porcelain: only untracked reports/w-remit-r4..r7/ and reports/w-remit-r9/logs/exec2-runner.out (as in every round)
- verdict sections read whole: §4.2, §4.5, §5.2, §7.1, §7.2, §8, §10

## Steps (append-only on 8efa93f; identity w-seller-fee-stage2a-remit <worker@forge.local>)
1. §1 B/N1: replace the gross-probe `reserve + expected >= gross` veto (fee_remit.rs:1235 at 240240c) so the outer planner
   reaches `plan_confirmable_invoice` (or an exact search) before any fee refusal; `FeesDoNotFit` only when NO candidate fits.
   Keep the reserve-only veto (`reserve >= gross`) since the helper needs gross - reserve > 0.
   New FULL `remit` success regression: gross 3 / reserve 0 / 1000 ppk / [32] → invoice 1, one swap, one melt, pool 32→29,
   whole-pool debit 3 ≤ 3, row Settled (record 45). Second witness gross 8 / reserve 5 as record 46 if cheap.
   Fix the `:1225–1226` comment so it is true.
2. §2 D/2c: racing oracle accepts `Refused(SpendingHeld{..})` for the loser WITH assertions: remittance_id == winner row id,
   quote_id == winner's bound quote, held_sats == 15, out contains the HELD line shape with pinned receipts; keep one-melt /
   one-Paid / one Settled row / accounting (15,0,0). Add deterministic ordered tests: loser starts after the fence
   (winner bound) → SpendingHeld; loser after winner Settled → NothingUnremitted/PlanRefused. No #[ignore], no rerun.
3. §3 H1 source docs: fee_remit.rs:40–45, :89, :112, :1457–1465, :1783–1784, :1957–1959, quickstart :1133–1141 → one model
   (reserve-only precheck → optional replan → prepare (may fetch metadata; posts nothing fee-bearing) → post-swap actual
   arithmetic → fence → confirm; refusal leaves Planned + receipts pinned; bound is actual-vs-prepared).
   Also fee_remit.rs:1572 / store.rs:2273 "smaller invoice" → "new/replanned invoice". Optionally fix the 3 stale test
   comments (:5280/:5381/:5617) and the phrase-count assertions (:3614/:5337/:5430) — allowed now; say so.
4. §4 I: last .rs commit = exec head; all suites, money-path ×3, fmt/clippy mapped to a6217328..head added set, all --exact
   records (44 + new) with git state lines 1–2; logs under reports/w-remit-r10/logs/.
5. §5 body: body-r10.md from body-r9-pin.md: Round 10 section (N1/2c/H1–H4 discharge), H2 fee_paid 5, H3 record 37 vs 40,
   H4 +16,029 subtotal, §104 census 39/42, evidence table 68 rows, URL inventory 13/17, CI table with run ids, known-defects
   honest; permalinks at final head in reports/w-remit-r10/; ≤65,536 chars (62,000 soft). selfcheck → 0 misses. Pin, readback,
   pin record, CI read at BOTH pins (must be green).
6. §6: DONE/BLOCKED sessions_send to agent:maxie:discord:channel:1545487917575831684 AND hearth, same turn.
