#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Step 26b: substitute the gate 1–4 grep outputs re-measured at 1a5c7c5 (greps-1a5c7c5.txt) into
the remapped round-8 body, fill the census placeholder, drop the stray blank line inside the gate-4
fence, and turn the round-8 `@` citations into `:` (AFTER the remap, so they were never shifted).
Input body-r8-remapped.md → output body-r8-final-draft.md. Every anchor must match exactly once."""
import pathlib, re, sys
R8 = pathlib.Path("/Users/forge/forge/v2/wt/w-seller-fee-stage2a-remit/reports/w-remit-r8")
t = (R8 / "body-r8-remapped.md").read_text()

def once(old, new):
    global t
    n = t.count(old)
    if n != 1:
        sys.exit(f"anchor found {n} times, expected 1: {old[:100]!r}")
    t = t.replace(old, new)

def replace_span(start, end, new):
    """replace from the unique `start` anchor to the end of the unique `end` anchor (inclusive)."""
    global t
    a = t.find(start); b = t.find(end, a)
    if a < 0 or t.count(start) != 1 or b < 0:
        sys.exit(f"span anchors: start×{t.count(start)} end-found={b >= 0}: {start[:80]!r}")
    t = t[:a] + new + t[b + len(end):]

# ---- Gate 1 -----------------------------------------------------------------------------------------
replace_span('$ grep -n "mint_allowed(" crates/maxplayer-core/src/wallet_ops.rs\n',
             'melt_status_for_invoice_async (fn :1613, read-only)\n',
             '$ grep -n "mint_allowed(" crates/maxplayer-core/src/wallet_ops.rs\n'
             '984:   send_async (fn :972)          1044:  receive_async (fn :1025)       1119:  melt_within_async (fn :1105, operator composition, out of scope)\n'
             '1164:  pay_melt_quote_async (fn :1153, retained; no live caller)\n'
             '1390:  prepare_melt_payment_blocking (fn :1377) — THE payment path, before the wallet is opened\n'
             '1633:  melt_quote_async (fn :1623, read-only)   1688: melt_status_for_quote_async (fn :1678, read-only)   1727: melt_status_for_invoice_async (fn :1720, read-only)\n')
once('108:            "--confirm" if remit => confirm = true,\n411:    let trigger = if confirm {',
     '110:            "--confirm" if remit => confirm = true,\n413:    let trigger = if confirm {')
once('475:    pub fn pays(self) -> bool {\n476-        !matches!(self, Self::DryRun)',
     '511:    pub fn pays(self) -> bool {\n512-        !matches!(self, Self::DryRun)')

# ---- Gate 2a ----------------------------------------------------------------------------------------
once("Non-test boundaries: `run.rs` first `#[cfg(test)]` at 617 (run-loop `tests` module at 7937); `fee_remit.rs` `test_support` module at 2111, `tests` at 3010 (the file's first `#[cfg(test)]` attribute is now `:233`, the `PreparedToken::Fake` variant inside non-test code, so classification uses the module starts); `seller_fees.rs` 442.",
     "Non-test boundaries at `1a5c7c5`: `run.rs` first `#[cfg(test)]` at 617 (run-loop `tests` module at 7937); `fee_remit.rs` `test_support` module at 2352, `tests` at 3364 (the file's first `#[cfg(test)]` attribute is `:269`, the `PreparedToken::Fake` variant inside non-test code, so classification uses the module starts); `seller_fees.rs` 444 (`tests` at 500).")
# the two file-prefixed caller lines were already shifted by the hunk remap; assert the measured values
for must in ("crates/maxplayer/src/seller_fees.rs:418:    let outcome = remit(&store, &mut effects, trigger, now_unix, out)?;",
             "crates/maxplayer-core/src/fee_remit.rs:1965:        Ok(mut effects) => remit_best_effort(store, &mut effects, trigger, now_unix),  # remit_live_best_effort (:1958) → remit_best_effort (:1935) → remit"):
    if t.count(must) != 1:
        sys.exit(f"remapped gate-2a line not found once: {must[:90]!r}")

# ---- Gate 3 -----------------------------------------------------------------------------------------
replace_span("$ git grep -n -F 'maxplayer@agi.cash' 24ba082 -- 'crates/*.rs' | wc -l\n",
             "— 35 at 19f30d3 + 2 fee-bearing test fixtures\n",
             "$ git grep -n -F 'maxplayer@agi.cash' 1a5c7c5 -- 'crates/*.rs' | wc -l\n"
             "      38\n"
             'NON-TEST crates/maxplayer-core/src/platform_fee.rs:68:pub const PLATFORM_FEE_ADDRESS: &str = "maxplayer@agi.cash";\n'
             "test     37 lines, all past their file's test boundary: fee_remit.rs ×14 (≥2898; test_support 2352), lnurl_pay.rs ×5 (≥705; 655), platform_fee.rs ×1 (:220; 126), run.rs ×1 (:14871; 617), store.rs ×6 (≥3888; 2839), seller_fees.rs ×10 (≥688; 444) — 36 at 24ba082 + 1 round-8 test fixture (regression (i), `fee_remit.rs:4632`)\n")

# ---- Gate 4 -----------------------------------------------------------------------------------------
census = [l for l in (R8 / "greps-1a5c7c5.txt").read_text().splitlines() if l.startswith("send_async ")][-1].rstrip(" ·").replace(" · ", ", ")
once("<CENSUS-1a5c7c5>\n\n$ grep -n 'effects.prepare_melt(", census + "\n$ grep -n 'effects.prepare_melt(")
once("crates/maxplayer-core/src/fee_remit.rs   # non-test (< 3010)\n1480:    let prepared = match effects.prepare_melt(&quote.quote_id, &ceiling) {\n1540:            if let Err(error) = effects.cancel_melt(prepared) {          # fence refused: cancel first\n1590:        let cancel_note = match effects.cancel_melt(prepared) {          # pay-time margin refused: cancel first\n1606:    match effects.confirm_melt(prepared) {                              # the one spend\n$ git diff --stat a6217328..24ba082 -- crates/maxplayer-core/src/wallet_ops.rs\n 1 file changed, 1178 insertions(+), 3 deletions(-)",
     "crates/maxplayer-core/src/fee_remit.rs   # non-test (< 3364)\n1576:    let prepared = match effects.prepare_melt(&quote.quote_id, &ceiling) {\n1600:            if let Err(error) = effects.cancel_melt(prepared) {          # confirmability refused (round 8, step 1c): cancel first, before the fence\n1653:            if let Err(error) = effects.cancel_melt(prepared) {          # fence refused: cancel first\n1704:        let cancel_note = match effects.cancel_melt(prepared) {          # pay-time margin refused: cancel first\n1720:    match effects.confirm_melt(prepared) {                              # the one spend\n$ git diff --stat a6217328..1a5c7c5 -- crates/maxplayer-core/src/wallet_ops.rs\n 1 file changed, 1291 insertions(+), 3 deletions(-)")

# ---- `@` → `:` for the round-8 citations ------------------------------------------------------------
n_at = len(re.findall(r"(?<=\.rs)@\d+|(?<=`)@\d+", t))
t = re.sub(r"(?<=\.rs)@(\d+)", r":\1", t)
t = re.sub(r"(?<=`)@(\d+)", r":\1", t)
left = re.findall(r"@\d{2,5}", t)
if left:
    sys.exit(f"unconverted @ tokens: {left}")

out = R8 / "body-r8-final-draft.md"
out.write_text(t)
n = len(t.encode())
print(f"final draft: {n} bytes, {t.count(chr(10)) + 1} lines, {n_at} '@' citations converted, 24ba082 mentions: {t.count('24ba082')}")
if n > 65000:
    sys.exit(f"TOO BIG: {n}")
