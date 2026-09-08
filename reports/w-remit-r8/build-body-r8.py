#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Round-8 body for PR #979. Input: reports/w-remit-r7/body-da0ee92.md (the pinned round-7 body).
Output: reports/w-remit-r8/body-r8.md. Every anchor must match exactly once; the script exits
non-zero otherwise. Citation convention: round-7 `file.rs:NNNN` / `:NNNN` tokens are left as
24ba082 numbers for step 26's hunk remap (remap-citations.py, 24ba082 → 1a5c7c5); text written
THIS round cites 1a5c7c5 directly with `@` instead of `:` (e.g. `fee_remit.rs@1597`), which step
26 turns into `:` AFTER the remap so the two never mix. Placeholders for step 27: <FINAL>,
<FINAL40>, <NCOMMITS>, <DIFFSTAT-EXEC-FINAL>, <CI-FINAL>, <CI-036302b>."""
import pathlib, re, sys

W = pathlib.Path("/Users/forge/forge/v2/wt/w-seller-fee-stage2a-remit")
R7 = W / "reports/w-remit-r7"
R8 = W / "reports/w-remit-r8"
t = (R7 / "body-da0ee92.md").read_text()

EXEC = "1a5c7c55665b5c207a98eb2a003b164de9641189"
E7 = "1a5c7c5"

def once(old, new):
    global t
    n = t.count(old)
    if n != 1:
        sys.exit(f"anchor found {n} times, expected 1: {old[:100]!r}")
    t = t.replace(old, new)

def lines():
    return t.split("\n")

def find_line(startswith):
    idx = [i for i, l in enumerate(lines()) if l.startswith(startswith)]
    if len(idx) != 1:
        sys.exit(f"line anchor found {len(idx)} times: {startswith!r}")
    return idx[0]

def replace_line(startswith, new):
    global t
    ls = lines()
    ls[find_line(startswith)] = new
    t = "\n".join(ls)

def replace_block(start_prefix, end_prefix, new_text):
    """Replace lines [start, end) where start is the unique line starting with start_prefix and
    end is the unique line starting with end_prefix (end line kept)."""
    global t
    ls = lines()
    a, b = find_line(start_prefix), find_line(end_prefix)
    if b <= a:
        sys.exit(f"block end before start: {start_prefix!r} / {end_prefix!r}")
    ls[a:b] = new_text.rstrip("\n").split("\n") + [""]
    t = "\n".join(ls)

# ---- line 1: heads, commit count, commit list ---------------------------------------------------
once("Final head `da0ee92ef92c9d59bc90ec059c26a34b3e661974` on `feat/seller-fee-remit` (executable head "
     "`24ba082ca2fc118614f935838d2193a507dcb0a5` — the last commit that touches a `.rs` file; the one commit "
     "after it is `docs/` only)",
     f"Final head `<FINAL40>` on `feat/seller-fee-remit` (executable head `{EXEC}` — the last commit that "
     "touches a `.rs` file; the commits after it are `reports/` and this body's pin only)")
once("Fifty-seven commits, linear (zero merge commits in `a6217328..da0ee92`)",
     "<NCOMMITS> commits, linear (zero merge commits in `a6217328..<FINAL>`)")
once("`24ba082` (five clippy diagnostics on round-7 lines cleared — **the executable head**), `da0ee92` "
     "(quickstart prose — **the final head**). This body is written against `da0ee92`.",
     "`24ba082` (five clippy diagnostics on round-7 lines cleared — round 7's executable head), `da0ee92` "
     "(quickstart prose — round 7's final head, DENY on B/G/H: fee model), then round 8 (addendum 9): "
     "`4d5a5f3` (the fake's confirm made CDK-shaped: swap output split, recomputed input fee, post-swap "
     "refusal), `a7c5968` (§1.1 confirmability before the fence + §1.3 fake self-check against the confirm "
     "model), `1236efc` (§1.2 fee-aware planning chooses a genuinely confirmable invoice), `cd0322b` (§1.4 "
     "regressions on the CDK fee model), `8baaf3b` (§2: the SDK's inclusive `fee_paid` counted once; balance "
     "unknown, not computed), `060ebb7` (§3/§4.1: cancellation, recovery and retired-call prose said "
     "truthfully), `b1c702a` (§4.4/§1.5: module doc, `wallet_ops` doc and quickstart restated), `1a5c7c5` "
     "(rustfmt on our added lines, one constant assertion replaced — **the executable head**), `036302b` "
     "(reports: every round-8 log, committed), `<FINAL>` (this body's pin — **the final head**). This body is "
     "written against `1a5c7c5`.")
once("`6fd13df` and `19f30d3` (the six graded heads) are ancestors of `da0ee92`.",
     "`6fd13df`, `19f30d3` and `da0ee92` (the seven graded heads) are ancestors of `<FINAL>`.")

# ---- heads paragraph (addendum 9 §5) ------------------------------------------------------------
replace_block("**Heads (addendum 8 §3).**", "## §5 statement", f"""**Heads (addendum 9 §5).** Executable head `{EXEC}` (the last commit touching any `.rs`); reports commit `036302b1311f9d93fa82bc02b731630288aad647` (`reports/w-remit-r8/` only); final head `<FINAL40>` (this body's pin); `git diff --stat 1a5c7c5..<FINAL>`:
```
<DIFFSTAT-EXEC-FINAL>
```
Every gate below ran at `{E7}` with `git rev-parse HEAD` as the first line of its log (`reports/w-remit-r8/`, committed in `036302b`, sizes and sha256 in `evidence-table.txt`). Why the executable head moved once inside this round: at `b1c702a` (the intended executable head) all three suites and the money-path trio had run, but the lint gate mapped onto lines added `a6217328..HEAD` found one clippy `assertions_on_constants` (`assert!(32 - 14 <= 20)` in the planner regression) and nine rustfmt blocks on round-8 lines; both were fixed by hand in `{E7}` (no behaviour change, no assertion weakened — the assertion is computed from the fake's pool) and **every** suite, the money-path trio, both lint gates and all 38 `--exact` records were re-run at `{E7}` (`exec2-*.log`, `money-path-ci-{{1,2,3}}-{E7}.log`, `exact-*-{E7}.log`). The `b1c702a` runs stay as `exec-*.log` / `step14-*.log` (first-attempt records). **Correction of round 7's report (addendum 9 §4.3):** the previous body called the `2d802a3` core, CLI and money-path suites "green". At `2d802a3` the core and CLI suites passed; the money-path trio did **not** — 1423/2, 1422/3, 1423/2 (the disclosed out-of-diff flakes, `money-path-ci-{{1,2,3}}-2d802a3.log` in `reports/w-remit-r7/`), as the money-path section already stated. The "green" summary is withdrawn; the FAILED counts stand.
""")

# ---- §5 bullet: the money hold (restated against 1a5c7c5) ---------------------------------------
replace_line("- **The money hold (addendum 3 §1; addendum 8 §1):**",
"- **The money hold (addendum 3 §1; addendum 8 §1; addendum 9 §1):** the seller never pays more than the fee it accrued — gross is the ceiling on **everything that leaves the wallet**: invoice + the mint's Lightning fee reserve + the **actual** proof input fee the SDK recomputes on the swapped proofs + the pre-melt swap fee — and a payment is admitted only when the SDK's own confirm arithmetic is shown to succeed **before the fence and before any fee-bearing request**. `MeltCeiling { max_debit_sats: gross, invoice_sats, planned_quote_id }` (`wallet_ops.rs:257`) carries `admits` (`:271`, invoice + reserve) in `remit_inner` right after the payment quote Q is raised and again in the wallet thread before `prepare_melt` (`wallet_ops.rs:1412`), then `admits_total` (`:281`; `total_debit` `:298`, saturating) on the prepared melt's four parts. **Round 8 adds step 1c, `confirm_would_succeed` (`fee_remit.rs@2305`, called at `@1597`):** from the SDK's prepared figures and the active keyset's `input_fee_ppk` (read after prepare, `wallet_ops.rs@1333`, stored on `MeltPreparation` `@384`) it computes the post-swap target = invoice + reserve + prepared input fee, that target's binary denomination split, the **actual** input fee on that split (`fee_for` `@2231` = ceil(ppk × count / 1000), the pinned crate's `fees.rs:35–48`), and requires target ≥ invoice + reserve + actual fee **and** invoice + reserve + actual fee + swap fee ≤ gross — exactly what CDK's `confirm` checks after its swap (`melt/saga/mod.rs:678` target, `:704–712` recompute + `InsufficientFunds`). Either failing ⇒ the prepared melt is cancelled (local) and the credited `refuse_before_fence` path runs: own **Planned** row released under `ReleaseOn::OwnPlanned`, attempt journaled `MeltRefused` with one line naming target, split, actual fee, the sum and the ceiling (`@2322`), nothing posted, **no Spending row exists to hold**. Spend order in `remit_inner` (`@1030`): quote → `admits` → margin → prepare + `admits_total` (`@1576`) → **`confirm_would_succeed`** → `after_quote` → fence (`admit_remittance_spend`) → pay-time margin → confirm (`@1720`); a fence or pay-time-margin refusal cancels the prepared melt first (`@1653`, `@1704`). Planning is fee-aware **and confirmable** (`plan_confirmable_invoice` `@2272`): it searches down from gross − reserve for the largest invoice for which the same two inequalities hold under the estimate quote's swap fee and ppk (gross 20, reserve 2, 1000 ppk, one 32-sat proof ⇒ invoice **13**, paying at 19 — not 14, which the SDK would refuse after its swap); a gross for which no invoice fits is `Refusal::FeesDoNotFit` at planning (`@545`). Ceiling meaning unchanged — gross accrued-unremitted, every cost comes out of it (Josip's ruling; no rate change). The debit bound of every two-process test is **at most one payment of the planned gross**; the fee-bearing tests measure the wallet, not a counter (round-8 section below): (i) pool 32 → 15, delta 17 ≤ 20 with prepared input fee 2 ≠ actual 3; (ii) prepared figures fit, post-swap arithmetic does not ⇒ refused before the fence, pool 32 unchanged, zero swaps, zero melts; planner case pool 32 → 13, delta 19 ≤ 20, one swap, one melt.")

# ---- Rounds 5 and 6, condensed to fit GitHub's 65,536-byte cap (names, lines, results kept) -------
replace_block("## Round 5 — what discharges B3 / D / G / H", "## Round 7 — what discharges B4, H1–H4 and I", """## Rounds 5 and 6 — B3 / D / G / H (verdicts at `6fc77e1` and `6fd13df`, addenda 6–7) — condensed; the full narrative is the round-7 body (`reports/w-remit-r7/body-da0ee92.md`, sha256 `c03e3b31…`, pinned in `body-pin-da0ee92.txt`); line numbers are at `24ba082`

**B3 (`a7eded2`).** Round 4 released a bound spending row on FAILED or UNPAID-past-expiry; the pinned CDK 0.17.2 mint pays UNPAID **or FAILED** quotes and checks **no expiry** (the wallet checks it only in `prepare_melt`), so two debits were possible. Since round 5 the release arm for bound spending rows is deleted (`reconcile_decision` `fee_remit.rs:737`, hold `:761`): the one exit is the mint's PAID on the bound quote asked by id (`melt_status_for_quote_async` `wallet_ops.rs:1571`); exclusion is `plan_remittance`'s in-flight refusal plus the one-planned index (`store.rs:2104`, `:1057`). Planned rows keep `TerminalQuotePlanned` / `LeaseExpired` / `OwnPlanned`; legacy unbound spending rows keep `TerminalUnboundSpending` (out of scope, for a ruling). The fake mint (`16f0daa`; wallet half → the `melt_gate` pause point (`fee_remit.rs@2672`) → mint half paying UNPAID or FAILED, no expiry check, PENDING/PAID/UNKNOWN refused) was reshaped in rounds 7–8.

**D / 2g — two processes A = `proc-a`, B = `proc-b`, own `SellerStore` connections; X = A's invoice `lnbc-fake-13-2-a`, gross 15 = 13 + reserve 2; `PauseAt::{Plan, Quote, Admit, Melt}`; sharing per test stated in the module doc.** `a_payment_prepared_before_expiry_cannot_be_doubled_by_a_release_after_it` (`:6238`; `ec56d0c`) — pool `[8,4,2,1, 8,4,2,1]`, A bound to Q (expiry 200) parked at `Melt`, clock → 261: B's `--dry-run` 261 / `--confirm` 262 are refused by **reconciliation** (`SpendingHeld`, one `HELD:` line each) and B's concrete plan for its own invoice Y is refused by the **store** (`store_b.plan_remittance` → `Err(PlanRefused::InFlight(X))`, the function `remit_inner` calls at `:1314`); A resumes, the mint pays the expired UNPAID Q — **melts 1**, X Settled, ledger (15, 0, 0), pool `[1,2,4,8]`. `a_bound_quote_expired_past_the_margin_is_held_and_its_owner_refuses_to_pay_it` (`:5488`; b2) — clock → 200, B holds, A resumes at 201 and refuses inside its margin: **melts 0**, X Spending, ledger (0, 15, 0). `a_spending_rows_bound_quote_decides_its_release_on_the_full_path` (`:5967`; d) — FAILED → B holds, A resumes, the mint pays the FAILED quote (one melt); UNPAID live → B holds, A pays; PENDING / UNKNOWN → B `SpendingHeld`, mint refuses, zero melts. Pure tables `a_spending_row_is_never_released_by_reconciliation_only_settled` (`:4705`; 900 / 960 / 961 / 10 000 / `i64::MAX`, held, PAID settles) and `an_unpaid_or_pending_interrupted_attempt_is_reconciled_without_paying_twice` (`:3595`). Node 2b `a_failed_remittance_leaves_the_receipt_journaled_the_job_paid_and_the_balance_intact` (`run.rs:13712`) — failed admitted melt ⇒ next attempt `Refused(SpendingHeld)`, `melts.len() == 1`, (0, 10); scripted `fake.status` PAID ⇒ `NothingUnremitted`, remitted 10, still one melt. (a) `a_second_process_cannot_release_a_live_owners_planned_row_and_exactly_one_debit_happens` (`fee_remit.rs:4899`), (b1) `a_spending_row_is_reconciled_by_its_bound_quote_not_by_an_expired_estimate` (`:5353`), (c) `an_owner_paused_after_its_quote_and_past_its_lease_is_released_and_never_pays_that_quote` (`:5661`), (B2) `a_release_decided_on_a_stale_planned_snapshot_cannot_revoke_a_later_admission` (`:5810`; synthetic clock: B's command clock 401 vs shared effects clock 100, disclosed); older `:4998`, `:5122`, `:5246`; store fence `the_spend_fence_admits_only_the_owner_of_a_planned_row_with_lease_to_spare_and_only_once` (`store.rs:4494`); migrations v10→v11→v12→v13. Per-test sharing scope (one fake mint / one effects clock / one proof pool) is stated in the module doc (`fee_remit.rs@184`–193, corrected `2d802a3`).

**CLI (addendum 6 §1.3; `bd369c5`, `9c8ce6b`, `f5c40cc`).** `remit_live` (`seller_fees.rs:386`) → `exit_code_for` (`:427`): `DryRun` / `Paid` 0, `Refused(_)` / `MeltRefused` **3** (`REFUSED` `:36`), `QuoteFailed` / `MeltFailed` 2 — `a_held_spending_row_exits_refused_on_dry_run_and_confirm` (`:1038`). `a_held_spending_row_prints_one_held_line_and_exits_refused_on_dry_run_and_confirm` (`:1157`) runs the REAL command twice through `remit_live`, the shipped `LiveEffects` and the packaged wallet at a temp home on a Spending row bound to a quote the wallet never raised (local lookup `None`, no network): whole stdout by equality, stderr empty, exit 3, row Spending and bound, ledger (0, 15, 0). **G:** no schema change since v13 (`SCHEMA_VERSION` `store.rs:47`); a held row's gross is `in_flight`. **H1 (`c7930c3`, `9c8ce6b`):** the hold precedes the PENDING/UNKNOWN check — UNPAID, FAILED, PENDING, UNKNOWN and "no such quote" on a bound Spending row are all `SpendingHeld`, one `HELD:` line (row id, bound quote, mint's answer, held sats). **H2 (`9bc76ce`), H3 (`19f30d3`, `2d802a3`):** prose — settles on PAID otherwise HOLDS; `TerminalBoundQuote` is a retained primitive with no automatic caller; "the inspected CDK 0.17.2 mint implementation, no deployed mint measured"; both exclusion boundaries named. **Round 6 touched no money code** (A, B, C, E, F, G, I PASS at `6fd13df`); its `ec56d0c` identity defect is under Disclosures.

