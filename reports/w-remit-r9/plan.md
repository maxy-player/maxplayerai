# Round 9 plan — PR #979 after DENY at 4714623 (exec 1a5c7c5)

Governing: addendum 10 (`seller-fee-stage2a-addendum10-20260908T1150Z.md`, SHA-256
`4a3c945196b92b19a4238fd8c2e5d765b398dbab6e55b20bac992f19bfff493a`, computed by me) + frozen verdict
`seller-fee-stage2a-VERDICT-4714623-FROZEN-COPY-20260908T1125Z.md` §3.2–3.4, §4, §6, §9 (read).
Worktree HEAD at plan time: `ac033f4` (== origin/feat/seller-fee-remit). No `.rs` edited yet.
Every citation below re-read at `ac033f4` (whose `.rs` == `1a5c7c5`), 2026-09-08 12:00Z.

## 0. Citations verified at HEAD (line → what it does)

| cite | verified |
|---|---|
| `fee_remit.rs:1318–1332` | `debit_ceiling = reserve_ceiling + estimate.expected_fees_sats`; `> gross` ⇒ writes "REFUSED — … expects … sats of proof fees on top" and `return Refused(FeesDoNotFit)` — an EARLY RETURN before the §1.2 recheck that starts at `:1332` ("§1.2 re-check on the quote actually raised"). Verdict trace confirmed. |
| `:2272–2284` `plan_confirmable_invoice(gross, reserve, swap_fee, ppk)` | downward search `(1..=gross−reserve).rev().find(…)` on `post_swap_figures(need, None, ppk)` with both §1.1 inequalities. Witness 19/2/1000/[32] ⇒ 13 (need 15 → prepared 4 → target 19=[16,2,1] → actual 3; 19 ≥ 18; 13+2+3+1 = 19 ≤ 19). |
| `:2305–2347` `confirm_would_succeed(&preparation, gross) -> Result<u64,String>` | same inequalities on `Some(preparation.input_fee_sats)`; ONE-line reason strings at `:2322` and `:2340`. |
| `:1482–1720` live sequence | `:1482` melt_quote; `:1533` `ceiling.admits(amount, reserve)` reserve-only precheck; `:1576` `prepare_melt`; `:1597` `confirm_would_succeed`; `:1600` `cancel_melt` then `:1606` `refuse_before_fence`; `:1632` fence; `:1653` cancel on lost fence; `:1704` cancel on pay-time margin; `:1720` `confirm_melt`. |
| `:1508–1511` `refuse_before_fence` | calls `release_own_planned` (`:1508`) then ONE `writeln!` whose format string at `:1511` contains `\n  A seller never pays…` — two emitted lines. |
| `:1464–1468` `release_own_planned` | `store.release_remittance(id, ReleaseOn::OwnPlanned{owner}, now)`. |
| `store.rs:2372–2385` | `OwnPlanned` arm: `UPDATE … SET state = Failed, settled_at_unix` WHERE Planned ∧ `spending_since_unix IS NULL` ∧ owner; then `:2385` `UPDATE receipts SET remittance_id = NULL`. So the release is Failed + unpin. A Planned-preserving path = NO store write (see N2). |
| `store.rs:2180–2182` | doc of `admit_remittance_spend`: "released only when the mint reports that quote terminal, never on time (§1.2)" — addendum 5's withdrawn premise; `fee_remit.rs:810` holds every non-PAID bound row. |
| `fee_remit.rs:862–870` (reconcile_decision) | own Planned row, quote UNPAID/None, not spending ⇒ `Reconcile::Release{OwnPlanned}` "the row is this process's own earlier attempt, which is over". This is how a row left Planned by N2 is reaped on the NEXT attempt before re-planning. |
| `wallet_ops.rs:1262–1327` `expected_melt_fees` | exact-fit ⇒ (proofs fee, 0); swap layout ⇒ (`get_keyset_count_fee(split(need).len())` = PREPARED input estimate, swap fee on `select_proofs(need+input_fee)`). Witness: (4, 1). |
| `:1499–1545` | `:1507` `admits_total(invoice, reserve, input_fee_sats, swap_fee_sats)` on the PREPARED input fee — 13+2+4+1 = 20 > 19 rejects the witness; `:1534–1545` reads ppk AFTER this check (so the actual bound cannot be computed before it today — order must swap). |
| `:227–231` | `fee_sats` doc: "Lightning fee PLUS the ACTUAL proof input fee … Inclusive". |
| `:242–243` | `fee_reserve_sats`: "the ceiling on `fee_sats`" — FALSE (records 28: 4 > 3; 30: 5 > 2). |
| `:332–333` | `MeltEstimate::fee_reserve_sats`: "the ceiling on the fee it will take" — same defect. |
| `fee_remit.rs:1102` | PAID reconciliation prints "melt fee at most {reserve} sats (the quote's reserve …" — aggregate bound claim. |
| tests | 28 `:4620` (reserve 3, invoice 12, pool 32→15); 37 `:4762` (estimate 3 / live 0, invoice 12, negative, `:4815` counts substring, `:4841–4842` assert Failed + 0 receipts); 29 `:4871` (live reserve 7, Failed/receipts-back comment); 30 `:4978` (reserve 2 ⇒ 13); 31 `:5046` never-fits; 14 `:5089` 2f. At `24ba082`: `:4118` estimate 3 / live 0 success paying 12; `:4290` reserve 2 paying 14. |

