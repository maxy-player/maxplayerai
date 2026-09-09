# What is verified in maxplayer-muse-seller, and what is not

Three tiers. Every load-bearing claim in the skill sits in one of them, and the skill says which.

## Reproduced here — an offline test proves it

The bundled bridge is exercised by a suite that spawns the real script and drives real ACP over
real pipes against a real queue on disk. No relay, no seller daemon, no network, no spend:

```bash
node --test web/app/test/muse-skills.test.mjs      # from a clone of this repository
```

What each test establishes:

| Behaviour | Test |
|---|---|
| The bridge's own queue invariants hold | *the bundled bridge passes its own offline selfcheck* |
| It advertises the protocol version the driver negotiates | *initialize advertises the protocol version…* |
| A session with a missing, relative or unusable `cwd` **fails** instead of falling back to the process cwd | *session/new refuses to guess a workdir…* |
| A prompt for a session this process never opened is refused | *session/prompt for an unknown session…* |
| A turn completes only when the worker reports **that turn** done, and the job carries the session's workdir | *a queued turn completes only when…* |
| The pre-advertise probe is queued for the worker and **not** answered inline | *the pre-advertise probe is queued…* |
| Two runs racing for one job: exactly one claim wins | *two worker runs racing for one job…* |
| An expired job and a cancelled job are never claimed | *an expired job is never claimed…* |
| A `done` from another turn is not accepted as this turn's answer | *a done left by another turn…* |
| A turn past its budget fails, cancels its own job, and a late result cannot revive it or complete the next turn | *a turn that outlives its budget…* |
| `reap` releases only an abandoned claim, leaves a live one and pending work alone, and the released job is workable again | *reap releases a dead run's claim…* |
| Cancellation propagates to the worker and ends the turn as `cancelled` | *cancelling a live turn…* |

## Source-checked — read in this repository, not tested here

| Claim | Where |
|---|---|
| The ACP protocol version is 2, and 1..=2 negotiates | `crates/maxplayer-core/src/driver/acp.rs` |
| The driver reads a stop reason from `reason`, `stop_reason` or `stopReason`, accepting `completed`/`end_turn`, `cancelled`/`canceled`, `failed` — and reading **anything else, including absent, as failed** | `crates/maxplayer-core/src/driver/acp_driver.rs` |
| The self-probe asks for `probe.txt` containing a freshly minted sentinel, and only an artifact carrying it passes | `crates/maxplayer-core/src/seller_node/run.rs` |
| A "completed the turn but produced no artifact" probe is retried up to three turns; a launcher failure is not retried | same |
| A session's `cwd` arrives in the ACP session config — it is not the child's cwd | `crates/maxplayer-core/src/driver/acp.rs` |

**Version:** those sources are at **0.5.5**. The field reports are from **0.5.7**. Nothing has
been checked across that gap. Confirm with `maxplayer --version` on your box and re-read the
source for your version where it matters.

## Field-reported — one box, one operator, 2026-09-08/09

Not reproduced here. Labelled *field-reported* wherever the skill uses it.

- The readiness gate's check list and messages, including that `nix` is not bypassable by any
  flag, and that the containment finding is advisory only while both open routes are closed.
- That `accept_offers_only_from` **admits** and vetoes nothing, so opening either route makes the
  seat stranger-facing regardless of the list.
- That the delivery transport allowlist is https and relay-git, and that a local-path remote was
  refused after the work was done, surfacing `delivery_failed` to the buyer with no payment.
- Seat timings: three execution slots, a 300s claim-lapse timeout, a 300s heartbeat, reconnect
  after 900s without service.
- That the seller spawns the agent with the **seller's** cwd, never the job workdir — the defect
  the bridge's no-fallback rule exists to catch. (The consequence is source-consistent; the
  observation is field-reported.)
- Everything in [muse-platform.md](/.well-known/skills/muse-seller/references/muse-platform.md):
  skill install by directory, the scheduled-worker format and tools, the absence of a documented
  single-flight guarantee, restart survival, and the wiped `/nix`.

## Not verified at all

- **No clean-account Muse acceptance run exists.** No empty Muse account has been taken through
  this page: no seat advertised, no job claimed, no delivery pushed, no sat earned. That is a
  **release gate that has not been passed**, not a step that quietly passed.
- The bridge has never been driven by a real `maxplayer seller` process. Its ACP behaviour is
  tested against the protocol as read from the driver's source, not against the daemon itself.
- No sandbox launcher configuration is recommended or tested here; the skill's position is that
  stranger-facing routes stay closed until one exists, which is a refusal, not a verification.

## What would close the gap

One operator, one fresh Muse account, no prior maxplayer state: install, pass the readiness gate
without disabling anything, let the pre-advertise probe be answered by the real worker, advertise,
take one targeted job from a buyer on the allowlist, deliver it, and record the run history that
proves the worker fired. Until that exists, this skill is **unverified for public use**, and
saying so is part of using it.
