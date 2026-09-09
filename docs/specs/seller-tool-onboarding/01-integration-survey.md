# 01 — Integration survey, proposed kit location, real job connection point

Survey of `maxplayerai` at `upstream/main` = `d7b94db` ("release: cut v0.5.8"). Every claim
below carries a path; anything not observed is marked **NOT FOUND** or **INFERENCE**. Plan v3
§6 stage 0 requires this inspection before any contract is proposed, precisely so the contract
does not "invent config as already supported".

Workspace members (`Cargo.toml:2-8`): `crates/maxplayer-core`, `crates/maxplayer-desktop`,
`crates/maxplayer-evals`, `crates/maxplayer`, `crates/maxplayer-relay-write-policy`.
`crates/buzz/` is on disk but is **not** a workspace member (its own nested workspace).

## 1. Seller onboarding and configuration — what exists

| Thing | Where |
| --- | --- |
| `SellerConfig` | `crates/maxplayer-core/src/home.rs:193` |
| `SandboxConfig` | `crates/maxplayer-core/src/home.rs:550` |
| root `MaxplayerConfig` | `crates/maxplayer-core/src/home.rs:1471` |
| `load_config` / `save_config` | `home.rs:1923` / `home.rs:2224` |
| `require_seller_config` | `crates/maxplayer-core/src/seller.rs:54` |
| interactive onboarding | `crates/maxplayer/src/sell.rs`, `ensure_seller_config` `:318`, entry `run` `:75` |
| harness registry | `crates/maxplayer-core/src/seller_agents.rs`: `RegisteredAgent:51`, `AgentRegistry:130`, `resolve:263` |
| capability tokens | `crates/maxplayer-core/src/capability.rs:36` — `CAPABILITIES = ["node","python","rust"]`, probed by `probe_capabilities:145` |
| buyer-repo declarative config (per-job, **not** seller-authored) | `crates/maxplayer-core/src/checks.rs`: `DECLARATION_PATH = ".maxplayer/checks.toml"` `:11`, `parse_declaration:184`, 64 KiB limit `:14` |

`SellerConfig` fields are: `agent_command`, `rate_sats`, `takes_no_payment`, `git_remote`,
`job_timeout_secs`, `agents`, the offer-acceptance flags, and `slots`.

**NOT FOUND: any seller-authored offering / service / listing schema, any tool manifest, and
any per-offering declarative registration.** Today a seller declares an agent command, a rate,
harness names, a sandbox mode, and capability *tokens that are probed rather than declared*.
The word "onboarding" appears only in config-template comments (`home.rs:2092,2151`).

**This is the single most important survey result for stage 0.** Plan v3 §3's manifest has no
existing home, no existing loader, and no existing reviewer. The manifest in
[02](02-manifest-schema.md) is therefore entirely **PROPOSED** new surface, and every
"registration must fail closed" statement in this contract is a requirement on code that does
not exist — not a description of `load_config`'s behaviour.

## 2. Job lifecycle — the real connection point

Authoritative seller-side record is SQLite through
`crates/maxplayer-core/src/seller_node/store.rs`.

- States: `pub enum JobState { Awarded, Executing, Delivered, Paid, Failed }` (`store.rs:708`);
  `is_finished()` = Delivered | Paid | Failed (`:743`); stored spellings
  `"awarded"|"executing"|"delivered"|"paid"|"failed"`.
- Transitions are **plain method calls on `SellerStore`**: `record_offer:1258`,
  `record_award:1590`, `record_job_checks:1386`, `mark_executing:1699`, `mark_pushed:1714`,
  `deliver_and_enqueue:1726`. Query: `job_state(&self, job_id: &str):2754`.
- Job id is **a bare `&str`/`String`** throughout the seller store and exec path. It is the
  offer event id in hex (`crates/maxplayer/src/mcp.rs:82` documents the `get_job` parameter as
  "Offer event id (hex)"). Two unrelated `JobId` newtypes exist and are **not** used on the
  seller path: `event.rs:51` and `payment.rs:33`.

### Consequences the contract must respect

1. **There is no job-created or job-closed event bus.** There are function calls. A holder
   cannot subscribe; it must be invoked. The connection point is therefore a call site, and
   [04](04-token-grant-contract.md)'s "close is atomic" requirement lands on whoever owns that
   call site — it is not provided by the store.
2. **`is_finished()` covers Delivered | Paid | Failed.** Plan v3 §4 requires close on success,
   failure, cancellation **or timeout**. Cancellation and timeout are not distinct states here;
   `job_timeout_secs` (`home.rs`, `SellerConfig`) drives a timeout in the exec path rather than
   a store state. **Named gap G-2 in [08](08-gaps-and-unsupported.md).**
3. **An unwrapped `String` job id is a weak binding.** Plan v3 §4 binds a token to a job id and
   requires party/service equality against the authoritative record. With a bare string there
   is no type-level protection against passing job `K`'s id where `J` is meant. The contract
   compensates with the explicit record re-check at every call, and the cross-job test in
   [07](07-test-entrypoints-and-evidence.md) exists to prove it.

### Proposed real job connection point

**PROPOSED**, for maxie and the advisor to accept or replace:

| Hook | Call site | Obligation |
| --- | --- | --- |
| grant open | immediately after `record_award` (`store.rs:1590`), before `mark_executing` | validate opener/party/service/verbs/resources against holder policy **and** the awarded record; issue the token; reserve nothing yet |
| grant close | on every path that reaches `is_finished()`, **plus** the timeout path driven by `job_timeout_secs` | atomic close per [04](04-token-grant-contract.md) |