## 1. N1 — one arithmetic, one gate (addendum 10 §1, verdict §3.2)

**Bound helper (single source of truth), `fee_remit.rs` beside `post_swap_figures`:**
```
pub(crate) struct ConfirmBound { need, prepared_input, target, actual_input, worst_debit }
pub(crate) fn confirm_bound(invoice, reserve, prepared_input: Option<u64>, swap_fee, ppk, requires_swap: bool)
    -> Result<ConfirmBound, ConfirmShortfall>   // Err names which inequality failed
```
Non-swap layout (`requires_swap == false`): actual = prepared (exact-fit fee), target = need. Swap layout:
`post_swap_figures(need, prepared_input, ppk)`; ok iff `target ≥ need + actual` ∧ `need + actual + swap ≤ gross`.
Three callers, no other arithmetic:
1. `plan_confirmable_invoice` — `prepared_input = None`, `requires_swap = ppk > 0 || swap_fee > 0` (planning
   presumes the swap layout the probe reported; the exact-fit case has swap 0 and is bounded by the same
   `need + actual ≤ gross` with actual = prepared).
2. `confirm_would_succeed(&preparation, gross)` — `Some(preparation.input_fee_sats)`, `preparation.requires_swap`.
3. **`wallet_ops.rs:1507`** — move the ppk read (`:1534–1545`) BEFORE the bound; replace `admits_total` on the
   prepared total by the same bound. The helper cannot live in `fee_remit.rs` for wallet_ops to call
   (dependency direction), so the arithmetic moves to `wallet_ops.rs` as `MeltCeiling::admits_confirmable(…)`
   returning `Result<u64 /*actual*/, MeltTotalExceedsCeiling-shaped detail>`, and `fee_remit.rs`'s three
   functions call THAT. `MeltCeiling::total_debit` stays as the figure named in refusals (now
   invoice + reserve + ACTUAL + swap). `admits_total` is kept only for its unit test if still referenced;
   otherwise deleted (grep first; test `:2053–2133` region).
   The fake's `prepare_melt` (`fee_remit.rs:3090–3108`) mirrors: same helper, same refusal wording.

**Remove the prepared-total early return `:1318–1329`.** The plan already chose `net` by the bound; the
`debit_ceiling` line stays only as the printed "leaves your wallet: at most …" figure, which becomes the
bound's `worst_debit` (13+2+3+1 = 19 for the witness), not `net + reserve + expected`. The §1.2 recheck
(`:1332–1362`) on the raised quote's reserve stays and uses the helper (it IS the continuation: if the
raised quote's reserve differs from the probe's and the bound fails on `net`, re-plan — see below —
rather than refuse).

**Downward search:** unchanged shape (`.rev().find`), now through the helper. Witness 19/2/1000/[32] ⇒ 13.
Reserve-0 gross 20 ⇒ **15**: 20: need 20=[16,4] prep 2 target 22=[16,4,2] actual 3, 22 < 23 ✗; 19: need 19
prep 3 target 22 actual 3, 22 ≥ 22 ✓, 19+3+1 = 23 > 20 ✗; 18: target 20=[16,4] actual 2, 18+2+1 = 21 ✗;
17: target 19 actual 3, 19 < 20 ✗; 16: target 17 actual 2, 17 < 18 ✗; **15**: need 15 prep 4 target 19
actual 3, 19 ≥ 18 ✓, 15+0+3+1 = 19 ≤ 20 ✓. Matches verdict §3.3.