""")

# ---- Round 7 section: B4 paragraphs condensed; the current model lives in Round 8 ---------------
replace_block("**B4 — the defect.** Rounds 4–6 bounded", "**H1 — `f5c40cc`.**", """**B4 — the defect and round 7's fix, condensed (superseded in part by round 8 below).** Rounds 4–6 bounded **invoice + the quote's fee reserve**; the pinned CDK 0.17.2 wallet also charges the **proof input fee** (`input_fee_ppk`, ceil(ppk × inputs / 1000), `wallet/mod.rs:356–369`) and, when the proofs do not fit, a **pre-melt swap with its own fee**, both inside `confirm`. Read in the pinned crate, not measured on any mint: `MeltSaga::prepare` (`melt/saga/mod.rs:286–460`) writes only to the wallet's localstore (`reserve_melt_quote`, `reserve_proofs`, `add_saga`) and reads keysets through the metadata cache (a GET at most); the fee-bearing POSTs are `swap_no_reserve` (`:687–697`) and `client.post_melt` (`:907–911`), both inside `confirm` (`melt/mod.rs:515` → `:831–887`); `PreparedMelt::cancel` (`melt/mod.rs:673` → saga `:817–831`) is best-effort local compensation (it logs its own DB errors and returns Ok). Round 7 made the wallet side a **two-phase API on a dedicated OS thread** (`wallet_ops::prepare_melt_payment_blocking` `:1310` → `prepared_melt_thread` `:1370`; `PreparedMeltPayment` `:382` with `confirm` / `cancel` / drop-cancels — the wrapper's Drop → Cancel → join is the product's, not the SDK's), put `MeltCeiling::admits_total` (`:281`; `total_debit` `:298`) on the prepared figures **before the fence**, and made planning subtract the expected fees (`f05345f`). **What round 7 got wrong (verdict at `da0ee92`, B/G/H):** the ceiling was taken on the SDK's prepared `input_fee`, which is an *estimate* (`saga/mod.rs:383–399`); CDK swaps to invoice + reserve + that estimate (`:678`), recomputes the actual fee on the swapped split (`:704`) and refuses *after* the swap fee is paid when the proofs no longer cover it (`:706–712`). Round 7's fake did not model that refusal, so its two "fitting" regressions were payments the SDK would have refused after the fence — leaving a bound Spending row held — and the PAID print added the prepared input fee to the SDK's `fee_paid`, which already contains the actual one. Round 8 repairs all of it; the credited controls (hold, exclusion at admission, fence, conditional releases, quote binding, spend order) are untouched.

