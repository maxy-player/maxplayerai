# Round 8 plan — PR #979 after DENY at da0ee92 (code head 24ba082)

Written 2026-09-08 in worktree `~/forge/v2/wt/w-seller-fee-stage2a-remit`, HEAD `da0ee92` = origin, tree `2e800acd`.
Governing: addendum 9 (`seller-fee-stage2a-addendum9-20260908T0910Z.md`) + frozen verdict
(`seller-fee-stage2a-VERDICT-da0ee92-FROZEN-COPY-20260908T0907Z.md`, sha256 `84369810…4555d`).
Verdict: B, G, H FAIL; A, C, D, E, F, I PASS; 2f and all seven 2g PASS. Append-only, no rebase/amend/force-push.

## The B/G/H defect in one paragraph

The fee *model* is wrong, not the ownership controls. Pinned CDK 0.17.2 `prepare_melt` computes an exact
`swap_fee` on the selected inputs but only an *estimated* `input_fee` on the split of invoice + reserve
before that fee is added; `confirm` then swaps to target = invoice + reserve + prepared input_fee, receives
exactly that target's denomination split, recomputes the actual input fee on that split, and refuses
(`melt/saga:709–712`) *after the swap has been paid* if the received proofs do not cover invoice + reserve
+ actual fee. Our fake never does the split/recompute/refuse, so two delivered "fitting" regressions
(`fee_remit.rs:4118` invoice 12, `:4290` invoice 14 — both at 1000 ppk, one 32-sat proof) assert successes
CDK would refuse; on the product path that refusal lands after the fence and leaves a bound Spending row
held. Separately, CDK `FinalizedMelt::fee_paid` = final proofs − invoice − change already *includes* the
actual melt-input fee, but `wallet_ops.rs:1498–1507` and `fee_remit.rs:1624–1657` add the prepared input
fee again — a double count that fabricates an above-gross "wallet lost 23" WARNING (true loss 19) and a
wrong fallback balance. Finally, prose in source, module doc, quickstart and body promises saga recovery on
wallet reopen (not called on this path), calls SDK cancel a guarantee (it is best-effort, swallows DB
errors), says "the mint learns nothing" at prepare (metadata GETs may occur), describes retired
`pay_melt_quote_*` as the current spend, and mis-states CI/green status.

## Scope checklist (addendum 9 §1–§5)

### §1 B/F1 — CDK fee model, confirmable before the fence
1. [ ] §1.1 Pre-fence confirmability check: from prepared figures + mint `input_fee_ppk`, compute
       actual_input_fee(split of target) and require target ≥ invoice+reserve+actual AND
       invoice+reserve+actual+swap_fee ≤ gross; else cancel prepared melt, row stays Planned, one line.
       Touches: `crates/maxplayer-core/src/fee_remit.rs` ~1480–1493 (prepare / pre-fence error path),
       `wallet_ops.rs` ~1432–1460 (prepared total read + check-cancel), `admits_total` `wallet_ops.rs:281–307`.
       Verify CDK arithmetic first against pinned crate: `melt/saga` 383–399 (estimate), 403 (swap_fee),
       678 (target), 704–712 (recompute/refuse); `swap/saga` 285–301 (target split). Cite lines in code comments.
2. [ ] §1.2 Fee-aware planning searches downward from gross for the largest invoice satisfying §1.1
       (gross 20, reserve 2, 1000 ppk, 32-proof → invoice 13 at 19, not 14); none fits → `FeesDoNotFit`.
       Touches: `fee_remit.rs` planner ~1213 and `FeesDoNotFit` ~509. Ceiling meaning unchanged.
3. [ ] §1.3 Fake becomes CDK-shaped on confirm: split swap output to target, charge swap fee even when
       melt then fails, recompute actual input fee on split, refuse when proofs don't cover, `fee_paid`
       inclusive per §2. Self-check test tests fake against THIS confirm model.
       Touches: `fee_remit.rs` fake layout ~2215–2283, confirm ~2876–2958 (esp. 2916 scripted fee, 2932
       prepared-input subtraction), self-check test ~4051.