**Adaptive re-plan when the live reserve differs (addendum 10 §1.4): decision = RE-PLAN IN THE SAME
ATTEMPT, once, BEFORE prepare.** Cite: addendum 10 §1.4 "correct adaptive re-plan or re-quote when the live
reserve shrinks makes the chosen invoice unconfirmable"; §2.1 "a repeated arithmetic refusal is a
live-reserve drift event, not a steady state" — a next-attempt re-quote cannot satisfy §1.4 because the
next attempt plans from the probe estimate again and meets the same drift (record 37's schedule is
deterministic: estimate 3 / live 0 every time), so it would loop. Mechanism, at `:1533` after the live quote
is raised and `admits` passes:
- if `quote.fee_reserve_sats == estimate.fee_reserve_sats` ⇒ continue unchanged.
- else run the bound on `(net, live_reserve, None, estimate.expected_swap_fee_sats, ppk)`. Holds ⇒ continue
  (a shrink that still fits pays with slack — e.g. live reserve 1 on a reserve-2 plan). Fails ⇒
  `plan_confirmable_invoice(gross, live_reserve, swap, ppk)`:
  - `None` ⇒ `refuse_before_fence` (N2 semantics: row Planned, one line).
  - `Some(net2)` ⇒ raise invoice(net2) + `melt_quote` (live) for it; **`store.replan_remittance(id, owner,
    &RemittancePlan2{net_sats, payment_hash, bolt11, melt_fee_reserve_sats, melt_quote_id}, now)`** — a new
    conditional store method: `UPDATE fee_remittances SET net_sats, payment_hash, bolt11, melt_fee_reserve_sats,
    melt_quote_id WHERE remittance_id ∧ state = Planned ∧ spending_since_unix IS NULL ∧ owner = ?`; zero rows
    ⇒ `Ok(None)` ⇒ refuse-before-fence "row changed under me". Receipts stay pinned, gross unchanged,
    one row per attempt. Print one line: "Re-planned: the payment quote's fee reserve is {live} sats (planned
    on {est}); invoice {net} sats would not confirm, invoice {net2} sats will (…figures…)". Then continue
    with the SECOND quote through the unchanged sequence (precheck → prepare → bound → fence → confirm).
    The `ceiling` (`MeltCeiling{invoice_sats}`) is rebuilt for `net2`. A second mismatch on the re-planned
    quote is NOT re-planned again: it takes `refuse_before_fence` (bounded, deterministic).
  Under the 20 / est 3 / live 0 / 1000 / [32] schedule: plan 12 (est 3) → live 0 → bound(12,0): need 12 prep
  2 target 14=[8,4,2] actual 3, 14 < 15 ✗ → re-plan 15 → live quote 15/0 → prepare: prep 4 swap 1 → bound ✓
  (19 ≤ 20) → fence → confirm: swap 32→19 (fee 1, change [8,4]), actual 3, 18 ≤ 19, melt, Lightning 0,
  change 19−15−0−3 = 1, `fee_paid` = 19−15−1 = 3; pool 32 → 12+1 = 13, delta 19 ≤ 20. Paid ONCE.
- Ordering note: the re-plan happens BEFORE any `prepare`, so nothing is cancelled and the Planned row is
  only updated, never released.
- Forbidden (§1.5): gross untouched; no swap loss on failed melt; post-confirm hold untouched; no ownership
  control changed (`admit_remittance_spend`, releases, `reconcile_decision` untouched).

## 2. N2 — pre-fence arithmetic refusal: row stays Planned, ONE line (addendum 10 §2, verdict §3.3)

**Decision: reuse the existing Planned-preserving path = write nothing to the row.** `refuse_before_fence`
drops its `release_own_planned` call. The row remains Planned, unbound (`spending_since_unix IS NULL`,
`spending_quote_id IS NULL`), owner + lease intact, receipts pinned. The attempt journal records `Failed`
(unchanged). The NEXT attempt's `reconcile_decision` (`:862–870`, unchanged) releases it as "own earlier
attempt, over" via `OwnPlanned` (→ Failed, unpin) and re-plans under §1 — exactly "backoff continues and the
next attempt re-quotes and re-plans". No new store variant is needed for N2; no store code changes for N2.
`release_own_planned` remains for the two non-arithmetic callers: `QuoteFailed` (`:1476`) and lost-fence
`LeaseTooShort` (`:1662`) — named here, unchanged, not in addendum 10's order.