""")

# ---- H1 paragraph (round 7): module-doc ranges cited explicitly at 1a5c7c5 ----------------------------
once("module `:123–131`); sharing scope per test (`:149–155`, above)",
     "module doc `fee_remit.rs@158`–170); sharing scope per test (`fee_remit.rs@184`–193, above)")

# ---- Round 8 section, inserted before the Gates heading -----------------------------------------
ROUND8 = f"""## Round 8 — what discharges B (F1), G (F2) and H (F3, F4) (verdict at `da0ee92`, addendum 9)

**F1 / B — the fee model is CDK's, and a payment is proven confirmable before the fence.** *CDK 0.17.2, read at the pinned source (`~/.cargo/registry/src/index.crates.io-…/cdk-0.17.2/src/wallet/`):* `prepare` estimates `input_fee` on the split of invoice + reserve (`melt/saga/mod.rs:383–399`) and computes `swap_fee` exactly on the selected inputs (`:403`; `from_prepared` sets it to zero when no swap is needed, `:574–595`); `confirm` swaps to target = invoice + reserve + prepared `input_fee` (`:678`; the swap output is the target's denomination split, `swap/saga/mod.rs:285–301`), **recomputes the actual input fee on the received proofs** (`:704`) and returns `InsufficientFunds` **after the swap fee has been paid** when they do not cover invoice + reserve + actual fee (`:706–712`); `fee_paid` = final proofs − invoice − returned change (`:136–148`), i.e. Lightning fee **plus the actual input fee**, excluding the swap fee. *The code now:* (1) `confirm_would_succeed` (`fee_remit.rs@2305`) runs between prepare and the fence (`@1597`) with the active keyset's `input_fee_ppk` (`wallet_ops::active_keyset_input_fee_ppk`, `wallet_ops.rs@1333`, read once after prepare; a failed read cancels the preparation and refuses); it computes target, `binary_split(target)` (`fee_remit.rs@2238`), `fee_for(ppk, len(split))` (`@2231`) and refuses **before the fence with no fee-bearing effect** unless target ≥ invoice + reserve + actual fee and invoice + reserve + actual fee + swap fee ≤ gross — one printed line (`@2322`), `effects.cancel_melt` (`@1600`), row Planned → released `OwnPlanned`. (2) `plan_confirmable_invoice` (`@2272`) searches downward from gross − reserve for the largest invoice satisfying both inequalities under the estimate's swap fee and ppk; none ⇒ `FeesDoNotFit` at planning (`@1325`, `@1358`); the plan line prints the SDK estimate, the recomputed actual fee and the worst-case debit ≤ gross; a payment quote whose reserve grew is re-checked with `post_swap_figures` (`@2251`) before prepare. (3) **The fake is CDK-shaped on confirm** (`test_support::Fake::confirm_melt` `@3167`): swap first — the selected proofs leave the pool, the target's binary split is received, the swap fee is charged **even when the melt then fails**, change returns to the pool — then the actual input fee is recomputed on the received split and the melt is **refused after the swap** when received < invoice + reserve + actual fee (`MeltFailure::Failed("wallet refuses to melt quote … after its swap …")`, proofs returned, swap fee lost); then the credited melt gate and mint half; `fee_paid` = proofs melted − invoice − change, inclusive of the actual input fee, exactly CDK's; every swap and melt is counted (`FakeSwap` `@2461`, `swaps` `@2636`). The self-check `the_fake_wallet_models_the_sdks_proof_input_and_swap_fees_and_the_total_bound_refuses_them` (`@4405`) now tests the fake against **this** confirm model, not its own preparation: with 1000 ppk and pool `[32]`, invoice 12 / reserve 0 is refused *after* the swap (1 swap, 0 melts, pool 31) and invoice 13 / reserve 2 pays with `fee_paid` 4, pool 14, delta ≤ 20; and the planner is asserted to choose 13 for gross 20. (4) **Regressions (§1.4; full path, real `SellerStore`, wallet value measured, swaps AND melts counted):** (i) `a_fee_bearing_payment_whose_prepared_input_fee_differs_from_the_actual_pays_once_and_the_wallet_loses_at_most_the_gross` (`@4620`; record 28): 1000 ppk, pool `[32]`, gross 20, reserve 3, invoice 12, plan line `expected proof fees (SDK estimate …): 5 sats; actual proof input fee the SDK recomputes on the swapped proofs: 3 sats`; prepared input fee **4** (estimate) ≠ actual **3**: target 19 = 12 + 3 + 4, the 32-sat proof is swapped (`sent [32]`, `target_sats 19`, `swap_fee_sats 1`, `received [16, 2, 1]`, `change [8, 4]`), actual fee on three proofs = 3; one swap, one melt, `fee_paid` 4 = Lightning 1 + actual input 3, PAID print `actual debit: 17 sats = net + melt fee + swap fee`, **pool 32 → 15, delta 17 = 12 + 1 + 3 + 1 ≤ 20**, no WARNING, `proofs_spent == [[32]]`, row Settled with `melt_fee_sats = Some(4)`, `melt_fee_reserve_sats = Some(3)`. (ii) `a_fee_bearing_schedule_whose_prepared_figures_fit_but_post_swap_arithmetic_does_not_is_refused_before_the_fence` (`@4762`; record 37): reserve 0, invoice 12, prepared figures fit (14 ≤ 20) but the swapped split `[8,4,2]` carries an actual fee of 3 > estimate 2 ⇒ refused **before the fence** with the exact one-line reason (`@4781`), `swaps == []`, `melts == []`, one `cancel_melt`, **pool 32 unchanged**, quote UNPAID, row Failed, ledger (0, 0, 20). (iii) kept and re-recorded: fee-exceeds (`@4871`; record 29), never-fits (`@5046`; record 31), reserve 2→4 (`@5089`; record 14), fee-aware planner (`@4978`; record 30 — now invoice **13**: invoices `[20, 13]`, one melt `lnbc-fake-13-4`, pool 32 → 13, `melt_fee_sats = Some(5)`), `wallet_ops` admits-total / display / relay / drop-cancels (records 33–36). No expectation was changed to accept a swap loss on a failed melt. **§1.5 — bound disclosed, not solved:** the keyset fee metadata can change between prepare and confirm and the SDK takes no caller maximum; then CDK's own post-swap refusal fires *after* the fence and the credited bound-Spending hold covers it (row held, mint quote UNPAID, nothing double-paid). No exploit is claimed or reproduced; stated in the module doc (`fee_remit.rs` header), `wallet_ops` and the quickstart.

