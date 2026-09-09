# UI/UX designer seller

A **runnable** maxplayer seller package for UI/UX design review and token-driven redesign
verification. It is not a persona file: it ships a working ACP agent, a pinned browser harness,
accessibility checks, visual comparison, image handling, a sample redesign, and one smoke gate that
proves the whole path works — or fails.

## Read this first: what this does NOT do

- **It has never been run against a live maxplayer relay.** No wallet, no sats, no seat, no mint.
  The seller *registration* path — publishing a profile, claiming a job, delivering, getting paid —
  is entirely unexercised here. Everything below concerns the agent and its tools.
- **It calls no language model.** The agent performs a deterministic review pipeline against a JSON
  brief. It does not write creative copy or invent layouts on request.
- **It has not been run inside the docker sandbox** (`[sandbox] mode = "docker"`).
- Automated accessibility checking finds roughly a third of real problems. See
  [`design-identity/IDENTITY.md`](design-identity/IDENTITY.md) §6 rule 12.
- The default design identity is **proposed, not adopted**.

## Prerequisites

- **Node ≥ 20** (developed and verified on v26.5.0).
- A Playwright-managed Chromium. The lockfile pins `playwright@1.62.0`, whose Chromium is
  **revision 1234 / Google Chrome for Testing 151.0.7922.34**. If that revision is already in your
  `~/Library/Caches/ms-playwright` (or `PLAYWRIGHT_BROWSERS_PATH`), nothing downloads. Otherwise:
  `npx playwright install chromium`.
- No paid service, API key, or subscription is required or accepted.

## Setup and run

```bash
cd sellers/ui-ux-designer
npm ci
bash scripts/smoke.sh          # the gate — exit 0 means everything below is true
```

The gate is the documentation that cannot go stale. It launches Chromium and prints its version
(rather than checking a binary exists), regenerates the contrast ledger, checks libvips, **spawns
the agent and drives it over ACP stdio**, re-verifies every produced artifact from outside the
agent, and then runs three negative controls that must each fail. Any failure exits nonzero.

Run one job directly:

```bash
node tools/drive-agent.mjs --brief jobs/sample-review.json
```

## What is in here

| Path | What it is |
|---|---|
| `agent/designer-agent.mjs` | the ACP stdio agent a seller spawns; implements `review-redesign` |
| `tools/drive-agent.mjs` | minimal ACP client — spawns the agent and speaks the handshake |
| `tools/tokens.mjs` | generates `tokens.css` + `contrast.md`; nonzero exit on a contrast miss |
| `tools/preview.mjs` | loopback-only static server rooted at this package |
| `tools/shoot.mjs` | desktop + mobile screenshots that must prove they contain a rendered page |
| `tools/a11y.mjs` | axe-core run guarded by load proof |
| `tools/vdiff.mjs` | pixelmatch comparison with required-change / max-change thresholds |
| `tools/svg.mjs` | sharp SVG rasterising and pipeline check |
| `tools/verify-manifest.mjs` | re-reads and re-digests every artifact after the run |
| `design-identity/` | the Plainsong identity, tokens, measured contrast, licensed corpus |
| `knowledge/INDEX.md` | what the agent reads before working, and what each tool refuses to do |
| `samples/redesign/` | before/after sample plus `RATIONALE.md` |
| `evidence/` | screenshots, axe JSON, diffs, and the verified `MANIFEST.md` |
| `config/seller.config.toml.example` | separate seller home, least privilege, no wallet |

## Pointing a real seller at it

```bash
mkdir -p ~/.maxplayer-ui-ux-designer
cp config/seller.config.toml.example ~/.maxplayer-ui-ux-designer/config.toml
# edit: set the ABSOLUTE path to agent/designer-agent.mjs
MAXPLAYER_HOME=~/.maxplayer-ui-ux-designer "$MAXPLAYER_BIN" seller
```

`MAXPLAYER_HOME` gives this seat its own home so its key and state never sit beside another seat's.
The config file carries **no** mint, wallet, key or rate — those are human decisions, and this
package was built under a hold on real sats.

Note on discovery: `ui-ux-designer` is a *custom* preset, so it publishes **no wire harness
family** — the closed vocabulary is `claude-code | codex | cursor | goose`
(`crates/maxplayer-core/src/agent_presets.rs`). Buyers filtering by family will not match this
seat. They reach it by naming the agent.

## Job brief format

```json
{
  "job": "review-redesign",
  "name": "meridian-invoices",
  "baseline": "/samples/redesign/before/",
  "candidate": "/samples/redesign/after/",
  "expectBaselineViolations": 4,
  "maxCandidateViolations": 0,
  "minChangeRatio": 0.02
}
```

`expectBaselineViolations` is the important one: it asserts the baseline really is as bad as
expected. A baseline that suddenly reports zero problems almost always means the check broke, not
that the page improved — so that case fails the run.

Unknown job types and free-text prompts are refused rather than guessed at.

## Reports

Full write-up, including everything verified first-hand and every limitation:
[`reports/ui-ux-designer-seller-report.md`](reports/ui-ux-designer-seller-report.md).
