# Protected-auth provisioning for the cashu-cdk-seller seat

The seat builds, boots and passes `doctor` with 17 checks, and then **refuses to advertise**. That
refusal is correct behaviour, not a defect, and this document is the exact supported path out of it.

Everything below was read from this repository at commit `aa8e831`'s tree, not recalled. Nothing
here required reading, copying, or holding a secret value, and none is included.

## What the seat is actually blocked on

Observed at boot:

```
pre-advertise probe FAILED claude: ... {"code":-32000,"message":"Authentication required"}
prove-before-advertise: none of 1 configured harness(es) produced a probe artifact; refusing to advertise
```

The cause is documented in the product itself (`crates/maxplayer-core/src/home.rs:2198-2205`):

> THE CREDENTIAL DOES NOT CROSS INTO THE CONTAINER. A container inherits no home directory and no
> macOS Keychain, so a `claude /login` credential is unreachable inside the container. `doctor` still
> passes — it runs no agent turn — but the pre-advertise probe runs INSIDE the container and FAILS,
> so the seat never advertises rather than advertising and failing every job.

So a host-side interactive login is not a provisioning path for a Docker seat. The daemon must hold
the credential in **its own process environment**.

## The required provider and credential

This seat runs `maxplayer seller --agent claude`, so the provider is **Anthropic**.

The credential is **`CLAUDE_CODE_OAUTH_TOKEN`**, produced by `claude setup-token`.

`ANTHROPIC_API_KEY` is explicitly the wrong choice here, and this is the trap worth naming:
Claude Code prompts **once** to approve an API key found in the environment rather than using it
silently, and a daemon has nobody to give that approval. The probe therefore fails on a machine
where the variable is plainly set and looks correct (`docs/SELLER-QUICKSTART.md:490-495`). It is
also the usage-billed Console path rather than a subscription login (`:381-383`).

The daemon reads these names and no others (`crates/maxplayer-core/src/seller_exec.rs:301-311`,
`FORWARDED_AGENT_ENV`): `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`,
`ANTHROPIC_BASE_URL`, `OPENAI_API_KEY`, `OPENAI_BASE_URL`.

## How the value reaches the contained harness without being exposed

`CLAUDE_CODE_OAUTH_TOKEN` is one of the four entries in `CONTAINED_CREDENTIALS`
(`seller_exec.rs:2761-2789`). The mechanism:

1. The real value stays in the **daemon's** environment, on the host.
2. The container receives a **per-job placeholder** of matching shape (`sk-ant-oat01-` + 93 random
   characters), never the real value.
3. At egress the proxy substitutes the real value for the placeholder, for the one upstream that
   credential is bound to (`https://api.anthropic.com` via `ANTHROPIC_BASE_URL`).

The design is fail-closed: if a token were derived rather than sent verbatim, substitution would
miss and the job would fail to authenticate — a break, never a leak. A job that reads its own
environment sees a placeholder that is useless anywhere else and dies with the job.

This is also why the containment story does not weaken to make auth work: the seat keeps its
dedicated `maxplayer-cashu-jobs` network and per-job egress proxy, and the credential is contained
*because* of them, not in spite of them.

## Two assumptions that do NOT hold — checked, not assumed

- **OpenClaw `SecretRef` is not a maxplayer feature.** Maxplayer reads credentials from its own
  process environment (`FORWARDED_AGENT_ENV`) or from `[sandbox] file_credentials`; there is no
  SecretRef resolution anywhere in the credential path. A SecretRef written into `config.toml`
  would be forwarded as the literal string.
- **An OpenClaw opaque env sentinel cannot serve as the daemon credential.** The sentinel exists
  only inside a single gateway-host command invocation. The seller daemon is a long-lived process
  that must hold the value for the life of the seat, across restarts and every job it launches.
  Nothing about a per-run injection persists into that process.

- **`[sandbox] file_credentials` cannot express a `claude /login` credential.** It reads exactly one
  **top-level** JSON field (`crates/maxplayer-core/src/home.rs:761-779`); the OAuth file that
  `/login` leaves behind nests the token under a parent object. That option is built for clients
  like `cursor-agent` that need an argv endpoint flag, and it is not the path for claude.

## Current protected-store state

The OpenClaw protected store holds six entries: five Discord bot tokens and one gateway token.
**There is no model credential in it**, and this seat could not use one from there anyway, for the
two reasons above. Nothing in the store was read to write this document.

## The concrete human setup step

This is a human decision and a human action. It requires a Claude subscription account, and whoever
runs it holds the token; I neither see nor handle the value.

1. In your own terminal, as the account you want this seat to bill against:

   ```bash
   claude setup-token
   ```

   This prints a long-lived, **model-only** token. It is not a full account credential.

2. Put it in a mode-`0600` environment file owned by the user that runs the daemon, as
   `CLAUDE_CODE_OAUTH_TOKEN=...` — one line, nothing else. Do not paste it into chat, a commit,
   `config.toml`, shell history, or a ticket. Suggested location for this seat:
   `~/forge/v2/seats/cashu-cdk-seller/daemon.env` (`chmod 0600`).

3. Start the seller with that file sourced into the daemon's environment only — for example
   `set -a; . ~/forge/v2/seats/cashu-cdk-seller/daemon.env; set +a` in the shell that launches it,
   or `EnvironmentFile=` on a systemd unit. The launch command itself is unchanged from
   `RUNBOOK.md`.

4. Confirm the seat advertises: the boot log should show the pre-advertise probe producing an
   artifact instead of `Authentication required`, and the seat then publishing kind-0 and
   kind-30340.

Step 4's output is the evidence that is missing today. Until a human completes steps 1–3, no
discovery evidence can exist, and none is claimed.

## What is still unproven after that

Provisioning auth proves auth. It does not by itself prove the contained harness end-to-end. Once
the seat advertises, the remaining evidence to capture is: a probe artifact from inside the
container, kind-0 and kind-30340 discovery events, and one **test-only** job run end to end. No paid
job, no funding, no open admissions.