**One line.** `:1511` format string loses its `\n  …` second sentence; the single line is:
`REFUSED before spending — {reason}; ceiling {gross} sats; nothing left the wallet; the row stays planned
and the next attempt re-quotes.` where `reason` (`:2322`/`:2340`) already names total, parts and ceiling.
Cancel (`:1600`) stays BEFORE the refusal line; its failure note (`:1601–1604`) is a distinct diagnostic
line only on cancel failure (never in the ordered tests).

**Tests** (records 37 `:4762`, 29 `:4871`, 14 `:5089`, and the quote/fee refusals that share the helper):
assert `out.lines().filter(|l| l.starts_with("REFUSED before spending")).count() == 1` AND that no line of
the refusal stanza follows (the line after is absent or the attempt-journal line), row `state ==
RemittanceState::Planned`, `spending_since_unix.is_none()`, `receipts == 2`, `melt_quote_id` planned id;
2f (`:5089`) second attempt: first the reconcile "own earlier attempt" release line, then a new row pays —
`rows.len() == 2`, [Failed, Settled]. Financial expectations (delta 0, no swap, quote UNPAID) unchanged.

## 3. N3 — the reserve bounds the LIGHTNING component (addendum 10 §3, verdict §4)

- `wallet_ops.rs:242–243`: "the mint's fee RESERVE on the paying quote: the ceiling on the LIGHTNING fee
  the mint may keep; `fee_sats` (Lightning + actual proof input fee) can exceed it by that input fee."
- `:332–333`: same, for `MeltEstimate`.
- `fee_remit.rs:1102`: "Lightning fee at most {reserve} sats (the quote's reserve); the inclusive melt fee
  (Lightning + actual proof input fee) is recorded as not observed — the mint reports PAID, not what it
  kept". `fee = None` stays; nothing manufactured.
- Plan line `:1367` "mint melt fee reserve (ceiling)" → "(bounds the Lightning fee)"; `:1375` labels each
  quantity: reserve · estimated proof input fee · actual proof input fee · actual swap fee · inclusive melt fee.
- Test (§3.3): extend the PAID-reconciliation test to assert the printed line names "Lightning fee at most"
  and does NOT contain "melt fee at most"; plus record 28/30 assert the plan line labels.

## 4. N4 — eight fixes (after §1–3 land; body + source), verdict §6 items 1–8

1. Body §15: three explicit effect cancellations `:1600` (arithmetic), `:1653` (lost fence), `:1704` (pay-time
   margin) — cite the post-round-9 lines.
2. Body §15: prepare "may fetch mint metadata/keysets (CDK `keysets.rs:44–51`), a GET; no proof- or
   fee-bearing request"; strike body §50's "replaced" claim, state it was false at 4714623.
3. Body §16: record 28 prepared 4 ≠ actual 3 (one number, agree with §46).
4. Body §46: refusal restated per §2 (Planned, one line, cancel first); record 37's figures: target 14,
   prepared total 15 = 12+0+2+1 (both quantities, named).
5. Body §46: true order reserve-only precheck → [re-plan if live reserve differs] → prepare → post-swap
   arithmetic → fence → confirm.
6. Body §46: CDK `from_prepared` (`saga/mod.rs:592`) zeroes `swap_fee` unconditionally; the actual swap fee
   is recomputed by our fake/our arithmetic — not a charged-fee promise.
7. = §3 above.
8. `store.rs:2180–2182`: "Once admitted the row is HELD until the mint reports the bound quote PAID
   (`fee_remit.rs:810`); no terminal-state release, no clock release."
Then: every fee/model/output sentence in body, module doc (`fee_remit.rs:1–120`), `wallet_ops` type docs,
quickstart re-read for MEANING against the new exec head; self-check script re-run; adversarial sample of
the seven failed claim groups re-verified by hand and listed in `reports/w-remit-r9/claims.md`.

## 5. New tests (names)

