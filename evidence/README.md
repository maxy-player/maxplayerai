# Evidence bundles

Each subdirectory is one complete execution of `crates/maxplayer-tool-kit/docker/demo.sh`,
named by its UTC start time. A bundle holds per-step verdicts (`results.txt`), both MCP
transcripts, holder and vendor logs, vendor counter snapshots, and a `manifest.json`.

Every bundle here is `mechanism_only`. The vendor and the CLI were both written for this
contract and therefore cannot falsify it: these runs establish that the holder mechanism
behaves as specified, not that any third-party tool has been accepted.

## Why there are two bundles from 2026-09-09

`20260909T220053Z` and `20260909T220105Z` are two runs started twelve seconds apart. That
overlap was not intentional. The turn that produced them was one of several killed mid-flight
by provider errors, and two overlapping executions resulted; both completed, and `git add -A`
committed both.

They are kept rather than pruned, because deleting an inconvenient artifact is a worse habit
than explaining it, and because the accident is mildly informative: the two runs are
independent, use different ephemeral ports and container names, and agree exactly.

| | `20260909T220053Z` | `20260909T220105Z` |
| --- | --- | --- |
| checks passed | 27 | 27 |
| checks failed | 0 | 0 |
| verdict | PASS | PASS |
| platform | linux/arm64, docker 29.5.2 | linux/arm64, docker 29.5.2 |

The check *names* are identical between the two; the only textual differences are the run id
and the output path. Either bundle can be read as the record of the run; citing one is not a
claim that the other disagrees.

One caveat worth stating rather than leaving implicit: because the runs overlapped in time on
a shared Docker daemon, they are not independent in the strong sense — a daemon-level fault
could in principle have affected both. Each run does use its own network, volumes and
container names, so they do not share holder state, vendor state or sockets.
