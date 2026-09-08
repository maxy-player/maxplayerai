#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Round 9 (plan §6 step 8): self-check every product-file citation in body-r9.md against the tree at
240240c (the worktree's .rs files are identical to 240240c — asserted first). For every
`file.rs:NNNN` / bare `:NNNN` (file context = last product file named earlier in the same body line,
DEFAULTS for the paragraphs with implicit context) print `file:line: <code>`; for the load-bearing
citations assert a NEEDLE is on that line. Output citation-selfcheck-240240c.txt and
body-citation-inventory-r9.txt; exit non-zero on any needle miss. Lineage: reports/w-remit-r8/selfcheck-r8.py."""
import re, subprocess, sys, pathlib
W = pathlib.Path("/Users/forge/forge/v2/wt/w-seller-fee-stage2a-remit")
R9 = W / "reports/w-remit-r9"
EXEC = "240240c"
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
d = subprocess.run(["git", "diff", "--stat", EXEC, "HEAD", "--", "*.rs"], cwd=W, capture_output=True, text=True).stdout
if d.strip():
    sys.exit(f"worktree .rs files differ from {EXEC}:\n" + d)
SRC = {k: (W / v).read_text().split("\n") for k, v in FILES.items()}
# implicit file context: 34 = Rounds 5–6 two-process paragraph; 40 = Round-7 B4 paragraph; 64 = N4 list (§5 cancel sites);
# 81–84 = gate-1 grep block (wallet_ops); 93 = gate-2a boundaries (fee_remit first #[cfg(test)])
DEFAULTS = {34: "fee_remit.rs", 40: "wallet_ops.rs", 64: "fee_remit.rs", 81: "wallet_ops.rs", 82: "wallet_ops.rs",
            83: "wallet_ops.rs", 84: "wallet_ops.rs", 93: "fee_remit.rs", 234: "wallet_ops.rs"}
# bare CDK 0.17.2 citations (pinned crate, not product lines) and log-line / CI-file tokens
CDK_LEAVE = {":817–831", ":831–887", ":687–697", ":907–911", ":704–712", ":706–712", ":704", ":678", ":403",
             ":574–595", ":574–596", ":592", ":591", ":136–148", ":139–148", ":817–830", ":383–399", ":709–712",
             ":285–301", ":286–460", ":428–450", ":673", ":515", ":44–51", ":366", ":35–48", ":15559", ":17463",
             ":17476", ":233", ":220", ":14871"}
# body lines whose product citations are HISTORICAL by their own text (verdict citations at 24ba082 / da0ee92) — inventoried, not needled
HISTORICAL = {(48, "wallet_ops.rs:1498"), (48, ":1504–1507"), (48, "fee_remit.rs:1624–1628"), (48, ":1646–1657"),
              (52, "lnurl_pay.rs:27"), (52, "seller_fees.rs:20"), (52, ":1086"), (52, ":1126–1153"), (234, "wallet_ops.rs:903")}
# load-bearing citations: (file, line) -> needle that must be on that line at 240240c
NEEDLES = {
    # fee_remit.rs — remit, trait, impl, spend order
    ("fee_remit.rs", 934): "pub fn remit(", ("fee_remit.rs", 1033): "fn remit_inner(",
    ("fee_remit.rs", 314): "pub trait RemitEffects", ("fee_remit.rs", 338): "fn prepare_melt(",
    ("fee_remit.rs", 346): "fn confirm_melt(", ("fee_remit.rs", 350): "fn cancel_melt(",
    ("fee_remit.rs", 422): "impl RemitEffects for LiveEffects", ("fee_remit.rs", 443): "fn prepare_melt(",
    ("fee_remit.rs", 463): "fn confirm_melt(", ("fee_remit.rs", 475): "fn cancel_melt(",
    ("fee_remit.rs", 1475): "let ceiling = MeltCeiling", ("fee_remit.rs", 1481): "let release_own_planned",
    ("fee_remit.rs", 1525): "let refuse_before_fence", ("fee_remit.rs", 1538): "REFUSED before spending — {reason}; ceiling {gross} sats; nothing left the wallet; the row stays planned",
    ("fee_remit.rs", 1555): "ceiling.admits(quote.amount_sats", ("fee_remit.rs", 1692): ".replan_remittance(",
    ("fee_remit.rs", 1715): "Re-planned: the payment quote's fee reserve", ("fee_remit.rs", 1741): "quote_inside_margin(quote_now_unix)",
    ("fee_remit.rs", 1764): "effects.prepare_melt(", ("fee_remit.rs", 1785): "confirm_would_succeed(&preparation",
    ("fee_remit.rs", 1788): "effects.cancel_melt(prepared)", ("fee_remit.rs", 1812): "effects.after_quote(",
    ("fee_remit.rs", 1820): ".admit_remittance_spend(", ("fee_remit.rs", 1833): "let admit_now_unix = admit_now_unix.unwrap_or",
    ("fee_remit.rs", 1836): "Err(lost) =>", ("fee_remit.rs", 1841): "effects.cancel_melt(prepared)",
    ("fee_remit.rs", 1858): "REFUSED before spending — {reason} (checked at unix", ("fee_remit.rs", 1868): "OwnershipLost",
    ("fee_remit.rs", 1881): "3. Confirm the prepared melt", ("fee_remit.rs", 1885): "fee-bearing request was posted",
    ("fee_remit.rs", 1887): "quote_inside_margin(pay_now_unix)", ("fee_remit.rs", 1892): "effects.cancel_melt(prepared)",
    ("fee_remit.rs", 1900): "not paid: {error}", ("fee_remit.rs", 1904): "planned.remittance_id",
    ("fee_remit.rs", 1908): "effects.confirm_melt(prepared)", ("fee_remit.rs", 1941): "PAID — remittance",
    # fee_remit.rs — refusals, reconcile, planning, constants
    ("fee_remit.rs", 548): "FeesDoNotFit {", ("fee_remit.rs", 575): "SpendingHeld {", ("fee_remit.rs", 597): "RowChangedUnderMe {",
    ("fee_remit.rs", 653): "Self::SpendingHeld {", ("fee_remit.rs", 776): "pub fn reconcile_decision(",
    ("fee_remit.rs", 790): "return Reconcile::Settle", ("fee_remit.rs", 800): "Reconcile::Hold(Refusal::SpendingHeld",
    ("fee_remit.rs", 809): "A bound spending row: held on everything but PAID", ("fee_remit.rs", 825): "Reconcile::Hold(Refusal::Settling",
    ("fee_remit.rs", 1105): "Lightning fee at most", ("fee_remit.rs", 1135): "Refusal::RowChangedUnderMe",
    ("fee_remit.rs", 1272): "Refusal::FeesDoNotFit", ("fee_remit.rs", 1354): "Refusal::FeesDoNotFit", ("fee_remit.rs", 1370): "Refusal::FeesDoNotFit",
    ("fee_remit.rs", 1386): "mint melt fee reserve (bounds the Lightning fee)", ("fee_remit.rs", 1392): "expected proof fees (SDK estimate",
    ("fee_remit.rs", 1430): "store.plan_remittance(", ("fee_remit.rs", 228): "pub const REMIT_LEASE", ("fee_remit.rs", 237): "pub const SPEND_MARGIN",
    ("fee_remit.rs", 514): "pub fn pays(", ("fee_remit.rs", 2123): "pub fn remit_best_effort(", ("fee_remit.rs", 2146): "pub fn remit_live_best_effort(",
    ("fee_remit.rs", 2153): "remit_best_effort(store, &mut effects", ("fee_remit.rs", 2431): "fn plan_confirmable_invoice(",
    ("fee_remit.rs", 2470): "fn confirm_would_succeed(", ("fee_remit.rs", 2517): "pub(crate) mod test_support", ("fee_remit.rs", 3577): "mod tests",
    ("fee_remit.rs", 272): "#[cfg(test)]", ("fee_remit.rs", 160): "at most one debit", ("fee_remit.rs", 186): "What each shares",
    # fee_remit.rs — fake and tests
    ("fee_remit.rs", 2628): "struct FakeSwap", ("fee_remit.rs", 2801): "prepared_input_override", ("fee_remit.rs", 2809): "swaps: Vec<FakeSwap>",
    ("fee_remit.rs", 2845): "melt_gate", ("fee_remit.rs", 3380): "fn confirm_melt(", ("fee_remit.rs", 3595): "fn assert_refused_before_fence_row_stays_planned",
    ("fee_remit.rs", 4067): "fn interrupted_after_the_plan_is_reconciled_as_paid_without_a_second_melt",
    ("fee_remit.rs", 4212): "fn an_unpaid_or_pending_interrupted_attempt", ("fee_remit.rs", 4668): "fn the_fake_wallet_models_the_sdks",
    ("fee_remit.rs", 4877): "fn a_fee_bearing_payment_whose_prepared_input_fee_differs", ("fee_remit.rs", 4969): "fn a_failed_balance_read_after",
    ("fee_remit.rs", 5019): "fn a_reserve_that_shrinks_between_estimate_and_payment_is_re_planned_once",
    ("fee_remit.rs", 5155): "fn a_fee_bearing_payment_at_a_19_sat_gross", ("fee_remit.rs", 5284): "fn a_fee_bearing_schedule_whose_prepared_figures",
    ("fee_remit.rs", 5303): "melt refused before spending: the wallet would swap to 15 sats", ("fee_remit.rs", 5384): "fn a_fee_bearing_total_that_exceeds",
    ("fee_remit.rs", 5482): "fn fee_aware_planning_sizes", ("fee_remit.rs", 5550): "fn fees_that_can_never_fit", ("fee_remit.rs", 5593): "fn a_reserve_that_grows",
    ("fee_remit.rs", 5926): "fn a_spending_row_is_never_released_by_reconciliation_only_settled",
    ("fee_remit.rs", 6120): "fn a_second_process_cannot_release_a_live_owners_planned_row", ("fee_remit.rs", 6219): "fn a_spending_row_is_not_released_when_its_lease_expires",
    ("fee_remit.rs", 6343): "fn an_owner_that_outlives_its_lease", ("fee_remit.rs", 6467): "fn an_owner_whose_lease_ran_down",
    ("fee_remit.rs", 6574): "fn a_spending_row_is_reconciled_by_its_bound_quote", ("fee_remit.rs", 6709): "fn a_bound_quote_expired_past_the_margin",
    ("fee_remit.rs", 6882): "fn an_owner_paused_after_its_quote", ("fee_remit.rs", 7031): "fn a_release_decided_on_a_stale_planned_snapshot",
    ("fee_remit.rs", 7188): "fn a_spending_rows_bound_quote_decides", ("fee_remit.rs", 7459): "fn a_payment_prepared_before_expiry_cannot_be_doubled",
    # wallet_ops.rs
    ("wallet_ops.rs", 56): "MeltExceedsCeiling {", ("wallet_ops.rs", 73): "MeltTotalExceedsCeiling {", ("wallet_ops.rs", 89): "MeltWouldNotConfirm {",
    ("wallet_ops.rs", 274): "pub struct MeltOutcome", ("wallet_ops.rs", 288): "pub balance_after_sats", ("wallet_ops.rs", 292): "The mint's fee RESERVE on the paying quote",
    ("wallet_ops.rs", 295): "the reserve bounds the Lightning component only", ("wallet_ops.rs", 311): "fn fee_for(", ("wallet_ops.rs", 318): "fn binary_split(",
    ("wallet_ops.rs", 331): "fn post_swap_figures(", ("wallet_ops.rs", 348): "pub struct ConfirmBound", ("wallet_ops.rs", 396): "pub fn confirm_bound(",
    ("wallet_ops.rs", 454): "pub struct MeltCeiling", ("wallet_ops.rs", 468): "pub fn admits(", ("wallet_ops.rs", 479): "pub fn admits_confirmable(",
    ("wallet_ops.rs", 507): "pub fn total_debit(", ("wallet_ops.rs", 528): "The mint's fee RESERVE for this melt", ("wallet_ops.rs", 531): "which [`confirm_bound`] accounts for separately",
    ("wallet_ops.rs", 566): "pub struct MeltPreparation", ("wallet_ops.rs", 582): "pub input_fee_ppk", ("wallet_ops.rs", 606): "pub struct PreparedMeltPayment",
    ("wallet_ops.rs", 629): "pub fn confirm(", ("wallet_ops.rs", 645): "pub fn cancel(", ("wallet_ops.rs", 676): "impl Drop for PreparedMeltPayment",
    ("wallet_ops.rs", 1171): "pub async fn send_async(", ("wallet_ops.rs", 1183): "mint_allowed(", ("wallet_ops.rs", 1224): "pub async fn receive_async(",
    ("wallet_ops.rs", 1243): "mint_allowed(", ("wallet_ops.rs", 1285): "pub async fn melt_async(", ("wallet_ops.rs", 1304): "pub async fn melt_within_async(",
    ("wallet_ops.rs", 1318): "mint_allowed(", ("wallet_ops.rs", 1352): "pub async fn pay_melt_quote_async(", ("wallet_ops.rs", 1363): "mint_allowed(",
    ("wallet_ops.rs", 1396): "ceiling.admits(invoice_sats, fee_reserve_sats)", ("wallet_ops.rs", 1419): ".prepare_melt(&quote.id",
    ("wallet_ops.rs", 1427): ".confirm()", ("wallet_ops.rs", 1533): "fn active_keyset_input_fee_ppk(", ("wallet_ops.rs", 1577): "pub fn prepare_melt_payment_blocking(",
    ("wallet_ops.rs", 1590): "mint_allowed(", ("wallet_ops.rs", 1637): "fn prepared_melt_thread(", ("wallet_ops.rs", 1678): "ceiling.admits(invoice_sats, fee_reserve_sats)",
    ("wallet_ops.rs", 1709): "active_keyset_input_fee_ppk(&wallet)", ("wallet_ops.rs", 1716): "prepared.cancel()", ("wallet_ops.rs", 1733): "ceiling.admits_confirmable(",
    ("wallet_ops.rs", 1747): "WalletOpsError::MeltExceedsCeiling", ("wallet_ops.rs", 1756): "WalletOpsError::MeltWouldNotConfirm",
    ("wallet_ops.rs", 1771): "WalletOpsError::MeltTotalExceedsCeiling", ("wallet_ops.rs", 1782): "prepared.cancel()", ("wallet_ops.rs", 1837): "let spent",
    ("wallet_ops.rs", 1865): "pub async fn melt_quote_async(", ("wallet_ops.rs", 1875): "mint_allowed(", ("wallet_ops.rs", 1920): "pub async fn melt_status_for_quote_async(",
    ("wallet_ops.rs", 1930): "mint_allowed(", ("wallet_ops.rs", 1962): "pub async fn melt_status_for_invoice_async(", ("wallet_ops.rs", 1969): "mint_allowed(",
    ("wallet_ops.rs", 2155): 'refuse_nested_block_on("send_blocking")', ("wallet_ops.rs", 2182): 'refuse_nested_block_on("melt_blocking")',
    ("wallet_ops.rs", 2256): "pub fn pay_melt_quote_blocking(", ("wallet_ops.rs", 1102): "if ",
    # store.rs
    ("store.rs", 47): "pub const SCHEMA_VERSION", ("store.rs", 372): "TerminalBoundQuote { quote_id: String }", ("store.rs", 1075): "fee_remittances_one_planned",
    ("store.rs", 2101): "pub fn plan_remittance(", ("store.rs", 2121): "in_flight_remittance_in(", ("store.rs", 2122): "PlanRefused::InFlight",
    ("store.rs", 2180): "UPDATE fee_remittances SET spending_since_unix = :now", ("store.rs", 2199): "Once admitted the row is HELD until the mint reports the bound quote PAID",
    ("store.rs", 2202): "quote holds the row too", ("store.rs", 2206): "pub fn admit_remittance_spend(", ("store.rs", 2287): "pub fn replan_remittance(",
    ("store.rs", 2354): "pub fn settle_remittance(", ("store.rs", 2437): "pub fn release_remittance(", ("store.rs", 2448): "ReleaseOn::TerminalBoundQuote",
    ("store.rs", 2474): "AND owner = ?5", ("store.rs", 2182): "lease_until_unix > :now + :margin", ("store.rs", 4593): "fn the_spend_fence_admits_only_the_owner",
    # run.rs / seller_fees.rs / platform_fee.rs / home.rs
    ("run.rs", 617): "#[cfg(test)]", ("run.rs", 3312): "fn run_remit_attempt(", ("run.rs", 3321): "remit_best_effort", ("run.rs", 3323): "remit_live_best_effort",
    ("run.rs", 3666): "fn drain_remit_in_flight", ("run.rs", 3686): "SPENDING", ("run.rs", 4033): "serve().await", ("run.rs", 4041): "drain_remit_in_flight().await",
    ("run.rs", 4154): "auto_remit", ("run.rs", 4159): "remit_retry", ("run.rs", 4166): "automatic remittance is OFF", ("run.rs", 4235): "remit_retry",
    ("run.rs", 4260): "remit_pacing_changed", ("run.rs", 7210): "remit_follows_collect(", ("run.rs", 7211): "remit_platform_fee_after_collect(",
    ("run.rs", 7239): "fn remit_platform_fee_after_collect(", ("run.rs", 7240): "auto_remit", ("run.rs", 7246): "remit_closed", ("run.rs", 7264): "std::thread::Builder",
    ("run.rs", 7316): "fn start_retry_remit(", ("run.rs", 7319): "remit_closed", ("run.rs", 7327): "std::thread::Builder", ("run.rs", 7937): "mod tests",
    ("run.rs", 13712): "fn a_failed_remittance_leaves_the_receipt",
    ("seller_fees.rs", 38): "REFUSED", ("seller_fees.rs", 110): '"--confirm"', ("seller_fees.rs", 388): "fn remit_live", ("seller_fees.rs", 413): "let trigger = if confirm",
    ("seller_fees.rs", 418): "remit(&store", ("seller_fees.rs", 429): "fn exit_code_for", ("seller_fees.rs", 444): "#[cfg(test)]", ("seller_fees.rs", 500): "mod tests",
    ("seller_fees.rs", 1040): "fn a_held_spending_row_exits_refused", ("seller_fees.rs", 1160): "fn a_held_spending_row_prints_one_held_line",
    ("platform_fee.rs", 56): "fee_address", ("platform_fee.rs", 68): "PLATFORM_FEE_ADDRESS", ("home.rs", 1411): "default_allow_real_mints",
    ("home.rs", 1546): "fn default_allow_real_mints", ("home.rs", 1445): "pub platform_fee: PlatformFeeConfig", ("home.rs", 1448): "BuyerReservationFloorConfig::is_default",
    ("lib.rs", 47): "pub mod fee_remit", ("lib.rs", 55): '#[cfg(feature = "wallet")]', ("fee_remit.rs", 1869): "remittance_id: planned.remittance_id", ("fee_remit.rs", 1905): "error,",
    ("wallet_ops.rs", 1316): "OUTSIDE the job-pay budget gate", ("fee_remit.rs", 812): "No clock, no terminality inference",
    ("fee_remit.rs", 162): "two boundaries", ("fee_remit.rs", 172): "a_payment_prepared_before_expiry", ("fee_remit.rs", 196): "how a mint's clock behaves", ("lib.rs", 60): "pub mod long_poll", ("lib.rs", 63): "pub mod buyer_fund",
}
body = (R9 / "body-r9.md").read_text().split("\n")
cite = re.compile(r"(?P<file>[a-z_]+\.rs)?:(?P<a>\d{2,5})(?:[–-](?P<b>\d{2,5}))?(?![\d])")
rows, misses, seen, hist = [], [], set(), 0
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
        if (lineno, tok) in HISTORICAL:
            rows.append(f"body {lineno:4d}  {tok:<22} -> HISTORICAL (labelled so in the text; not checked against {EXEC})"); hist += 1; continue
        if f:
            cur = f
        if cur is None:
            rows.append(f"body {lineno:4d}  {tok:<22} -> NO FILE CONTEXT"); continue
        a = int(m.group("a"))
        src = SRC[cur]
        code = src[a - 1].strip() if 0 < a <= len(src) else "<OUT OF RANGE>"
        rows.append(f"body {lineno:4d}  {cur}:{a:<6} {code[:110]}")
        seen.add((cur, a))
        if m.group("b"):
            seen.add((cur, int(m.group("b"))))
for (f, n), needle in sorted(NEEDLES.items()):
    code = SRC[f][n - 1] if 0 < n <= len(SRC[f]) else ""
    ok = needle in code
    rows.append(f"NEEDLE {f}:{n} {'ok ' if ok else 'MISS'} {'(cited)' if (f, n) in seen else '(not cited in body)'} {needle!r} -> {code.strip()[:90]}")
    if not ok:
        misses.append((f, n, needle))
uncovered = sorted(s for s in seen if s not in NEEDLES)
out = R9 / f"citation-selfcheck-{EXEC}.txt"
out.write_text(f"self-check of body-r9.md citations at {EXEC} ({len(seen)} distinct file:line cited; {hist} historical occurrences left as labelled; {len(NEEDLES)} needles; {len(misses)} misses; {len(uncovered)} cited sites without a needle)\n"
               + "\n".join(rows) + "\nCITED WITHOUT NEEDLE: " + ", ".join(f"{f}:{n}" for f, n in uncovered) + "\n")
inv = [f"citations at {EXEC} — every product file:line cited by body-r9.md, with the source line"]
for f in FILES:
    ns = sorted(n for (ff, n) in seen if ff == f)
    if not ns:
        continue
    inv.append(f"{f}:")
    for n in ns:
        src = SRC[f]
        inv.append(f"{n}:{src[n - 1].strip()[:120] if 0 < n <= len(src) else '<OUT OF RANGE>'}")
(R9 / "body-citation-inventory-r9.txt").write_text("\n".join(inv) + "\n")
print(f"cited {len(seen)} distinct; historical {hist}; needles {len(NEEDLES)}; misses {len(misses)}; cited-without-needle {len(uncovered)}")
for m in misses:
    print("MISS", m)
print("UNCOVERED", uncovered)
sys.exit(1 if misses else 0)
