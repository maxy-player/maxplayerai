#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Step 26c: self-check every product-file citation in body-r8-final-draft.md against the tree at
1a5c7c5 (the worktree's .rs files are identical to 1a5c7c5 — asserted first). For every
`file.rs:NNNN` / bare `:NNNN` (file context = last product file named earlier in the same body line,
DEFAULTS for the two paragraphs with implicit context) print `file:line: <code>`; for the
load-bearing citations assert a NEEDLE is on that line. Output citation-selfcheck-1a5c7c5.txt;
exit non-zero on any needle miss."""
import re, subprocess, sys, pathlib
W = pathlib.Path("/Users/forge/forge/v2/wt/w-seller-fee-stage2a-remit")
R8 = W / "reports/w-remit-r8"
FILES = {
    "fee_remit.rs": "crates/maxplayer-core/src/fee_remit.rs",
    "wallet_ops.rs": "crates/maxplayer-core/src/wallet_ops.rs",
    "seller_fees.rs": "crates/maxplayer/src/seller_fees.rs",
    "run.rs": "crates/maxplayer-core/src/seller_node/run.rs",
    "store.rs": "crates/maxplayer-core/src/seller_node/store.rs",
    "home.rs": "crates/maxplayer-core/src/home.rs",
    "platform_fee.rs": "crates/maxplayer-core/src/platform_fee.rs",
    "lnurl_pay.rs": "crates/maxplayer-core/src/lnurl_pay.rs",
    "sell.rs": "crates/maxplayer/src/sell.rs",
    "lib.rs": "crates/maxplayer-core/src/lib.rs",
}
d = subprocess.run(["git", "diff", "--stat", "1a5c7c5", "HEAD", "--", "*.rs"], cwd=W, capture_output=True, text=True).stdout
if d.strip():
    sys.exit("worktree .rs files differ from 1a5c7c5:\n" + d)
SRC = {k: (W / v).read_text().split("\n") for k, v in FILES.items()}
DEFAULTS = {32: "fee_remit.rs", 38: "wallet_ops.rs"}
CDK_LEAVE = {":817–831", ":831–887", ":687–697", ":907–911", ":704–712", ":706–712", ":704", ":678",
             ":403", ":574–595", ":136–148", ":817–830", ":383–399", ":709–712"}
# load-bearing citations: (file, line) -> needle that must be on that line
NEEDLES = {
    ("fee_remit.rs", 931): "pub fn remit(", ("fee_remit.rs", 1030): "fn remit_inner(",
    ("fee_remit.rs", 335): "fn prepare_melt(", ("fee_remit.rs", 343): "fn confirm_melt(", ("fee_remit.rs", 347): "fn cancel_melt(",
    ("fee_remit.rs", 440): "fn prepare_melt(", ("fee_remit.rs", 460): "fn confirm_melt(", ("fee_remit.rs", 472): "fn cancel_melt(",
    ("fee_remit.rs", 545): "FeesDoNotFit {", ("fee_remit.rs", 773): "fn reconcile_decision(",
    ("fee_remit.rs", 1325): "FeesDoNotFit", ("fee_remit.rs", 1358): "FeesDoNotFit",
    ("fee_remit.rs", 1576): "effects.prepare_melt(", ("fee_remit.rs", 1597): "confirm_would_succeed(&preparation",
    ("fee_remit.rs", 1600): "effects.cancel_melt(prepared)", ("fee_remit.rs", 1653): "effects.cancel_melt(prepared)",
    ("fee_remit.rs", 1704): "effects.cancel_melt(prepared)", ("fee_remit.rs", 1720): "effects.confirm_melt(prepared)",
    ("fee_remit.rs", 1693): "3. Confirm the prepared melt", ("fee_remit.rs", 1753): "PAID — remittance",
    ("fee_remit.rs", 2231): "fn fee_for(", ("fee_remit.rs", 2238): "fn binary_split(", ("fee_remit.rs", 2251): "fn post_swap_figures(",
    ("fee_remit.rs", 2272): "fn plan_confirmable_invoice(", ("fee_remit.rs", 2305): "fn confirm_would_succeed(",
    ("fee_remit.rs", 2322): "melt refused before spending", ("fee_remit.rs", 2461): "struct FakeSwap",
    ("fee_remit.rs", 2636): "swaps: Vec<FakeSwap>", ("fee_remit.rs", 3167): "fn confirm_melt(",
    ("fee_remit.rs", 4405): "fn the_fake_wallet_models_the_sdks", ("fee_remit.rs", 4620): "fn a_fee_bearing_payment_whose_prepared_input_fee",
    ("fee_remit.rs", 4712): "fn a_failed_balance_read_after", ("fee_remit.rs", 4762): "fn a_fee_bearing_schedule_whose_prepared_figures",
    ("fee_remit.rs", 4781): "melt refused before spending", ("fee_remit.rs", 4871): "fn a_fee_bearing_total_that_exceeds",
    ("fee_remit.rs", 4978): "fn fee_aware_planning_sizes", ("fee_remit.rs", 5046): "fn fees_that_can_never_fit",
    ("fee_remit.rs", 5089): "fn a_reserve_that_grows", ("fee_remit.rs", 4632): "maxplayer@agi.cash",
    ("wallet_ops.rs", 66): "MeltTotalExceedsCeiling {", ("wallet_ops.rs", 111): "MeltTotalExceedsCeiling {",
    ("wallet_ops.rs", 238): "balance_after_sats", ("wallet_ops.rs", 270): "pub struct MeltCeiling",
    ("wallet_ops.rs", 284): "pub fn admits(", ("wallet_ops.rs", 294): "pub fn admits_total(", ("wallet_ops.rs", 311): "pub fn total_debit(",
    ("wallet_ops.rs", 368): "pub struct MeltPreparation", ("wallet_ops.rs", 384): "input_fee_ppk",
    ("wallet_ops.rs", 408): "pub struct PreparedMeltPayment", ("wallet_ops.rs", 972): "pub async fn send_async(",
    ("wallet_ops.rs", 1025): "pub async fn receive_async(", ("wallet_ops.rs", 1086): "pub async fn melt_async(",
    ("wallet_ops.rs", 1105): "pub async fn melt_within_async(", ("wallet_ops.rs", 1153): "pub async fn pay_melt_quote_async(",
    ("wallet_ops.rs", 1262): "fn expected_melt_fees(", ("wallet_ops.rs", 1333): "fn active_keyset_input_fee_ppk(",
    ("wallet_ops.rs", 1377): "pub fn prepare_melt_payment_blocking(", ("wallet_ops.rs", 1390): "mint_allowed(",
    ("wallet_ops.rs", 1437): "fn prepared_melt_thread(", ("wallet_ops.rs", 1479): "MeltExceedsCeiling",
    ("wallet_ops.rs", 1515): "MeltTotalExceedsCeiling", ("wallet_ops.rs", 1580): ".confirm()",
    ("wallet_ops.rs", 1595): "let spent", ("wallet_ops.rs", 1613): "cancel()", ("wallet_ops.rs", 1572): "cancel()",
    ("wallet_ops.rs", 1623): "pub async fn melt_quote_async(", ("wallet_ops.rs", 1678): "pub async fn melt_status_for_quote_async(",
    ("wallet_ops.rs", 1720): "pub async fn melt_status_for_invoice_async(", ("wallet_ops.rs", 2009): "pub fn pay_melt_quote_blocking(",
    ("seller_fees.rs", 110): '"--confirm"', ("seller_fees.rs", 413): "let trigger = if confirm", ("seller_fees.rs", 418): "remit(&store",
    ("run.rs", 3321): "remit_best_effort", ("run.rs", 3323): "remit_live_best_effort", ("run.rs", 3666): "fn drain_remit_in_flight",
    ("run.rs", 4159): "remit_retry", ("run.rs", 13712): "fn a_failed_remittance_leaves_the_receipt",
    ("store.rs", 2083): "fn plan_remittance", ("store.rs", 2186): "fn admit_remittance_spend", ("store.rs", 2338): "fn release_remittance",
    ("platform_fee.rs", 68): "PLATFORM_FEE_ADDRESS", ("home.rs", 1546): "fn default_allow_real_mints",
    ("fee_remit.rs", 160): "two boundaries", ("fee_remit.rs", 184): "What each shares", ("fee_remit.rs", 2672): "melt_gate",
}
body = (R8 / "body-r8-final-draft.md").read_text().split("\n")
cite = re.compile(r"(?P<file>[a-z_]+\.rs)?:(?P<a>\d{2,5})(?:[–-](?P<b>\d{2,5}))?(?![\d])")
rows, misses, seen = [], [], set()
for lineno, text in enumerate(body, 1):
    cur = DEFAULTS.get(lineno)
    for m in cite.finditer(text):
        f = m.group("file"); tok = m.group(0)
        before = text[max(0, m.start() - 12):m.start()]
        if re.search(r"\d$", before) and not f:
            continue
        if f and f not in FILES:
            continue
        if not f and tok in CDK_LEAVE:
            continue
        if f:
            cur = f
        if cur is None:
            rows.append(f"body {lineno:4d}  {tok:<22} -> NO FILE CONTEXT"); continue
        a = int(m.group("a"))
        src = SRC[cur]
        code = src[a - 1].strip() if 0 < a <= len(src) else "<OUT OF RANGE>"
        rows.append(f"body {lineno:4d}  {cur}:{a:<6} {code[:110]}")
        seen.add((cur, a))
for (f, n), needle in sorted(NEEDLES.items()):
    code = SRC[f][n - 1] if 0 < n <= len(SRC[f]) else ""
    ok = needle in code
    rows.append(f"NEEDLE {f}:{n} {'ok ' if ok else 'MISS'} {'(cited)' if (f, n) in seen else '(not cited in body)'} {needle!r} -> {code.strip()[:90]}")
    if not ok:
        misses.append((f, n, needle))
out = R8 / "citation-selfcheck-1a5c7c5.txt"
out.write_text(f"self-check of body-r8-final-draft.md citations at 1a5c7c5 ({len(seen)} distinct file:line cited; {len(NEEDLES)} needles; {len(misses)} misses)\n" + "\n".join(rows) + "\n")
# inventory in the round-7 format: file: then line:snippet, for every distinct product citation
inv = ["citations at 1a5c7c5 (rs) / <final> (docs) — every product file:line cited by body-r8-final-draft.md, with the source line"]
for f in FILES:
    ns = sorted(n for (ff, n) in seen if ff == f)
    if not ns:
        continue
    inv.append(f"{f}:")
    for n in ns:
        src = SRC[f]
        inv.append(f"{n}:{src[n - 1].strip()[:120] if 0 < n <= len(src) else '<OUT OF RANGE>'}")
(R8 / "body-citation-inventory-r8.txt").write_text("\n".join(inv) + "\n")
print(f"cited {len(seen)} distinct; needles {len(NEEDLES)}; misses {len(misses)}")
for m in misses:
    print("MISS", m)
sys.exit(1 if misses else 0)
