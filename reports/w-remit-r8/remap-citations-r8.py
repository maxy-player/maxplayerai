#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Remap every round-7 `file.rs:NNN` / `:NNN` / `:NNN–MMM` citation in the round-8 body (inherited
text written at 24ba082) onto 1a5c7c5, using the -U0 hunks of `git diff 24ba082 1a5c7c5 -- <file>`.
Adapted from reports/w-remit-r7/remap-citations.py. Rules: lines before the first hunk keep their
number; lines inside an OLD hunk range are AMBIGUOUS (reported, left unchanged, re-anchored by hand
via OVERRIDES); lines after a hunk shift by the cumulative delta. Bare `:NNN` citations take the most
recent explicit product `<name>.rs` mentioned earlier in the same body line (a CDK path such as
`melt/mod.rs` does NOT change that context); bare CDK tokens are listed in CDK_LEAVE and left alone.
Round-8 text cites 1a5c7c5 with `@` (e.g. `fee_remit.rs@2305`, `@1597`); those never match the
citation regex (it requires `:`) and are turned into `:` by the caller AFTER this remap.
usage: remap-citations-r8.py <in.md> <out.md> <report.txt>"""
import re, subprocess, sys, pathlib

OLD, NEW = "24ba082", "1a5c7c5"
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
# bare CDK 0.17.2 citations (pinned crate, not remapped)
CDK_LEAVE = {":817–831", ":831–887", ":687–697", ":907–911", ":704–712", ":706–712", ":704", ":678",
             ":403", ":574–595", ":136–148", ":817–830", ":383–399", ":709–712"}
# body lines whose bare citations have an implicit file (the two-process paragraph of Rounds 5–6)
DEFAULTS = {32: "fee_remit.rs", 38: "wallet_ops.rs"}  # 38: Round-7 B4 paragraph, bare cites follow `wallet_ops::prepare_melt_payment_blocking`
# (body line, exact token) -> replacement text, or "LEAVE" (verdict citations that describe code at
# 24ba082 / da0ee92 and say so in the text)
OVERRIDES = {
    (17, ":1580–1583"): ":1693–1696",  # remit_inner step-3 comment block, moved by the step-1c insertion
    (46, "wallet_ops.rs:1498"): "LEAVE", (46, ":1504–1507"): "LEAVE", (46, "fee_remit.rs:1624–1628"): "LEAVE", (46, ":1646–1657"): "LEAVE",
    (50, "lnurl_pay.rs:27"): "LEAVE", (50, "seller_fees.rs:20"): "LEAVE", (50, ":1086"): "LEAVE",
    (50, ":1126–1153"): "LEAVE",  # docs/SELLER-QUICKSTART.md at the current tree, written this round
}

def hunks(path):
    d = subprocess.run(["git", "diff", "-U0", OLD, NEW, "--", path], capture_output=True, text=True).stdout
    out = []
    for m in re.finditer(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@", d, flags=re.M):
        os_, oc = int(m.group(1)), int(m.group(2)) if m.group(2) is not None else 1
        ns_, nc = int(m.group(3)), int(m.group(4)) if m.group(4) is not None else 1
        out.append((os_, oc, ns_, nc))
    return out

MAPS = {k: hunks(v) for k, v in FILES.items()}

def remap(short, line):
    hs = MAPS.get(short)
    if hs is None:
        return None, "UNKNOWN-FILE"
    delta = 0
    for os_, oc, ns_, nc in hs:
        if oc == 0:
            if line > os_:
                delta += nc
            continue
        if line < os_:
            break
        if os_ <= line < os_ + oc:
            return None, "AMBIGUOUS"
        delta += nc - oc
    return line + delta, "ok"

body = pathlib.Path(sys.argv[1]).read_text()
table = []
out_lines = []
cite = re.compile(r"(?P<file>[a-z_]+\.rs)?(?P<colon>:)(?P<a>\d{2,5})(?P<range>[–-](?P<b>\d{2,5}))?(?![\d])")

for lineno, text in enumerate(body.split("\n"), 1):
    current_file = DEFAULTS.get(lineno)
    pos = 0
    pieces = []
    for m in cite.finditer(text):
        tok = m.group(0)
        f = m.group("file")
        before = text[max(0, m.start() - 12):m.start()]
        if re.search(r"\d$", before) and not f:  # 2026-09-08T05:19 -> ':19'
            continue
        ov = OVERRIDES.get((lineno, tok))
        if ov == "LEAVE":
            table.append((lineno, f or current_file or "?", tok, "LEFT (override)"))
            continue
        if ov:
            pieces.append(text[pos:m.start()]); pieces.append(ov); pos = m.end()
            table.append((lineno, f or current_file or "?", tok, ov + " (override)"))
            continue
        if f and f not in FILES:
            table.append((lineno, f, tok, "CDK/other file, left"))
            continue
        if not f and tok in CDK_LEAVE:
            table.append((lineno, "cdk", tok, "CDK bare, left"))
            continue
        if f:
            current_file = f
        if current_file is None:
            table.append((lineno, "?", tok, "UNRESOLVED"))
            continue
        short = current_file
        na, sa = remap(short, int(m.group("a")))
        nb, sb = (None, "ok")
        if m.group("b"):
            nb, sb = remap(short, int(m.group("b")))
        status = sa if sa != "ok" else sb
        if status == "ok":
            new = (f or "") + ":" + str(na) + (("–" + str(nb)) if m.group("b") else "")
            pieces.append(text[pos:m.start()]); pieces.append(new); pos = m.end()
            table.append((lineno, short, tok, new))
        else:
            table.append((lineno, short, tok, status))
    pieces.append(text[pos:])
    out_lines.append("".join(pieces))

pathlib.Path(sys.argv[2]).write_text("\n".join(out_lines))
with open(sys.argv[3], "w") as t:
    t.write(f"citation remap {OLD} -> {NEW}; hunks per file: " + ", ".join(f"{k}={len(v)}" for k, v in MAPS.items()) + "\n")
    for row in table:
        t.write("body-line %5d  %-15s  %-22s -> %s\n" % row)
    n_amb = sum(1 for r in table if r[3] == "AMBIGUOUS")
    n_unr = sum(1 for r in table if r[3] in ("UNRESOLVED", "UNKNOWN-FILE"))
    n_left = sum(1 for r in table if "left" in r[3].lower())
    n_ok = len(table) - n_amb - n_unr - n_left
    t.write(f"total {len(table)}  remapped {n_ok}  left-as-is {n_left}  ambiguous {n_amb}  unresolved {n_unr}\n")
print(open(sys.argv[3]).read().splitlines()[-1])
