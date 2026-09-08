#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Round 9 pin build (ruling 2026-09-08T18:32Z item 2): make body-r9.md pinnable under GitHub's
65,536-character PR-body cap (target <= 62,000 characters).

Input : reports/w-remit-r9/body-r9.md            (the 0-miss self-checked body at 065fc5a / 37b3615)
Output: reports/w-remit-r9/body-r9-pin.md        (the body that is pinned)
        reports/w-remit-r9/rounds-5-8-sections.md (the moved-out round 5-8 narrative, verbatim)

What moves out (verbatim, nothing rewritten): the Rounds 5-6, Round 7 and Round 8 sections (body lines
30-55 at 37b3615, keeping the two ledger lines inline), the 44-record `--exact` listing (the code block
under gate 2a..r9, lines 109-154) and the per-file evidence hash block (lines 254-279). Each moved
block is replaced by a short pointer with a permalink at the commit that carries the moved files
(MOVED_SHA, argv[1]); every other line is copied unchanged, except the CI block's two "in_progress /
pending" lines (rewritten from ci-reads-pin.txt) and the heads paragraph's `a269952` diffstat lines.

Usage: python3 build-body-pin-r9.py <moved-files-commit-sha|@@A@@>
"""
import re
import subprocess
import sys

MOVED = sys.argv[1] if len(sys.argv) > 1 else "@@A@@"
BASE = "https://github.com/maxy-player/maxplayerai/blob/%s/reports/w-remit-r9/" % MOVED
SRC = "reports/w-remit-r9/body-r9.md"
OUT = "reports/w-remit-r9/body-r9-pin.md"
MOVED_FILE = "reports/w-remit-r9/rounds-5-8-sections.md"

lines = open(SRC, encoding="utf-8").read().split("\n")
L = lambda n: lines[n - 1]  # 1-based


def assert_starts(n, prefix):
    if not L(n).startswith(prefix):
        sys.exit("line %d does not start with %r: %r" % (n, prefix, L(n)[:80]))


# Anchors at 37b3615 (the body has not changed since 065fc5a).
assert_starts(13, "## §5 statement")
assert_starts(30, "## Rounds 5 and 6")
assert_starts(38, "## Round 7")
assert_starts(44, "## Round 8")
assert_starts(54, "**Prior-finding ledger (round 8's rows):**")
assert_starts(56, "## Round 9")
assert_starts(70, "## Gates")
assert_starts(108, "**Gates 2a (New-only), 2b–2g")
assert_starts(109, "```")
assert_starts(154, "```")
assert_starts(156, "**Gate 3")
assert_starts(181, "## Full suites")
assert_starts(193, "## Money-path binary ×3")
assert_starts(204, "## CI")
assert_starts(226, "fa4228e · 6172b09 · 9b0bc01")
assert_starts(227, "final head (this body's pin)")
assert_starts(231, "## fmt / clippy")
assert_starts(236, "## Per-file diffstat")
assert_starts(253, "## Evidence files")
assert_starts(254, "```")
assert_starts(279, "```")
assert_starts(281, "## Disclosures")
assert_starts(291, "## Out of scope")

link = lambda name: "[`%s`](%s%s)" % (name, BASE, name)

# ---- moved-out narrative (verbatim) --------------------------------------------------------------
moved_header = [
    "# Rounds 5–8 of the #979 body — moved out of the pinned body (round 9, ruling 2026-09-08T18:32Z item 2)",
    "",
    "Verbatim body lines 30–55 of `reports/w-remit-r9/body-r9.md` at `37b3615` (the 0-miss self-checked body; "
    "`citation-selfcheck-240240c.txt`), moved out so the pinned body fits GitHub's 65,536-character cap. Nothing "
    "here is rewritten; the two prior-finding ledger lines stay inline in the pinned body as well. Line numbers "
    "inside are at the heads each section names (`24ba082` for rounds 5–6, `240240c` where re-anchored in round 9).",
    "",
]
moved_body = lines[29:55]  # lines 30..55
open(MOVED_FILE, "w", encoding="utf-8").write("\n".join(moved_header + moved_body) + "\n")

rounds_stub = [
    "## Rounds 5–8 — B3 / D / G / H (addenda 6–7), B4 / H1–H4 / I (addendum 8), F1–F4 (addendum 9) — moved out of this body",
    "",
    "The four sections (verdicts at `6fc77e1`, `6fd13df`, `19f30d3`, `da0ee92`) stand verbatim, including their round-9 "
    "corrections (F3's withdrawal of \"local only\" — N4 item 2 — and F4's re-anchoring at `240240c`), in "
    + link("rounds-5-8-sections.md")
    + " (body lines 30–55 at `37b3615`); the round-8 body as pinned is `reports/w-remit-r8/body-pin-4714623.txt`. "
    "They were moved, not cut, because GitHub caps a PR body at 65,536 characters and the round-9 body measured 90,260 "
    "(ruling 2026-09-08T18:32Z item 2). Everything those rounds established that still governs the money path is restated "
    "at `240240c` in the §5 statement above (fence, quote binding, hold, CDK fee model, three cancel sites, prepare's true "
    "bound) and in the Round 9 section below.",
    "",
    L(54),
    "",
]

# ---- exact listing stub ---------------------------------------------------------------------------
exact_stub = [
    "```",
    "44 records, one `cargo test … -- --exact <name>` each, logs `logs/exact-NN-<gate>-240240c.log` (line 1 = `git rev-parse HEAD`),",
    "commands in exact-records.txt, results per record in exact-summary-240240c.txt (moved out of this body):",
    "  43 passed (1 passed; 0 failed each) · record 33 RETIRED (0 passed, 0 failed: `a_melt_ceiling_admits_total…` was removed",
    "  with `admits_total` in round 9 — commit A — and is replaced by records 42 and 43)",
    "  round 9's records: 37 fee-metadata drift refused before the fence · 39 gross 19 / reserve 2 pays 13 (§1.3) · 40 shrunk",
    "  reserve re-planned, pays 15 (§1.4a) · 41 store `replan_remittance` · 42 `confirm_bound` search · 43 `admits_confirmable` ·",
    "  44 N3 paid-label; records 01–36, 38 are rounds 2–8's, unchanged in name and result.",
    "```",
]

# ---- evidence stub --------------------------------------------------------------------------------
evidence_stub = [
    "```",
    "69 rows (name · bytes · sha256) in evidence-table.txt, linked below: exec2-{core-wallet-nff,cli-default,cli-acp-wallet,fmt,clippy}.log,",
    "exec2-{fmt-overlap,clippy-map,progress}.txt, money-path-ci-{1,2,3}-240240c.log, 44 exact-NN-*-240240c.log, exact-records.txt,",
    "lint-mapping.md, greps-240240c.txt, plan.md, run-gates-exec2.sh (the per-file rows this body carried at 37b3615 are there verbatim)",
    "```",
    "",
    "**Moved out of this body (ruling item 2), each committed on the branch, permalinks at `%s`:** " % MOVED
    + link("rounds-5-8-sections.md") + " (rounds 5–8 narrative, verbatim) · "
    + link("exact-records.txt") + " + " + link("exact-summary-240240c.txt") + " (the 44 `--exact` records and results) · "
    + link("evidence-table.txt") + " (69 rows, name · bytes · sha256) · "
    + link("citation-selfcheck-240240c.txt") + " + " + link("citation-remap-240240c.txt") + " + "
    + link("body-citation-inventory-r9.txt") + " (190 sites / 223 needles / 0 misses, `selfcheck-r9.py`) · "
    + link("claims.md") + " (7 / 7) · " + link("lint-mapping.md") + " (fmt/clippy per added line) · "
    + link("greps-240240c.txt") + " (gates 1–4 raw) · " + link("body-r9.md") + " (the unsplit 90,260-character body) · "
    + link("commit-history-a6217328-240240c.md") + " (every commit, round by round) · " + link("build-body-pin-r9.py") + " (this split, mechanical).",
]

# ---- claims summary + known defects (new sections, ruling items 2 and 3) ---------------------------
claims_section = [
    "## claims.md — the seven claim groups the verdict at `4714623` failed, re-verified by hand at `240240c`: 7 / 7 PASS",
    "",
    link("claims.md") + " (committed `37b3615`) quotes, per group, the body sentence now (body line), the source line now "
    "(`grep -n` at `240240c`, code quoted) and a PASS/FAIL with the reason: (1) `from_prepared` zeroes `swap_fee` "
    "**unconditionally** (`saga/mod.rs:592`, no branch) — PASS; (2) order reserve-only precheck `fee_remit.rs:1555` → prepare "
    "`:1764` → post-swap arithmetic `:1785` → fence `:1820` — PASS; (3) **three** `effects.cancel_melt(` sites `:1788` / `:1841` / "
    "`:1892`, all inside `remit_inner` (`:1033`–`:2006`), no fourth — PASS; (4) prepare may GET keysets through the SDK's "
    "metadata cache (`keysets.rs:44–51` → `mint_metadata_cache.rs:366`), posts no proof- or fee-bearing request; \"cache-only\" "
    "0 hits — PASS; (5) record 28 prepared input fee **4** ≠ actual 3 at both body sites (`:4877`, `:4908`, `:4949`) — PASS; "
    "(6) one `writeln!` (`:1536–1539`, single-line format string), row stays Planned (`:3595`; called `:5359`, `:5452`, `:5648`) — "
    "PASS, with the occurrence-count note carried to Known defects below; (7) no blanket \"all claims correct\" sentence remains "
    "(0 grep hits) — PASS as the citation-level claim it now is. The file is the semantic sample the verdict said a zero-offset "
    "self-check cannot replace; the self-check itself (190 sites / 223 needles / 0 misses at `240240c`) is `citation-selfcheck-240240c.txt`.",
    "",
]

known_defects = [
    "## Known defects — named, not fixed (ruling 2026-09-08T18:32Z item 3; a `.rs` change would move the executable head and re-run every gate — the advisor rules on them)",
    "",
    "- **`fee_remit.rs:5280` — stale test comment.** The comment above "
    "`a_fee_bearing_schedule_whose_prepared_figures_fit_but_post_swap_arithmetic_does_not_is_refused_before_the_fence` (`:5284`) "
    "still says \"the row released (Planned → Failed, receipts back)\" — round 8's release; the test asserts round 9's contract: "
    "`assert_refused_before_fence_row_stays_planned` (`:5359`, helper `:3595`) — the row stays Planned, unbound, receipts pinned — and "
    "only the attempt journal is `Failed` (`:5362`).",
    "- **`:5381` — the same stale sentence** above `a_fee_bearing_total_that_exceeds_the_gross_by_the_fee_is_refused_before_any_swap_or_melt` "
    "(`:5384`): \"released (Planned → Failed, receipts back)\"; the test asserts stays-Planned (`:5452`) and attempt `Failed` (`:5455`).",
    "- **`:5617` — stale test comment** inside `a_reserve_that_grows_between_estimate_and_payment_is_refused_before_spending` (`:5593`): "
    "\"the payment quote was raised and checked BEFORE the fence — the row was never admitted, so it was released as a planned row of our "
    "own\" (addendum 5 §1 rule 1 wording); the test asserts the row is **not** released — `assert_refused_before_fence_row_stays_planned` (`:5648`).",
    "- **The \"exactly one refusal line\" assertion counts phrase occurrences, not lines:** `out.matches(\"REFUSED before spending\").count()` "
    "== 1 at `:3614`, `:5337` and `:5430` (the verdict at `4714623` criticised this form). It is sound at `240240c` only because the format "
    "string at `:1536–1539` is one line with no embedded newline and the `{reason}` text carries none; an `out.lines()`-based assertion is "
    "owed, not written.",
    "",
]

# ---- CI lines refresh (from ci-reads-pin.txt, read 2026-09-08 ~18:50Z) ------------------------------
ci_line_226 = ("fa4228e · 6172b09 · 9b0bc01 · a269952 · 228516a · 065fc5a · 37b3615   runs 34260069114 · 34260445923 · 34260997966 · "
               "34261585806 · 34261910706 · 34262869007 · 34263289512   all completed success   17:56–18:29Z   (reports only; body drafts, "
               "self-check, claims.md — read at 2026-09-08T18:50Z, `gh run list --branch feat/seller-fee-remit --limit 14`, `ci-reads-pin.txt`)")
ci_line_227 = ("%s (moved-out files + this split) · final head (this body) · pin commit   pending at build time — read after the pin and "
               "recorded in body-pin-<final>.txt, committed after the final head as in rounds 7–8" % MOVED)

# ---- heads paragraph diffstat refresh ----------------------------------------------------------------
def git(*a):
    return subprocess.check_output(["git"] + list(a), encoding="utf-8").strip()

head_now = git("rev-parse", "--short", "HEAD")
stat_tail = git("diff", "--stat", "240240c..HEAD").split("\n")[-1].strip()
excl = git("diff", "--stat", "240240c..HEAD", "--", ".", ":(exclude)reports")

# ---- line 1 commit history + line 66 round-9 commit list -> commit-history file ----------------------
HIST_FILE = "reports/w-remit-r9/commit-history-a6217328-240240c.md"
l1 = L(1)
h_start = l1.index("**: `3cda4ee` (brief)") + 2
h_end = l1.index(". This body is written against `240240c`")
assert_starts(66, "**Round 9 commits, in order**")
history = l1[h_start + 2:h_end]
open(HIST_FILE, "w", encoding="utf-8").write("\n".join([
    "# #979 commit history `a6217328..240240c`, round by round — moved out of the pinned body (round 9, ruling 2026-09-08T18:32Z item 2)",
    "",
    "Verbatim from body line 1 of `reports/w-remit-r9/body-r9.md` at `37b3615` (the per-round commit list) and body line 66 "
    "(the round-9 commits with what each did). Nothing rewritten. Executable head `240240c3f26bd35c3a8c5a3d70229f9730a0a221`; "
    "every commit after it is `reports/` only; the final head is the pinned body's commit, the pin commit records the read-back.",
    "",
    "## Every commit, round by round (body line 1)",
    "",
    history,
    "",
    "## Round 9 commits, in order (body line 66)",
    "",
    L(66),
    "",
]))
line1 = (l1[:h_start] + " — the commit list, round by round with one line per commit, is "
         + link("commit-history-a6217328-240240c.md")
         + " (moved out of this body for the character cap; it ends at `240240c`, the executable head, and the reports-only commits after it up to this body's pin — the final head)"
         + l1[h_end:])
line66 = ("**Round 9 commits, in order** (`git log --format=%s`; every one authored and committed as `w-seller-fee-stage2a-remit <worker@forge.local>`): "
          "`b0c2c03` (plan) → `c9cce6c` (A) → `eefdb70` (B) → `b7e3f34` (C) → `573ab50` `c06b490` `1f03898` `bcbc8b9` (D) → `fa2b8af` → `e6f7b7a` → `380f097` → "
          "`9ae7c9c` `71b4971` (E) → `2521a44` (F) → `240240c` (lint fix-up — **the executable head**), then reports only; what each did, one line per commit, is in "
          + link("commit-history-a6217328-240240c.md") + " (the CI section below reads every one of these heads).")

out = []
# 1..12 preamble, with the a269952 diffstat lines refreshed to the build head
for n in range(1, 13):
    s = L(n)
    if n == 1:
        s = line1
    if n == 5:
        s = s.replace("at `a269952`:", "at `%s` (the build head of this pinned body; its own sha cannot appear inside it):" % head_now)
    if n == 7:
        s = "$ git diff --stat 240240c..%s -- . ':(exclude)reports'      → %s" % (
            head_now, "(empty: no .rs, docs or Cargo change after the executable head)" if excl == "" else "NON-EMPTY: " + excl)
    if n == 8:
        s = "$ git diff --stat 240240c..%s | tail -1" % head_now
    if n == 9:
        s = " %s   (reports/w-remit-r9/ only)" % stat_tail
    out.append(s)
out += lines[12:29]            # 13..29 §5 statement
out += rounds_stub             # replaces 30..55
out += lines[55:65] + [line66] + lines[66:69]   # 56..69 Round 9, line 66 shortened
out += lines[69:108]           # 70..108 Gates heading, gates 1, 2a, the exact paragraph (108)
out += exact_stub              # replaces 109..154
out += lines[154:192]          # 155..192 gates 3, 4, Full suites
out += claims_section          # ruling order: claims summary
out += lines[230:235]          # 231..235 fmt / clippy (lint summary)
out += known_defects           # ruling order: known defects
out += lines[192:203]          # 193..203 Money-path ×3
out += lines[203:225]          # 204..225 CI
out += [ci_line_226, ci_line_227]
out += lines[227:230]          # 228..230
out += lines[235:253]          # 236..253 diffstat + Evidence heading
out += evidence_stub           # replaces 254..279
out += lines[279:]             # 280.. Disclosures, Out of scope

text = "\n".join(out)
if not text.endswith("\n"):
    text += "\n"
open(OUT, "w", encoding="utf-8").write(text)
n_chars = len(text)
print("wrote %s: %d characters, %d bytes; moved-out %s: %d characters" % (
    OUT, n_chars, len(text.encode("utf-8")), MOVED_FILE, len(open(MOVED_FILE, encoding="utf-8").read())))
print("cap 65536: %s; target 62000: %s" % ("OK" if n_chars <= 65536 else "OVER", "OK" if n_chars <= 62000 else "OVER"))