4. [ ] §1.4 Regressions (real store, measured wallet value, swap AND melt counted):
       (i) fee-bearing success, prepared ≠ actual input fee, delta ≤ gross, swap once, melt once — incl.
       the `:4118` and `:4290` inputs with the invoice §1.2 now chooses;
       (ii) prepared fits but post-swap would not → refused BEFORE fence, delta 0, no swap/melt, row Planned;
       (iii) keep `:4187` fee-exceeds, `:4343` never-fits, `:4386` reserve 2→4 (2f). No expectation may
       accept a swap loss on a failed melt.
       Touches: `fee_remit.rs` tests ~4051–4400.
5. [ ] §1.5 Residual bound disclosed: fee metadata can change prepare→confirm, SDK takes no caller max.
       State in module doc `fee_remit.rs:58–80`, quickstart `docs/SELLER-QUICKSTART.md` ~1126–1134, body.

### §2 G/F2 — count inclusive `fee_paid` once
6. [ ] §2.1 Actual debit = invoice + fee_paid + swap_fee. Remove the added prepared input fee.
       Touches: `wallet_ops.rs:1498, 1504–1507`; `fee_remit.rs:1624–1628`, WARNING `:1646–1657`.
7. [ ] §2.2 Labels: Lightning reserve (quote) / actual aggregate melt fee (fee_paid) / estimated input fee
       (prepared) / actual swap fee. Reconciliation `fee = None` stays (`fee_remit.rs` ~1056).
       Touches: `wallet_ops.rs:235–241`, printed lines in `fee_remit.rs` ~1624–1657.
8. [ ] §2.3 Unused/fallback derive from correct actual debit; failed post-confirm balance read → print
       **unknown**, never a number. Touches: `wallet_ops.rs` ~1504–1507 fallback, `fee_remit.rs` ~1631.
9. [ ] §2.4 Regression: fee-bearing success, prepared ≠ actual input, failing observational balance
       read → no false overspend warning; residual correct or explicitly unknown. Touches: `fee_remit.rs` tests.

### §3 H/F3 — cancellation and recovery said truthfully (prose only, no recovery wiring)
10. [ ] §3.1 Strike "recovers on next open" at `wallet_ops.rs:431, 1443, 1458`; `fee_remit.rs:1543, 1593`.
        Name `recover_incomplete_sagas` as OWED; describe current state exactly.
11. [ ] §3.2 SDK cancel = best-effort local compensation (swallows/logs DB errors, returns Ok); credit
        Drop → Cancel → join (`wallet_ops.rs` ~446–456) as the product wrapper's. Fix `wallet_ops.rs:1442`.
12. [ ] §3.3 Replace "mint learns nothing"/"local only" at `wallet_ops.rs:1300` and kin with: prepare may
        fetch mint metadata/keysets; posts no proof-bearing or fee-bearing request.

### §4 H/F4 — stale and contradictory prose
13. [ ] §4.1 `lnurl_pay.rs:27`, `crates/maxplayer/src/seller_fees.rs:20, 1086`, `wallet_ops.rs:1098, 1897`:
        retired `pay_melt_quote_*` is no longer the current spend; live edge is
        `prepare_melt_payment_blocking → PreparedMeltPayment::confirm`. Retention stays.
14. [ ] §4.2 Body §196: strike the final-head-CI-not-required claim (addendum 8 §4 requires CI green on FINAL head).
15. [ ] §4.3 Body §10: withdraw "green" for the `2d802a3` money-path trio; keep §191 FAILED disclosures (2/3/2).
16. [ ] §4.4 After §1–§2 land: restate every fee/model/output claim in body, module doc `fee_remit.rs:58–80`,
        `wallet_ops.rs:235–241`, quickstart ~1126–1134 against the new code; re-check load-bearing citations
        against the new executable head; re-pin the body.

