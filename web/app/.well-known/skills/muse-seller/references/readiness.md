# The seller readiness gate on a Muse box

`maxplayer seller` runs startup readiness checks before it will serve, and **refuses to start on
a blocking failure**. Everything below is *field-reported* from one seat's boot logs unless
marked otherwise; the authority is the gate's own output on your box, which names the fix inline.

Read your own gate output first. It is written to be read.

## The checks, and what each one is worth

| Check | What a PASS actually establishes |
|---|---|
| `nix` | A working `nix --version` ran **in this process's environment** — not in your login shell |
| credential helper | Not required; the seller signs in-process |
| seller key | The key file exists in the home |
| relay reachability | Connected **and** authenticated, right now |
| mint reachability | Every accepted mint answered |
| agent preset | The registry **resolves** your agent the way boot does — *resolution only*, never that it can run |
| telemetry | Capture is armed |
| sandbox launcher / credential containment / egress / image / engine floor | The containment posture is what you configured — including "no launcher, agent runs directly" |
| sandbox containment | **Advisory only while both open routes are closed.** Opening either makes it a failure |
| seat reachability | How many named buyers can reach you, and whether an open route exists |
| home permissions | Home and wallet are owner-only |
| harness credential permissions | Skipped for a raw `--agent-argv` with no preset label — the gate refuses to guess a path |

## The two that catch people

**`nix` has no escape hatch.** Not `--skip-doctor`, not any flag. The gate's own reasoning: a
readiness check asks whether the box is ready *right now*, while nix asks whether it can *ever*
do the work — a different kind of requirement, with no warn-and-serve mode. If your account
cannot get a working nix, it cannot run a seller. Report that as a blocker.

On a Muse box this bites twice, because the platform has been observed wiping `/nix` mid-day even
though it sits outside `~` (field-reported, once). So a seat that booted this morning can fail
the nix check this afternoon with nothing changed by you. Treat nix as **ephemeral state to
re-establish**, not a one-time install — and when you re-establish it, do it by the method your
platform actually supports. Do not paste an installer command from a field report into a box you
have not checked: an installer that unpacks despite failing is not a passing install, and a
symlink farm that happens to work is not a supported configuration. If the supported install path
does not exist on your account, that is the blocker to report.

**The agent-preset check is not an execution proof.** It says the registry resolves your
`--agent-argv`. A resolvable agent can still fail to run. Execution is proven at the
pre-advertise self-probe and nowhere else — which is why the bridge queues the probe to the real
worker instead of answering it itself.

## Warnings you will see on a correctly configured Muse seat

Two WARNs are normal for the recommended posture (no sandbox launcher, both open routes closed,
raw `--agent-argv` pointing at the bridge):

- **sandbox containment** — advisory *because* both routes are closed. This is the finding that
  turns into a hard failure the moment you set `claim_open_pool` or `accept_open_targeted`. The
  softening is bought by the routes being closed, never by the buyer list.
- **harness credential permissions** — the gate will not guess a credential directory for a raw
  argv with no preset label, so it does not inspect one.

Neither is a reason to disable a check. **A readiness check you turned off is not a check that
passed** — if you find yourself reaching for a bypass, you have found a blocker to report.

## When the gate refuses

Read the FAIL line: it names the check, what it looked for, and the fix. Fix that, then re-run
`maxplayer seller`. Do not:

- disable or skip the check;
- fake the condition it tests;
- route around a platform restriction to make it pass.

Symptom-indexed help for a seat that starts but misbehaves:
[maxplayer-debug-selling](/.well-known/skills/debug-selling/skill.md).
