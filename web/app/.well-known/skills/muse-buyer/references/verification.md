# What is verified in maxplayer-muse-buyer, and what is not

Read this before you rely on a step. Three tiers, and the tier is stated for every claim in the
skill that could cost money if it were wrong.

## Source-checked (this repository)

Checked by reading the code at the commit this page ships from. These do not depend on anyone's
field report.

| Claim | Where |
|---|---|
| The MCP surface is exactly `post_job`, `get_job`, `collect`, `award_claim`; every other tool moved to the CLI and returns an error naming its replacement | `crates/maxplayer/src/mcp.rs` |
| `post_job` requires `task`, `output`, `amount_sats`, and sets `additionalProperties: false` | same |
| `max_sats` defaults to `amount_sats`; the daemon never auto-awards a claim it cannot pay | same |
| `harness`, `harness_family`, `model`, `capabilities` are hard award filters; `model` requires `harness` | same |
| `collect` order is accept → verify tip-match → pay → materialize; it is idempotent and refuses without paying on mismatch or bad co-signature | same |
| `award_claim` is write-once per job, so a retry re-sends the same signed event | same |
| `get_job`'s `timeout_secs` above the cap is refused, not shortened | same |
| Request vocabulary (`claude`) differs from resolved attribution (`claude-agent-acp`) | same |
| `maxplayer buyer` refuses `--home` (both spellings) and names `MAXPLAYER_HOME` | `crates/maxplayer/src/cli.rs`, four tests |
| `MAXPLAYER_HOME` must be set on the MCP server process; the MCP command has no `--home` | repository `AGENTS.md` |

**Version:** those sources are at **0.5.5**. The field reports below ran **0.5.7**. Nothing here
has been checked across that gap, so treat a disagreement as a signal to re-read the source on
the version you actually installed:

```bash
maxplayer --version
```

## Field-reported (one operator's box, 2026-09-08/09)

Plausible and internally consistent, but observed once, on one account, by one operator. Not
reproduced for this skill. Anything in this tier is labelled *field-reported* where it appears.

- `npm install -g maxplayer` works on a Muse-style container.
- A Muse account installs a skill by placing a directory; `muse.skill_search` matches on name and
  frontmatter; there is no registry or install command.
- `~/workspace`, `~/.maxplayer` and `~/.maxplayer-seller` survive a restart; `/tmp` does not.
- A shipped config defaults to a live mint with `allow_real_mints = true` and
  `per_job_budget_sats = 30000`, with no total cap; `spent.jsonl` is the audit ledger.
- Buyer state files: `jobs/<job_id>.json`, `collects/<job_id>.json`, `spent.jsonl`, `buyer.sqlite`.
- A delivery refused by the seller's transport allowlist surfaced `reason_code=delivery_failed`
  to the buyer with no payment.

One caution about the logs this skill was built from: the paying buyer in the recorded 100-sat
trade was **not** the same buyer as the Muse seat that posted the earlier jobs. Do not read that
sequence as one account's clean end-to-end run.

## Not verified at all

- **No clean-account Muse acceptance run exists.** Nobody has taken an empty Muse account through
  this page start to finish. That is a **release gate that has not been passed**, not a step that
  passed quietly.
- No spend, no wallet operation and no relay contact was performed while writing this skill. The
  offline tests that ship with the seller skill exercise the bundled helper only.
- The reconstructed `post_job` examples in the source field guide **omitted the required `output`
  argument**, so no recorded successful call can be replayed verbatim. The argument names here come
  from the schema, not from a captured payload.

## What would close the gap

One operator, one fresh Muse account, no prior maxplayer state, working through the skill in order
and recording: the resolved `maxplayer --version`; whether `maxplayer doctor` passes without any
network workaround; the exact `post_job` arguments sent and the job id returned; the `collect`
result fields; and `wallet balance` before and after. Until that exists, this skill is
**unverified for public use**, and saying so is part of using it.
