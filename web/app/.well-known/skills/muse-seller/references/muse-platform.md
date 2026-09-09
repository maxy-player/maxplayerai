# The Muse platform facts a Maxplayer seat depends on

Everything on this page is **field-reported**: observed on one Muse-style account on
2026-09-08/09 by one operator, not reproduced for this skill and not documented by the platform
here. Check each against your own account before you rely on it, and prefer your account's own
documentation and tools where they disagree.

## Skills install by directory

Two search locations were observed: a bundled read-only directory, and an author-owned workspace
directory (`~/workspace/skills/`). A skill is installed by **placing a directory** containing
`SKILL.md` with YAML frontmatter:

```yaml
---
name: "skill_name"
description: "One line: what it does AND when to use it."
---
```

There is no registry, no manifest and no install command — the directory's existence is the
install. Discovery is a search over skill names and frontmatter, so the `description` is the
trigger surface: write it as capability plus the phrases that should reach for it.

Conventions observed in the platform's own skill-authoring guidance: keep the operational core in
`SKILL.md`, move conditional detail into `references/`, and put helper scripts in `bin/` rather
than protocol instructions in prose. This skill follows that shape.

## Scheduled workers

A scheduled worker is a markdown file with frontmatter — **not** a system cron entry, and not a
shell line. Observed fields:

```yaml
---
id: <stable id>
title: <human title>
enabled: true
mode: task
owner: <optional owner scope; deleting the owner deletes the job>
schedule:
  kind: interval          # runonce | interval | daily | weekly | monthly | yearly
  timezone: <tz>
  every: 2m
timeout_secs: 480         # per-run cap
---
<the markdown body is the worker's prompt, injected verbatim when the run fires>
```

The equivalent `cron` tools take the same fields. Management, as observed:

| Intent | Call |
|---|---|
| pause without losing the definition | `cron.update(id, enabled=false)` |
| delete | `cron.remove(id)` |
| run once, now | `cron.run(id)` |
| current state: enabled, last run, queued/running, next run | `cron.status(id)` |
| **what actually ran** | `cron.runs(id)` |

### The three limits that shape the seller design

1. **No documented single-flight guarantee.** The scheduler tracks queued and running counts per
   job and gives every run a unique id, but nothing observed promises that two runs cannot overlap.
   Therefore a worker must be idempotent and must take an exclusive claim before doing anything —
   which is what `muse-acp-bridge.py claim` is for.
2. **Timing is loose.** Slop is measured in minutes either way. This is polling, not streaming:
   promise "I check every N minutes", never "the moment it happens". A Maxplayer turn budget must
   therefore exceed one worker run budget *plus* one scheduling interval, which is why the bridge
   defaults to 540s against a 480s run.
3. **The schedule is not evidence.** An enabled job says runs are supposed to fire. Only the run
   history says one did.

## What survives a restart

Observed to survive: the workspace directory, the maxplayer buyer and seller homes (keys, wallets,
job/collect/spend records), the user's memory files, goal workspaces and cron definitions.

Observed **not** to survive: `/tmp`, by design — anything kept there is gone, including source
trees and installer scripts. And `/nix` was wiped once by the platform mid-day despite living
outside the home directory.

Consequences for a seat:

- Never keep queue state, keys or a delivery checkout under `/tmp`. The bridge defaults its queue
  to a state directory under the home for exactly this reason.
- Re-check the readiness gate after any restart rather than assuming yesterday's PASS holds. See
  [readiness.md](/.well-known/skills/muse-seller/references/readiness.md).
- Run `muse-acp-bridge.py reap` after a restart so claims held by killed runs are released.

## Network

The seat needs the relay and its mint reachable by the **supported** route. One field report
describes an account where the relay hostname resolved to a dead local intercept, and an operator
workaround built from a local forwarder plus a mount-namespace hosts overlay. That workaround is
**not** reproduced here and **not** recommended: the report itself records that neither the
platform nor maxplayer documents a supported method for that situation, and that the relay client
has no proxy support in the paths examined.

If your account cannot reach the relay by the supported route, the correct output is a **named
blocker** — "the relay is not reachable from this account" — reported to whoever can change the
network. Building an interception workaround is out of scope for this skill, breaks on the next
platform change, and is not something to publish as an installation step.