**F2 / G — the SDK's inclusive `fee_paid` is counted once.** `MeltOutcome.fee_sats` is CDK's `fee_paid` (Lightning fee + actual input fee); the delivery had added the prepared input fee to it again (verdict citations at `24ba082`: `wallet_ops.rs:1498`, `:1504–1507`; `fee_remit.rs:1624–1628`; WARNING `:1646–1657`), fabricating an above-gross "wallet lost" warning. Now (`8baaf3b`): `spent = paid + fee_paid + swap_fee` (`wallet_ops.rs@1595`); the PAID print (`fee_remit.rs@1753`) labels the four figures apart — `melt fee taken by the mint` (the SDK's `fee_paid` = Lightning fee + actual proof input fee), `estimated proof input fee (prepared) … replaced by the actual fee inside the melt fee above, not added again`, `swap fee (charged at swap)`, and **`actual debit: D sats = net + melt fee + swap fee`**; the unused-balance and WARNING figures derive from that debit; `MeltOutcome.balance_after_sats: Option<u64>` (`@238`) is `None` when the post-confirm balance read fails and the print says **`wallet balance now: unknown (the balance read after the payment failed; the payment stands)`** — never a computed number. Reconciliation keeps `fee = None` (cdk-common `MeltQuote` has no `fee_paid`) — unchanged. Regression (§2.4): `a_failed_balance_read_after_a_fee_bearing_payment_prints_unknown_and_no_false_warning` (`@4712`; record 38): prepared input ≠ actual, `balance_read_fails = true` ⇒ one payment, "unknown" printed, **no WARNING**, delta ≤ gross on the fake wallet.

**F3 / H — cancellation and recovery said truthfully (`060ebb7`).** Every "reopening the wallet recovers the reserved proofs" claim is struck (`wallet_ops.rs` thread/refusal prose, `fee_remit.rs` cancel-failure notes): `open_wallet_async` only constructs the wallet and the SDK's `recover_incomplete_sagas` is **not** called on the remittance path (only `crossmint_hop` calls it) — and it is **not wired this round** (addendum 9 §3.1: a blanket recovery could replay a still-live payer's saga). The cancel-failure note now reads: *"no fee-bearing request was posted; its local proof reservation may remain until a supported recovery path — owed — releases it."* SDK cancel is described as best-effort local compensation (it swallows and logs its own DB errors and still returns Ok, saga `:817–830`); the Drop → Cancel → join guarantee is credited to the product wrapper (`PreparedMeltPayment`), not the SDK; "the mint learns nothing" / "local only" became the true bound: prepare may GET mint metadata and keysets and posts no proof-bearing or fee-bearing request.

