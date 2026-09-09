# 08 — Named gaps, unsupported and deferred cases, next-stage needs

Plan v3 §6 stage 0's required output: "supported profile requirements and explicit unsupported
cases", plus the named gaps the advisor reviews. Plan v3 §5 is explicit that **named gaps are a
valid Blocked report, never a silent fallback or a weakening of the approved contract** — so
this file is written to be actionable against, not reassuring.

## A · Repository gaps

Each observed in [01](01-integration-survey.md) at `upstream/main` = `d7b94db`.

| ID | Gap | Severity | Blocks |
| --- | --- | --- | --- |
| **G-1** | No seller-authored offering/service/listing schema or tool manifest exists at all. A seller declares only `agent_command`, `rate_sats`, harness names, probed capability tokens and sandbox mode. | **High** | All of [02](02-manifest-schema.md). The manifest is entirely new surface with no loader, no validator and no reviewer. |
| **G-2** | No cancellation or timeout job state. `JobState` is `Awarded/Executing/Delivered/Paid/Failed`; `is_finished()` covers only the last three. Timeout is driven by `job_timeout_secs` in the exec path. | **High** | Atomic close on cancel/timeout ([04](04-token-grant-contract.md)). An unwired timeout leaves a grant open past its budget. |
| **G-3** | `McpServer` is `{ name, command }` (`driver/acp.rs:56`) — no image digest, no env allowlist, no credential-store selector, no effects declaration, no per-job tool scoping. | **High** | The rung-4 holder is a **new component**, not a configuration of `McpServer`. Any plan that reads as "just configure MCP" is wrong. |
| **G-4** | No event bus for job open/close — plain `SellerStore` method calls only. | Medium | A holder cannot subscribe; it must be invoked at named call sites. Close atomicity lands on the caller, not the store. |
| **G-5** | Job id is a bare `String` on the seller path; the two `JobId` newtypes are unused there. | Medium | No type-level protection against cross-job confusion. The per-call record re-check is the only defence; check 6 in [07](07-test-entrypoints-and-evidence.md) is its proof. |
| **G-6** | No grant, token, budget/reservation or holder concept exists in any form. | **High** | All of [04](04-token-grant-contract.md). |
| **G-7** | Credential custody today is host-held and vendor-specific (`codex_subscription.rs`), i.e. injected into the container — the opposite of in-holder enrollment. | Medium | Plan v3 §7's "initial login happens in the holder". The existing module contributes ideas, not a custody model. |

### What is genuinely reusable

Not everything is a gap, and the contract should not rebuild these:

- `SandboxConfig` docker mode mounting **only the per-job workdir** (`home.rs:550-552`).
- `SandboxPolicy::forward_env` (`seller_exec.rs:612`) and `forwarded_agent_env` (`:913`) — an
  env allowlist already written against an injected lookup so it is testable without mutating
  the process environment.
- `docker/maxplayer-sandbox` and `docker/maxplayer-netfilter` plus `sandbox_net.rs` /
  `sandbox_netns.rs` for egress control.
- From `codex_subscription.rs`: the pinned single upstream (`:10`), the token-lifetime margin
  measured against the job timeout (`:12`), and the no-`Debug` secret type (`:14-19`).

## B · Contract gaps found by the walks

| ID | Gap | Source |
| --- | --- | --- |
| **C-1** | The source-to-sink mapping is argv-shaped. It has no expression for nested body structure, so "one field → one complete field" is not checkable for JSON. | [06](06-walk-b-tenant-aware-http.md) B-1 |
| **C-2** | Declared maximum effects are profile constants, but batch endpoints make the maximum a function of body content. | 06 B-2 |
| **C-3** | Grant membership is defined over identifiers; APIs that select by predicate/filter cannot be grant-checked without evaluating the predicate. | 06 B-3 |
| **C-4** | Redirect and destination/callback fields are not covered by the constant policy's list, which was written for CLI options. | 06 B-4 |
| **C-5** | Call budgets sized for a CLI are exfiltration budgets under server-side pagination. | 06 B-5 |
| **C-6** | Whether a tool's credential refresh writes are separable from job state (assumption A5) decides supported vs unsupported, and there is no way to determine it except by inspecting each tool. | [05](05-walk-a-file-processing-cli.md) |
| **C-7** | Literal-text sink review is per-tool **and per-version**; nothing yet forces re-review at a version bump beyond the profile digest pin. | 05 |