### §5 I — freshness by construction
17. [ ] Last `.rs` commit = executable head. Run THERE with `git rev-parse HEAD` as line 1 of each log:
        core `--features wallet`, CLI default, CLI `acp,wallet`; money-path CI command ×3 (report FAILED
        counts honestly); `fmt --check`; clippy mapped to whole `a6217328..head` added set; every `--exact`
        record (36 + new §1.4/§2.4 gates). Logs → `reports/w-remit-r8/`.
18. [ ] Only `docs/`, body, `reports/` commits after. State both heads; paste `git diff --stat exec..final`.
        CI green on FINAL head, stated at both.

### §6 Reporting
19. [ ] `pushed <sha>` every push; commit/push ≤ 5 min; no tool call past ~10 min. DONE/BLOCKED via
        `sessions_send` to `agent:maxie:discord:channel:1545487917575831684` AND hearth (addendum 3 §7 shape).

## Do NOT touch (credited; addendum 9 preamble + §7)
- PAID-only bound-Spending hold after confirm (`fee_remit.rs` 774–776, 1684–1693); exclusion at admission
  (store 2032–2043, 2101–2104); fresh fence (store 2194–2231); conditional releases; quote binding;
  spend order prepare → total bound → fence → confirm with local cancel; all seven 2g tests; the 36
  exact records' shape; reconciliation `fee = None` provenance.
- Out of scope, name as owed only: BudgetGate wiring; operator melt path (`melt_within_*` keeps
  reserve-only ceiling — say so); threshold sweep; signed parameter; rate change; owed-vs-held; operator
  force-release/human approval; legacy unbound rows; removal of `pay_melt_quote_*` / `TerminalBoundQuote`;
  wiring `recover_incomplete_sagas`; repo-wide lint debt; the three out-of-diff flakes; rebase-or-merge.
- No rebase, amend, force-push, new PR, `git config`. Never change an expectation to accept a swap loss on
  a failed melt. Never weaken the PAID-only hold.

## Order of work
Step 2 (next turn): verify CDK arithmetic against the pinned crate in `~/.cargo/registry` (saga 383–399,
403, 678, 704–712; swap/saga 285–301), then implement §1.3 fake confirm model first (so §1.1/§1.2 can be
tested against it), then §1.1, §1.2, §2.1–2.3, then tests §1.4/§2.4, then prose §3/§4, then §5 evidence.

## §5 evidence notes (added step 16, 2026-09-08)
- Executable head moved to `1a5c7c5` (step-14 lint fix touched fee_remit.rs); `b1c702a` logs (exec-*.log) superseded by exec2-*.log.
- Money-path CI command, exact (r7 body-da0ee92.md line 184; CI ci.yml:146 differs — r7 used the release/live-mints form and the advisor accepted it):
  `cargo test -p maxplayer-core --release --no-default-features --features gateway,git-delivery,wallet,live-mints --locked`
  → reports/w-remit-r8/money-path-ci-{1,2,3}-1a5c7c5.log, one run per turn (~160 s + release build). Report FAILED counts honestly; known flakes: job_lifecycle live mint, credential_proxy loopback.
- Exact-record names: line 3 of each reports/w-remit-r7/exact-*-24ba082.log; drop `a_fee_bearing_mint_with_a_swap_required_layout…` (renamed), add the five round-8 tests.
- Exact records (built step 21): reports/w-remit-r8/exact-records.txt, 38 lines `NN-tag|<exact cargo command>` — the 36 r7 records
  with each r7 log's own command (records 24/25 are `-p maxplayer --features acp,wallet --bin maxplayer`), record 28 re-pointed to the
  renamed test `a_fee_bearing_payment_whose_prepared_input_fee_differs_from_the_actual…`, plus 37 (post-swap refused before fence)
  and 38 (balance unknown, no false warning). Records 30 and 32 already cover the planner test and the fake self-check, so the
  round-8 additions are 2, not 5. Run as-is: `exact-NN-tag-1a5c7c5.log`, line 1 rev-parse, line 2 `$ <command>`.