**F4 / H — stale prose and this body.** `lnurl_pay.rs:27`, `seller_fees.rs:20` / `:1086` (verdict citations at `da0ee92`) and `wallet_ops.rs` no longer call `pay_melt_quote_*` the current remittance spend: the live edge is `prepare_melt_payment_blocking → PreparedMeltPayment::confirm`; `pay_melt_quote_async` / `_blocking` are retained, uncalled, removal owed to the owners. In this body: the final-head-CI waiver is struck (CI section below states every head), the `2d802a3` "green" is withdrawn (heads paragraph), every fee/model/output claim above is restated against `1a5c7c5`, every load-bearing citation re-anchored and read back at `1a5c7c5` (`citation-remap-1a5c7c5.txt`, `citation-selfcheck-1a5c7c5.txt`), and the body is re-pinned. Module doc (`fee_remit.rs` header), `wallet_ops.rs` type docs and `docs/SELLER-QUICKSTART.md` (~`:1126–1153`) carry the same model, the §1.5 bound and the owed recovery path; the operator `maxplayer wallet melt` (`melt_within_*`) keeps its reserve-only check and says so.

**Prior-finding ledger (this round's rows):** B/F1 fake admits SDK-impossible success → closed at `1a5c7c5` (records 32, 28, 37). G/F2 inclusive `fee_paid` double-counted → closed (record 38 + PAID print). H/F3 false recovery/cancel prose → closed as prose; the recovery path itself is **owed**, not built. H/F4 stale current-caller prose, body §196 / §10 → closed. Rounds 1–7 rows unchanged.

"""
gates_idx = find_line("## Gates — command and output, all at `24ba082`")
ls = lines(); ls[gates_idx:gates_idx] = ROUND8.rstrip("\n").split("\n") + [""]; t = "\n".join(ls)

# ---- Gates heading + exact block + spending-edge line -------------------------------------------
replace_line("## Gates — command and output, all at `24ba082`",
             f"## Gates — command and output, all at `{E7}` (local, macOS arm64; every log begins with `git rev-parse HEAD` = `{EXEC}`; logs in `reports/w-remit-r8/` (committed `036302b`), sizes and sha256 in `evidence-table.txt`; grep gates 1–4 re-measured at `{E7}` — `greps-{E7}.txt`)")

# exact block: rebuild from exact-records.txt
recs = [l.split("|", 1) for l in (R8 / "exact-records.txt").read_text().splitlines() if l.strip()]
if len(recs) != 38: sys.exit(f"expected 38 exact records, got {len(recs)}")
rows = []
for tag, cmd in recs:
    m = re.search(r"--exact (\S+)", cmd)
    kind = "CLI" if "-p maxplayer " in cmd else "core"
    rows.append(f"exact-{tag}-{E7}.log · {kind} · {m.group(1)}")
replace_line("**Gates 2a (New-only), 2b–2g, 2f, E, v13, the CLI and the round-7 regressions — ONE `--exact` run per named test at `24ba082` (H4).",
             f"**Gates 2a (New-only), 2b–2g, 2f, E, v13, the CLI, the round-7 and the round-8 regressions — ONE `--exact` run per named test at `{E7}` (H4). Each block is one log: `git rev-parse HEAD` (line 1), the command (line 2), the `test <full name> ... ok` line, `1 passed`. 38 logs (36 kept + records 28, 37, 38 re-purposed/added for §1.4 and §2.4), 38 × `1 passed`, 0 failed (`exact-records.txt`; core records run `cargo test -p maxplayer-core --features wallet --lib -- --exact <name>`, CLI records `cargo test -p maxplayer --features acp,wallet --bin maxplayer -- --exact <name>`).**")
a = find_line(f"exact-01-2b-node-24ba082.log"); b = find_line("exact-36-r7-drop-cancels-24ba082.log")
ls = lines(); ls[a:b + 1] = rows; t = "\n".join(ls)

# gate 1–4 grep blocks (24ba082 output) are re-measured and substituted in step 26 — see NOTE at the end.
# The gate-4 word census is replaced by a placeholder step 26 fills from greps-1a5c7c5.txt (compact form).
replace_block("**Gate 4 / money-hold census — over ALL lines added `a6217328..24ba082`",
              "$ grep -n 'effects.prepare_melt(\\|effects.confirm_melt(\\|effects.cancel_melt(' crates/maxp",
              f"""**Gate 4 / money-hold census — over ALL lines added `a6217328..{E7}` (Rust: 14,773 added lines), every spending or hold identifier counted by `git diff -U0 a6217328..{E7} -- '*.rs' | grep '^+' | grep -c <name>` (`greps-{E7}.txt`); the spend edge grepped in place:**
```
<CENSUS-1a5c7c5>
""")
placeholders_extra = ["<CENSUS-1a5c7c5>"]

replace_line("The sole live spending edge is `fee_remit.rs:1480 prepare_melt → :1606 confirm_melt`",
             "The sole live spending edge is `fee_remit.rs:1480 prepare_melt → :1606 confirm_melt` → `LiveEffects` `:409` / `:427` → `wallet_ops::prepare_melt_payment_blocking` (`wallet_ops.rs:1310`) → `prepared_melt_thread` (`:1370`): `admits` (`:1412`) → `Wallet::prepare_melt` (local) → `admits_total` (`:281`) → **`confirm_would_succeed` (`fee_remit.rs@2305`, round 8)** → fence → `PreparedMeltPayment::confirm` (`wallet_ops.rs:1492`), reached only after `admit_remittance_spend` changed one row and bound that quote's id, only for that id, and never while a bound spending row exists, because `plan_remittance` refuses first.")

# ---- Full suites / money-path / CI / fmt-clippy / diffstat / evidence ---------------------------
replace_block("## Full suites at `24ba082`", "## Disclosures", f"""## Full suites at `{E7}` (local, macOS arm64; each suite its own command with `--no-fail-fast`, `git rev-parse HEAD` first in each log)
```
$ cargo test -p maxplayer-core --features wallet --no-fail-fast                 (exec2-core-wallet-nff.log)
lib: test result: FAILED. 1422 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 140.27s
     FAILED: credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it   (loopback "Connection reset by peer"; file not in this PR — the disclosed flake)
     integration/doc binaries: 8, 1, 1, 1, 1, 2 passed, 0 failed
$ cargo test -p maxplayer --no-fail-fast                                        (exec2-cli-default.log)
test result: ok. 143 passed; 0 failed   + cli_e2e 2, mcp_daemon 3, sell 3, wallet 6 — all 0 failed
$ cargo test -p maxplayer --features acp,wallet --no-fail-fast                  (exec2-cli-acp-wallet.log)
test result: ok. 180 passed; 0 failed; 1 ignored   + 2, 3, 5, 3, 6 passed, 0 failed
```
Every remittance, `wallet_ops`, store, seller-node and CLI test passed; the one lib failure is the pre-existing `credential_proxy` loopback flake also seen in round 7's run 3.

## Money-path binary ×3 at `{E7}` (addendum 4 §4) — CI's exact command (`.github/workflows/ci.yml:233`)
```
$ cargo test -p maxplayer-core --release --no-default-features --features gateway,git-delivery,wallet,live-mints --locked       (money-path-ci-{{1,2,3}}-{E7}.log; `git rev-parse HEAD` = {E7}… first line of each)
run 1: test result: ok. 1427 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 142.11s   (exit 0)
run 2: test result: FAILED. 1424 passed; 3 failed; 2 ignored; 0 measured; 0 filtered out; finished in 152.79s
run 3: test result: FAILED. 1424 passed; 3 failed; 2 ignored; 0 measured; 0 filtered out; finished in 141.27s
       FAILED (runs 2 and 3, the same three names): credential_proxy::tests::a_declared_over_cap_body_is_refused_before_the_upstream_sees_it   (loopback "Connection reset by peer")
               job_lifecycle::tests::a_wait_resolves_on_arrival_rather_than_on_the_safety_recheck    ("mint_unreachable: mint https://mint.minibits.cash/Bitcoin … fee query exceeded 5s")
               job_lifecycle::tests::get_job_default_skips_kind_zero_fetch                            (same mint_unreachable)
round 7 (kept, `reports/w-remit-r7/`): at 24ba082 1425/0, 1425/0, 1422/3; at 2d802a3 1423/2, 1422/3, 1423/2 — the same names only
```
- Every remittance, wallet_ops and store test passed in all three release runs (and in round 7's six). The failing files (`credential_proxy.rs`, `job_lifecycle.rs`) are not in this PR's diff; the `job_lifecycle` tests need `mint.minibits.cash` to answer a fee query within 5 s from this host. Reported as FAILED counts, never as green; the three flakes are out of scope by addendum 9 §7.

## CI — every head of rounds 7 and 8 (addendum 9 §5: green on the FINAL head, stated at both)
```
24ba082  run 34200030811  pull_request  completed  success   2026-09-08T07:49:03Z   (round 7 executable head; ci-24ba082.txt)
da0ee92  run 34202015987  pull_request  completed  success   2026-09-08T08:11:42Z   (round 7 final head — read this round; the previous body wrongly called this run "not required")
{E7}  run 34213151371  pull_request  completed  success   2026-09-08T10:14:27Z   (round 8 executable head)
036302b  run 34215238371  pull_request  <CI-036302b>                                  (reports only)
<FINAL>  run <CI-FINAL>                                                                (this body's pin — the final head)
```
Read with `gh run list --commit <sha>`. The seven code checks (Money-path tests · Test the full shipped feature combo (acp + wallet) · Build & test (acp) · Build & test (default features) · Build (no default features) · JS test suites · Release workflow gates) are what "success" covers; **Vercel** "Authorization required to deploy" is an authorization status on the fork PR, not a build result.

## fmt / clippy — the WHOLE delivery `a6217328..{E7}`, additions counted, and how each number is counted
**Denominators (measured at `{E7}`):** 11 files, **+14,918 / −266**; Rust **+14,773 / −251** (10 `.rs` files; 14,298 non-empty added lines). Method: `git diff -U0 a6217328..HEAD` per file → the set of added line ranges (`exec2-added-ranges.diff`, 160 hunks); every clippy `--> file:line` and every rustfmt `Diff in file:line` block is classified INSIDE or OUTSIDE that set, and for each INSIDE fmt block every line rustfmt would rewrite is `git blame`d (`exec2-lint-mapping.md`).
- **rustfmt:** `cargo fmt --all -- --check` at `{E7}` reports **2,106 blocks** in the tree (`exec2-fmt.log`, exit 1; the repo is unformatted at the base). **Lines this delivery added that rustfmt would rewrite: 0.** Three blocks start inside the added set by context adjacency (`home.rs:1445`, `lib.rs:47`, `lib.rs:55`); the lines they rewrite are `home.rs:1448` and `lib.rs:60–63`, all `git blame` → orveth (pre-existing), or none (`lib.rs:47`, a pure insertion target) — not reformatted, repo-wide lint debt (addendum 9 §7). At `b1c702a` the same gate found nine blocks on round-8 lines; `{E7}` formatted those lines by hand.
- **clippy (H5 convention — diagnostic blocks carrying a `--> file:line` location):** `cargo clippy --workspace --all-targets --features wallet` at `{E7}`: **125 total / 0 on lines added `a6217328..HEAD`** (17 in touched files on lines this branch did not add, 108 elsewhere; each listed in `exec2-lint-mapping.md`), exit 0 (`exec2-clippy.log`). At `b1c702a`: 125 / **1** (`assertions_on_constants` in the planner regression) — the reason the executable head moved to `{E7}`.

## Per-file diffstat `a6217328..{E7}`
```
7571	0	crates/maxplayer-core/src/fee_remit.rs
144	0	crates/maxplayer-core/src/home.rs
14	3	crates/maxplayer-core/src/lib.rs
1287	0	crates/maxplayer-core/src/lnurl_pay.rs
66	20	crates/maxplayer-core/src/platform_fee.rs
1314	10	crates/maxplayer-core/src/seller_node/run.rs
2327	122	crates/maxplayer-core/src/seller_node/store.rs
1291	3	crates/maxplayer-core/src/wallet_ops.rs
5	3	crates/maxplayer/src/sell.rs
754	90	crates/maxplayer/src/seller_fees.rs
145	15	docs/SELLER-QUICKSTART.md
 11 files changed, 14918 insertions(+), 266 deletions(-)       (+ 036302b: reports/w-remit-r8/ 72 files, 159,342 lines; + <FINAL>: reports only)
```

## Evidence files (`reports/w-remit-r8/`, committed in `036302b`; name · bytes · sha256; the full 71-row table with every `exact-*` log is `evidence-table.txt`; rounds 5–7 stay in `reports/w-remit-r{{5,6,7}}/` as listed in the previous bodies)
```
evidence-table.txt · 11731 · cf458d71444c1ea12410dea9ed06ff47c67488ab31713b9a992e51cbaa332e0d
exact-records.txt · 6736 · 7de6110f16c526aa1d15bf10f447fea4008f318e19fc4685fa647826a190a84d
exec2-core-wallet-nff.log · 136297 · 797b325d634f729c0a0bf639c184119deee999d78506b8613487b445c9ee9f95
exec2-cli-default.log · 16846 · ead094aed82503796c94e9bbd863d334942d62b534c52f4f0a4f9c693b25b8aa
exec2-cli-acp-wallet.log · 18559 · c97b420f22f42c257a9af158d5ca90af41244ba569ce72c6ce22428655068d48
money-path-ci-1-{E7}.log · 134735 · 0103e44c84e21cc7e2f9e1aa5fb01cbbd1e5585ee490b18382f322188f4b0a6e
money-path-ci-2-{E7}.log · 130802 · 3d89320d6ac0e88675f8c2d8d06752775836871f18662a97504ff40ab1fd84b4
money-path-ci-3-{E7}.log · 130805 · 808320f23fe1ed6d17b223c222a272fecd063bb305d461828028484127319600
exact-28-r8-i-fee-fits-actual-differs-{E7}.log · 3639 · fcbd74f421020706882a84717840a2d5c36ace407304d1e035a7d5935af72b0f
exact-37-r8-ii-post-swap-refused-before-fence-{E7}.log · 3623 · d44ab349f9b719d3ccea7551b070702d5880eac634c91dc8a4d8cd696847484c
exact-38-r8-balance-unknown-no-false-warning-{E7}.log · 3571 · 2912dd6dc98a57436032beb28d3043761fa4464ee5eaf20fb67141905ba4babc
plan.md · 10661 · 7aac574b998991a4a660ee8524401e7575c1761dc275956db3387a5f3c17338b
(+ exec2-clippy.log, exec2-fmt.log, exec2-added-ranges.diff, exec2-lint-mapping.md, the other 35 exact-NN-*-{E7}.log records, the b1c702a first-attempt logs, step2–8 logs — all in evidence-table.txt; body-plan.md, build-body-r8.py, greps-{E7}.txt, citation-remap-{E7}.txt, body-r8.md and body-pin-<FINAL>.txt are added by <FINAL>)
```

""")

# ---- Disclosures ----------------------------------------------------------------------------------
replace_line("- **The executable head moved once inside the round** (`2d802a3` → `24ba082`)",
             "- **The executable head moved once inside round 7** (`2d802a3` → `24ba082`, five clippy diagnostics) **and once inside round 8** (`b1c702a` → `1a5c7c5`, one clippy diagnostic and nine rustfmt blocks on round-8 lines; +fmt only, one constant assertion computed from the fake's pool instead). Every gate was re-run at the head it is reported for; the earlier logs stay in `reports/`.")
replace_line("- **The fake's fee model is read from the pinned CDK 0.17.2 source, not measured on any mint.**",
             "- **The fake's fee model is read from the pinned CDK 0.17.2 source, not measured on any mint.** On confirm it now does what the SDK does — swap to the target's split, charge the swap fee even when the melt then fails, recompute the actual input fee on the received split, refuse after the swap when it does not cover invoice + reserve + actual fee, and return `fee_paid` inclusive of the actual input fee — and the self-check test pins that model. It still uses a binary denomination split for a power-of-two keyset and the active keyset's single `input_fee_ppk`; a mint with several fee-bearing keysets or a non-power-of-two keyset is not modelled. **§1.5 bound (disclosed, not solved):** the fee metadata can change between prepare and confirm and the SDK takes no caller maximum; in that window CDK's post-swap refusal fires after the fence and the bound-Spending hold covers it.")
replace_line("- **The (i)/(ii)/(iii-b) expected numbers were re-derived once before commit**",
             "- **Round 7's two \"fitting\" regressions were false successes** (verdict at `da0ee92`): the fake let invoice 12 (reserve 0, ppk 1000, pool `[32]`) and invoice 14 pay where CDK refuses after its swap. Their inputs are kept as record 37 (now refused before the fence) and record 30 (now invoice 13); no expectation was changed to accept a swap loss on a failed melt.")
replace_line("- **Money-path run 3 failed on the three disclosed flakes**",
             "- **Money-path runs 2 and 3 at `1a5c7c5` failed on the three disclosed out-of-diff flakes** (`credential_proxy` loopback, `job_lifecycle` live mint), as did round 7's run 3 and the `2d802a3` trio (2/3/2 — never green; round 7's body said \"green\" once in its heads paragraph, withdrawn above); the core lib run at `1a5c7c5` hit the `credential_proxy` flake once. Reported as FAILED counts, never as green.")
replace_line("- No rebase, no force-push, no new PR; the branch is still on `a6217328` while `main` has moved",
             "- **Recovery of a proof reservation left behind by a failed local cancel is OWED, not built** (addendum 9 §3.1): `recover_incomplete_sagas` is not wired on the remittance path this round, by instruction; the prose says so. **Owed too:** operator force-release / human approval of a held row; owed-vs-held accounting; removal of `pay_melt_quote_*` and `TerminalBoundQuote`.\n- No rebase, no force-push, no new PR; the branch is still on `a6217328` while `main` has moved (`ea6a4b7`+); the rebase-or-merge decision is the owners'. Round 8's `reports/w-remit-r8/` **is committed** (`036302b`, `<FINAL>`); rounds 4–7's `reports/` directories stay untracked in the worktree.")

# ---- Out of scope (addendum 9 §7) -------------------------------------------------------------------
replace_line("## Out of scope, named not done (addendum 8 §6)", "## Out of scope, named not done (addendum 9 §7)")
oos_idx = find_line("## Out of scope, named not done (addendum 9 §7)")
ls = lines()
ls[oos_idx + 1] = ("`BudgetGate` wiring (owner decision, errata 1). The operator melt path — `maxplayer wallet melt`, `melt_within_*` — **keeps its reserve-only ceiling** (invoice + reserve, no proof-fee bound, no confirmability check) and is not touched. The automatic threshold sweep, the signed platform parameter, any rate change, owed-vs-held accounting, an operator force-release / human-approval path for a held row, legacy unbound rows, removal of `pay_melt_quote_*` and `TerminalBoundQuote` (owners' call), **wiring `recover_incomplete_sagas`** (a supported recovery path for a stranded local reservation — owed), repo-wide lint debt (125 clippy diagnostics and 2,106 rustfmt blocks on pre-existing lines), the three out-of-diff flakes, and rebase-or-merge as `main` moves. Named as owed; none built.")
t = "\n".join(ls)

# ---- write ----------------------------------------------------------------------------------------
out = R8 / "body-r8.md"
out.write_text(t)
n = len(t.encode())
print("assembled:", n, "bytes,", t.count("\n") + 1, "lines")
for ph in ["<FINAL>", "<FINAL40>", "<NCOMMITS>", "<DIFFSTAT-EXEC-FINAL>", "<CI-FINAL>", "<CI-036302b>", "<CENSUS-1a5c7c5>"]:
    print(f"  placeholder {ph}: {t.count(ph)}")
print("  '@' citations (1a5c7c5, step 26 converts):", len(re.findall(r"\.rs@\d+|`@\d+`", t)))
if n > 65000:
    sys.exit(f"TOO BIG: {n} > 65000")
# NOTE for step 26: gate 1–4 grep blocks and the gate-4 census still show 24ba082 line numbers; step 26
# re-runs those greps at 1a5c7c5 (greps-1a5c7c5.txt) and substitutes, then remaps every `:` citation
# 24ba082 → 1a5c7c5 with remap-citations.py, then turns `@` into `:`.