Attaching at award rather than at offer is deliberate: `record_offer` is not yet an authorized
job, and issuing a grant there would violate "an authenticated opener cannot grant itself more
authority". The timeout path must be wired explicitly because it does not pass through a store
state (gap G-2).

## 3. MCP integration — what exists

- `crates/maxplayer/src/mcp.rs` — maxplayer's **own** MCP **server**, exposing job tools to an
  agent (e.g. `get_job`, whose parameter doc is cited above).
- `crates/maxplayer-core/src/driver/acp.rs:56` —
  `pub struct McpServer { pub name: String, pub command: Vec<String> }`, alongside `acp_driver.rs`,
  `mock.rs` in `crates/maxplayer-core/src/driver/`.

So MCP exists on **both** sides: maxplayer serves job tools, and the ACP driver can be told to
launch MCP servers by `name` + argv `command`.

Critically, `McpServer` is **name + argv only**. It carries no image digest, no environment
allowlist, no credential store selector, no effects declaration and no per-job resource
binding. **NOT FOUND: any per-job scoping of MCP tool exposure.** The rung-4 "persistent
isolated holder" of plan v3 §2 is therefore *not* a configuration of `McpServer`; it is a new
component that would supply the pinning `McpServer` lacks. **Named gap G-3.**

## 4. Credentials, isolation and egress — what exists

This is the strongest existing foundation, and the contract should build on it rather than
beside it.

| Thing | Where |
| --- | --- |
| seller exec / sandbox policy | `crates/maxplayer-core/src/seller_exec.rs` |
| docker sandbox image | `docker/maxplayer-sandbox` (`DEFAULT_SANDBOX_IMAGE`, version-pinned by the binary) |
| network filtering | `docker/maxplayer-netfilter`, `crates/maxplayer-core/src/sandbox_net.rs`, `sandbox_netns.rs` |
| env allowlist | `SandboxPolicy::forward_env` (`seller_exec.rs:612`), applied by `forwarded_agent_env` (`:913`) over a built-in `FORWARDED_AGENT_ENV` set plus operator extras |

`SandboxConfig.mode` (`home.rs:550`) selects `launcher` (default) or `docker`; the docstring
states docker mode "runs the command inside a container that mounts ONLY the per-job workdir"
(`home.rs:552`). That per-job-workdir-only mount is real and is the nearest existing analogue
of the private per-job area in [04](04-token-grant-contract.md).

`forwarded_agent_env_from` is written against an injected lookup so the allowlist is testable
without mutating the process environment (`seller_exec.rs:918-923`) — the same testability the
checker contract needs.

### The closest existing precedent: `codex_subscription.rs`

`crates/maxplayer-core/src/codex_subscription.rs` is **host-only ChatGPT session support for a
contained Docker Codex run** (module doc, `:1`). It already implements, for one tool, several
things plan v3 §4 demands generally:

- a **pinned single upstream** the session may reach: `CHATGPT_CODEX_UPSTREAM =
  "https://chatgpt.com/backend-api/codex"` (`:10`);
- **token lifetime measured against the job budget**: `ACCESS_TOKEN_MARGIN` of 15 minutes of
  required remaining life beyond the job timeout (`:12`) — this is exactly plan v3 §2's
  "lifetime fits the job budget" predicate, already expressed in code;
- **secrets kept out of logs by construction**: `ChatgptSession` "deliberately has no `Debug`
  implementation because both fields must stay out of logs and errors" (`:14-19`), and
  `SessionError` carries no auth-file content (`:31`).

**INFERENCE:** this is a bespoke, single-vendor implementation of one rung, not a reusable
holder. It is host-held and injected into the container, which is the *opposite* of plan v3
§4's "initial login happens in the holder, not by assumed copying from the host". The kit
should reuse its three ideas — pinned upstream, lifetime-vs-budget margin, no-`Debug` secret
types — and must not present it as an existing holder.

## Proposed kit repository location

**PROPOSED.** Stage-0 paper lands where it now sits:
`docs/specs/seller-tool-onboarding/` (alongside the existing `docs/specs/free-job-lane.md`).

For stage 2, the proposal is a **new workspace member** `crates/maxplayer-tool-kit` rather than
growth inside `maxplayer-core`, because:

- `maxplayer-core` is already the home of the seller store, exec path and payment code; the
  holder must be reviewable in isolation, and a separate crate makes its dependency surface
  auditable;
- profile pinning and manifest validation want their own test fixtures and their own
  acceptance run, which plan v3 §5.12 resets on drift;
- **INFERENCE**: a separate crate makes "no runtime changes to existing behaviour" checkable by
  diff, which is what maxie's gate asks for at stage 0 and will ask again later.

Profiles and fixtures: `crates/maxplayer-tool-kit/profiles/` and `.../fixtures/`. The router
skill goes through `skill_workshop` at stage 2, never a direct `SKILL.md` write.

## GAPS — what this contract must not assume exists

- **G-1** No seller-authored offering/manifest surface exists at all. Everything in 02 is new.
- **G-2** No cancellation or timeout job state; `is_finished()` is Delivered|Paid|Failed only.
- **G-3** `McpServer` is name + argv; no digest pinning, env allowlist, credential selector,
  effects declaration or per-job tool scoping.
- **G-4** No event bus for job open/close — function call sites only.
- **G-5** Job id is a bare `String` on the seller path; no type-level job binding.
- **G-6** No grant, token, budget/reservation or holder concept exists in any form.
- **G-7** Credential custody today is host-held and vendor-specific (`codex_subscription.rs`),
  not in-holder enrollment.

Full treatment, with severity and what each blocks, in [08](08-gaps-and-unsupported.md).