- §1.3 `a_fee_bearing_payment_at_a_19_sat_gross_the_prepared_estimate_would_refuse_pays_invoice_13_once`
  — gross 19 (`&[10, 9]`), reserve 2, 1000 ppk, [32]; asserts invoice 13, one swap, one melt, pool 32 → 13,
  delta 19 ≤ 19, `fee_paid` 4 (Lightning 1 + 3), no WARNING, ownership assertions as record 28.
- §1.4(a) `a_reserve_that_shrinks_between_estimate_and_payment_is_re_planned_once_and_pays_invoice_15`
  — 20 / est 3 / live 0 / 1000 / [32]; asserts the "Re-planned" line once, invoice 15 paid once, pool → 13,
  delta 19, one row Settled, receipts 2 discharged, `net_sats == 15`, `melt_quote_id` = second quote.
- §1.4(b) record 30 (`:4978`) stays (reserve 2 ⇒ 13). Record 37 (`:4762`) keeps its NEGATIVE separately:
  with re-plan wired, its old schedule (est 3 / live 0) now succeeds, so the negative moves to a schedule
  the re-plan cannot rescue — the live reserve equals the estimate (no re-plan branch) but the SDK's
  prepared figure differs from the planner's presumption: new fake knob `prepared_input_override =
  Some(2)` on the 12/0 quote ⇒ target 14 = [8,4,2], actual 3, 14 < 15 ⇒ refused before the fence, cancelled,
  Planned + one line, delta 0, quote UNPAID. Documented as the fee-metadata drift model (verdict §9 note).
- §2.2 the three refusal tests' new state/line assertions (above); one whole-stdout equality on record 29.
- §3.3 PAID-reconciliation label test.
- store: `replan_remittance_updates_only_our_own_planned_unbound_row_and_keeps_receipts_pinned` (+ zero-rows
  when Spending / other owner).
- unit: `confirm_bound` table test incl. 19/2 ⇒ 13, 20/0 ⇒ 15, 20/2 ⇒ 13, 3/1 ⇒ None.

## 6. Step order (each ≤ 5 min, commit + push + `pushed <sha>`)

1. `wallet_ops.rs`: `MeltCeiling::admits_confirmable` + `ConfirmBound`; ppk read moved before the bound at
   `:1507`; fake mirror; `admits_total` retired/kept per grep. Unit tests.  → commit A.
2. `fee_remit.rs`: `plan_confirmable_invoice`/`confirm_would_succeed` on the helper; remove `:1318–1329`
   early return; plan-line figures. Witness unit test. → commit B.
3. `store.rs` `replan_remittance` + store test. → commit C.
4. `fee_remit.rs` re-plan branch at `:1533`; §1.4(a) test; §1.3 test. → commit D.
5. N2: `refuse_before_fence` one line, no release; record 37/29/14 re-asserted; fake knob for 37. → commit E.
6. N3 docs/prints + label test. → commit F (last `.rs` touch = exec head unless fixes follow).
7. Gates (§7) at exec head. Fix-ups ⇒ new exec head, gates re-run there.
8. N4 body/docs/store comment (store.rs is `.rs`! — item 8 lands in commit F, before gates). Body + reports
   + self-check + claims.md → final head; pin commit; CI on final + pin.

## 7. Gates (addendum 9 §5 / addendum 10 §5), all at the exec head, `git rev-parse HEAD` line 1 of each log
in `reports/w-remit-r9/logs/`: core `--features wallet`; CLI default; CLI `acp,wallet`; money-path CI
command ×3 with FAILED counts; `cargo fmt --check`; clippy mapped over `a6217328..head` added set; every
`--exact` record (38 + new: §1.3, §1.4(a), §2.2 ×3, §3.3, store replan, confirm_bound). Then only docs/body/
reports commits; `git diff --stat exec..final` pasted; CI green on final and pin, stated at each.

## 8. Owed, not built (addendum 10 §7)
recovery path (`recover_incomplete_sagas`), BudgetGate, operator melt path keeps reserve-only ceiling,
threshold sweep, signed parameter, rate change, owed-vs-held/circuit breaker, force-release/human approval,
legacy unbound v12 rows, `pay_melt_quote_*`/`TerminalBoundQuote` removal, repo lint debt, three flakes,
LNURL buffer-before-limit + inherited false-default comment, rebase-or-merge as main moves.