C-1 through C-5 are all reasons HTTP is deferred; they are recorded as contract gaps rather
than tool gaps because a future HTTP profile must close them in the *contract*, not per tool.

## C · Explicitly unsupported in the initial release

Stated positively so nothing here can be read as "not yet tested":

1. **Any tool whose credential store cannot separate auth writes from job state** (C-6). Marked
   unsupported rather than shipped with a lock.
2. **Browser profile mutation and isolation.** Deferred; a lock file does not solve it.
3. **Rung 3 (key-swap proxy) and rung 5 (host executor) templates.** Recognized shape, template
   deferred.
4. **HTTP transport profiles.** Deferred pending C-1…C-5 and their own acceptance run.
5. **Public and direct-token routes.** Eligibility guidance only; **no automated adapter** in
   this release. The honest report is *manual setup required*, never *successfully onboarded*.
6. **Direct vendor tokens without enforceable revocation or vendor-enforced job-close binding.**
   Select mediation instead. An expiring stolen token is not harmless.
7. **Deriving the allowed operation/resource set from offer text.** Deferred.
8. **Manual work as a fallback route.** Deferred pending a separate product decision.
9. **Platform-hosted credential custody.** Seller hosting is the initial scope.
10. **Any command whose safety semantics are not covered by a reviewed profile.** A manifest
    alone cannot establish them; this is an explicit limit on generic onboarding.

`deferred`, `unsupported` and `success` are three distinct router results and must never be
collapsed into two.

## D · Supported profile requirements

The affirmative half of the stage-0 output. A profile may ship in the initial release only if
all of these hold:

1. **rung 4**, with the real tool holding its login inside a persistent isolated holder;
2. **argv-shaped invocation** with a complete source-to-sink row — source, validation/encoding,
   sink, vendor interpretation, allowed effect — for every field **and every constant**;
3. **constants that close the dangerous sinks**: no shell/eval, no plugin load, no
   job-selectable configuration, no secret-dumping debug mode, no uncontrolled destination;
   plus a non-interactive flag where the tool would otherwise prompt;
4. **declarable maximum effects** per invocation, bounded and reservable before execution;
5. **separable auth-store writes** (C-6), verified by inspection before build;
6. **single-file, in-slot output** into a holder-created slot, checked for symlinks, devices and
   out-of-slot escapes before export;
7. **a reviewed egress allowlist**, default deny, ideally a single pinned upstream;
8. **literal-text sinks re-reviewed at every profile version bump** (C-7).

A tool failing any of these is `unsupported` or `deferred` — a routing success, not a failure to
be worked around.

## E · Next-stage needs

Decisions and inputs stage 1 cannot start without, or must carry:

| Need | Owner | Note |
| --- | --- | --- |
| First real tool selection | **Petar** | Plan v3 §7. Not assumable by the worker; no live account access. |
| Supported initial platform confirmation | **Petar** | Plan v3 §7. The stage-1 *prototype* platform (custom CLI + fake service on Linux Docker) is already authorized and is a separate question from the supported release platform. |
| Acceptance of the proposed job connection point | **maxie / advisor** | Grant open after `record_award` (`store.rs:1590`), close on every `is_finished()` path **plus** the explicitly wired `job_timeout_secs` timeout (G-2). |
| Acceptance of the proposed kit location | **maxie / advisor** | New workspace member `crates/maxplayer-tool-kit`, rather than growth inside `maxplayer-core`. |
| Ruling on G-2 | **maxie / advisor** | Whether to add cancel/timeout job states, or wire close from the exec-path timeout without a store state. |
| Disposable-resource/effect allowance | **Petar or Josip** | Required before any write-producing live acceptance example. Absent it, live checks stop at one health request and one read. |
| Tool two selection | **Petar or Josip** | After freeze, for the unseen-tool acceptance test. |
| Router skill publication | **stage 2** | Through `skill_workshop`, never a direct `SKILL.md` write. |

## F · Standing honesty condition

Restated because it governs every artifact produced downstream of this contract:

> The stage-1 custom CLI plus local fake authenticated service proves the **mechanism only**.
> It is not independent real-tool acceptance and not general onboarding acceptance. Contrasting
> real-tool acceptance — two services on tool one plus one on a contrasting tool two, selected
> after freeze, with an independent oracle — is **retained in full** and receives no credit from
> any fixture-only or fake-vendor result.
