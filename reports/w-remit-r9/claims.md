# Round 9 — adversarial re-verification of the seven failed claim groups (verdict at 4714623 §6 / adversarial-semantic-audit)

Executable head `240240c3f26bd35c3a8c5a3d70229f9730a0a221`; body `reports/w-remit-r9/body-r9.md` at commit `065fc5a`
(citation self-check 0 misses). Method: for each group the verdict failed, the body sentence NOW (body line number),
the source line NOW (`grep -n` at 240240c, quoted), and PASS/FAIL on whether the sentence asserts what the line does —
a text match alone is not a pass (verdict §6 "Discharge H"). Done by hand, 2026-09-08. CDK = pinned
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/cdk-0.17.2/src/wallet/`.

## 1. `from_prepared` "sets swap_fee to zero when no swap is needed" (verdict N4 item 6)
- Body 46: "`from_prepared` (`:574–596`) … sets `swap_fee: Amount::ZERO` **unconditionally** (`:592`) while still carrying
  `proofs_to_swap` (`:591`) — round 8's 'sets it to zero when no swap is needed' was false (N4 item 6) … this delivery
  treats no `swap_fee()` reading as a charged-fee promise". Body 64 (6): "`from_prepared` zeroes `swap_fee` unconditionally".
- Source: `melt/saga/mod.rs:574 pub fn from_prepared(` … `:591 proofs_to_swap,` … `:592 swap_fee: Amount::ZERO,` — no branch.
- **PASS.** The conditional claim is gone; the body states the unconditional zero and names the round-8 error.

## 2. Post-swap check "before prepare" (verdict N4 item 5)
- Body 46: "the payment quote is pre-checked **reserve-only** (`admits`, `:1555`); … re-planned once … then prepare
  (`:1764`); then the post-swap arithmetic (`:1785`) — **after** prepare, **before** the fence (`:1820`)"; body 16 spend
  order "quote → admits (:1555) → re-plan (:1692) → margin (:1741) → prepare (:1764) → confirm_would_succeed (:1785) →
  after_quote (:1812) → fence (:1820) → pay-time margin (:1887) → confirm (:1908)".
- Source `fee_remit.rs`: `1555 if !ceiling.admits(quote.amount_sats, quote.fee_reserve_sats) {` · `1764 let prepared =
  match effects.prepare_melt(&quote.quote_id, &ceiling) {` · `1785 let actual_input_fee_sats = match
  confirm_would_succeed(&preparation, gross) {` · `1820 .admit_remittance_spend(`. 1555 < 1764 < 1785 < 1820.
- **PASS.** Reserve-only precheck, then prepare, then arithmetic, then fence — as the source orders it.

## 3. "Exactly two cancel sites" (verdict N4 item 1)
- Body 15: "**three** `effects.cancel_melt(` sites (`:1788` arithmetic refused, `:1841` fence lost, `:1892` pay-time
  margin) … the census is of `effects.cancel_melt(` call sites in `remit_inner`: three, no other." Body 64 (1) same.
- Source: `grep -n "effects.cancel_melt(" fee_remit.rs` → exactly `1788`, `1841`, `1892`; `remit_inner` spans
  `1033`–`2006` (next item `2007 fn lease_secs`), so all three are inside it and there is no fourth anywhere in the file.
- **PASS.**

## 4. Prepare is "local only" / cache-only (verdict N4 item 2)
- Body 15: prepare "**may GET** the mint's keysets and info when the cache is unpopulated or past its TTL
  (`keysets.rs:44–51` → `mint_metadata_cache.rs:366`: cache → database → `load_from_mint`); the round-8 body's
  '**local only** … nothing is posted to the mint' was false in that respect and is withdrawn". Body 50: "prepare may GET
  mint metadata and keysets and posts no proof-bearing or fee-bearing request" + "Round 9 correction (N4 item 2)" naming
  the round-8 §5 line and its false "every phrase replaced" claim. `grep -c "cache-only\|cache only" body-r9.md` = 0.
- Source: `melt/saga/mod.rs:303 .get_mint_keysets(KeysetFilter::Active)` → `keysets.rs:44 pub async fn get_mint_keysets(`
  `:46 .metadata_cache` `:47 .load(&self.localstore, &self.client, {` → `mint_metadata_cache.rs:366 pub async fn load(`
  (`:363` doc: "Use cached data if available, fetch if not").
- **PASS.** The body's bound is now "no proof-bearing or fee-bearing request", not "no mint contact".

## 5. Record 28 "prepared 2 ≠ actual 3" (verdict N4 item 3)
- Body 16: "(i) pool 32 → 15, delta 17 ≤ 20 with prepared input fee **4** ≠ actual 3 (record 28, `:4877`; the round-8
  body said 2 here and 4 in its round-8 section — N4 item 3)". Body 46 (4)(i): "prepared input fee **4** (estimate) ≠
  actual **3**: target 19 = 12 + 3 + 4 … delta 17 = 12 + 1 + 3 + 1 ≤ 20". Body 64 (3) same.
- Source `fee_remit.rs:4877` test `a_fee_bearing_payment_whose_prepared_input_fee_differs_from_the_actual_pays_once…`;
  `:4908` asserts `"Prepared melt of quote paid-quote-lnbc-fake-12-2: proof input fee 4 sats (estimate; actual on the
  swapped proofs 3 sats)"`; `:4912` comment "the prepared estimate (4) is printed, not added — debit 12 + 4 + 1 = 17";
  `:4916` `"actual debit: 17 sats = net + melt fee + swap fee"`; `:4949` `Some(4)` = inclusive fee_paid.
- **PASS.** Both body sites say 4; the contradiction is gone.

## 6. One printed line / row stays Planned on a pre-fence refusal (verdict N4 item 4, §3.3)
- Body 16: "`refuse_before_fence` (`:1525`) runs: **one** printed line (`:1538` …), attempt journaled `Failed`, and the
  row **stays Planned** — unbound, owner and lease intact, receipts pinned, nothing written to it". Body 46 (ii): record 37
  "Refused **inside prepare** by the wallet's own `admits_confirmable` (`wallet_ops.rs:1733`; `prepared.cancel()` first
  `:1782`) … `cancels == []` … `ceiling_refusals.len() == 1`, `swaps == []`, `melts == []`, pool 32 unchanged, quote
  UNPAID, exactly **one** `REFUSED before spending` line (`fee_remit.rs:1538`) … row **stays Planned**
  (`assert_refused_before_fence_row_stays_planned` `:3595`)"; prepared total **16** and confirm target **15** named apart.
- Source: `fee_remit.rs:1536–1539` one `writeln!` whose format string is a single line ("REFUSED before spending —
  {reason}; ceiling {gross} sats; nothing left the wallet; the row stays planned and the next attempt re-quotes."), no
  `\n` inside; `1525 let refuse_before_fence`. Test `:5284`: `:5288 fake.prepared_input_override = Some(0)`;
  `:5308 fake.swaps.is_empty()`, `:5309 fake.melts.is_empty()`, `:5311 fake.cancels.is_empty()`, `:5316
  fake.ceiling_refusals.len()` == 1, `:5337 out.matches("REFUSED before spending").count()` == 1 ("exactly one refusal
  line"), `:5342 !out.contains("Prepared melt of quote")`, `:5359 assert_refused_before_fence_row_stays_planned(…)`.
  `wallet_ops.rs:1733 ceiling.admits_confirmable(`, `:1782 prepared.cancel()`.
- **PASS**, with one note: the "exactly one line" assertion still counts occurrences of the phrase (`matches(…).count()`),
  as the verdict criticised at 4714623; it is sound now only because the format string at `:1538` is one line and the
  `{reason}` text (`:5303`) carries no newline. Not a body defect; a stricter `out.lines()` assertion is owed work.

## 7. The blanket "all claims correct" closure (verdict §6 H, "adversarial semantic sample")
- Body: `grep -n "all claims\|every claim\|claims correct\|no semantic\|all correct"` → 0 hits. The closure now reads
  (body 64): "Every citation in those sentences was re-read at `240240c` by hand while writing, and the whole body is
  re-anchored and read back below (`citation-remap-240240c.txt`, `citation-selfcheck-240240c.txt`), then re-pinned."
  Body 52 makes the same citation-level statement.
- **PASS** as a claim about citations, which is all it now claims; this file is the semantic sample the verdict said a
  zero-offset self-check cannot replace. Caveat: "then re-pinned" is true only after the pin step, which has not run.

## Residual (source comments, not body; adjacent work named, not done — the step said never the source)
Three test comments in `fee_remit.rs` still describe the round-8 release behaviour while the tests beneath them assert
the round-9 contract: `:5280` and `:5381` "the row released (Planned → Failed, receipts back)" and `:5617` "released as a
planned row of our own" — each test asserts `assert_refused_before_fence_row_stays_planned` (`:5359`, `:5648`). Comments
only; no behaviour claim in the body rests on them.

**Result: 7 / 7 PASS; body unchanged by this pass (self-check re-run below stays at 0 misses).**
