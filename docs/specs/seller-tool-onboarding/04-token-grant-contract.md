# 04 — Token, grant and custody contract

Paper artifact. **PROPOSED** throughout — gap G-6 in [01](01-integration-survey.md) records
that no grant, token, budget or holder concept exists in this repository in any form. Anchored
in plan v3 §4.

## Trust boundary

Trusted: the holder supervisor, and the genuine pinned CLI running inside the holder.
Untrusted: buyer prompts, buyer-supplied inputs, and the job container — always, including
when the job container is one the seller's own stack launched.

Plan v3 §4 chooses **trusted credential-reading children** over supervisor-only custody,
because most real tools cannot be driven any other way. That choice imports an obligation,
stated here so it cannot be quietly dropped:

> A container does not prove a CLI will not disclose its credential. The profile's reviewed
> input semantics, egress policy, output handling and isolation are all load-bearing, and a
> profile that cannot establish all four is unsupported in the initial release.

## Enrollment

The seller enrolls **interactively, inside the persistent holder**, through a protected local
terminal or a vendor browser flow. The tool writes its own authentication state into that
environment.

- Secrets never enter chat, command arguments, manifests, checker logs or notices.
- A host login is **not** assumed portable into the holder. Initial login happens in the
  holder (plan v3 §7), not by copying a host profile. This is the specific point on which the
  existing `codex_subscription.rs` precedent diverges — it is host-held and injected — so that
  module may contribute ideas but not its custody model (gap G-7).
- The seller may be needed for initial enrollment. Jobs do not re-enroll while the session
  stays valid.

## Per-job environment

At invocation the supervisor binds:

| Element | Binding |
| --- | --- |
| `HOME` | a private per-job directory, with a controlled configuration base |
| cache | private, per-job, writable, discarded at close |
| credential store | **only** the profile-selected store, exposed to the trusted child |
| input | the holder-private staging directory ([03](03-command-policy-mapping.md)) |
| output | the holder-created private slot |
| egress | reviewed per profile; default deny |

Never available: any buyer container's view of the credential mount, unrelated host files,
other jobs' directories, the Docker socket, host process namespaces.

The existing `SandboxPolicy::forward_env` allowlist (`seller_exec.rs:612`, applied by
`forwarded_agent_env` `:913`) is the right shape to build on and is deliberately testable
against an injected lookup. It is an **environment** allowlist only; it is not a credential
store selector, and it must not be described as one.

Read-only credential mounts are used **only** for tools that actually tolerate them. Declaring
read-only for a tool that must refresh is how the next section's bug appears.

### Credential maintenance

Tools that must write refresh tokens or profile state use a **serialized
credential-maintenance operation outside job control**. Only its designated auth-store writes
persist. Job-generated cache and config never merge back into the credential base.

If a tool cannot separate auth-store writes from job state safely, that profile is marked
**unsupported in the initial release**. Browser profile mutation and isolation are deferred —
a lock file does not solve them.

## Grant issuance

The seller approves, in holder policy (surfaced through the manifest's `grant_policy`): allowed
job **opener identities**, **parties**, **service IDs**, **verbs**, **resource sets** and
**ceilings**.

On job creation the holder validates the request against **both** that policy **and the
authoritative job record**, and rejects excess rather than silently narrowing to the allowed
subset. Two rules that are easy to lose:

- **An authenticated opener cannot grant itself more authority.** Being allowed to open jobs is
  not being allowed to choose their scope.
- **A policy change cannot broaden an existing job.** Grants are versioned; a running job keeps
  the grant version it was issued.

Connection point: immediately after `record_award` (`seller_node/store.rs:1590`) and before
`mark_executing` (`:1699`) — see [01](01-integration-survey.md) for why award, not offer.

## Token shape and verification

The holder issues a token bound to: **holder, party, service, job ID, grant version, expiry.**

Every call verifies, before any child process exists:

1. signature;
2. audience (this holder);
3. time, against a controlled clock;
4. an **active** job record in the authoritative store;
5. party equality and service equality against that record;
6. verb membership and resource membership in the grant;
7. remaining budget.

**Claims alone never override the record.** A token whose claims say `job=J, resource=R` while
the record says `J` is closed is a rejection, not a permitted call. Because the seller-path job
id is a bare `String` (gap G-5), this record re-check is the *only* thing standing between job
`K`'s valid token and job `J`'s resources — there is no type-level protection. Test 6 in
[07](07-test-entrypoints-and-evidence.md) exists specifically to hold that line.

## Close

Close happens on success, failure, cancellation or timeout, and is **atomic**: it denies new
calls, revokes mediated tokens, stops owned processes, and removes job data.

- Closed-job records **persist through token expiry**; a record cannot be forgotten while a
  token naming it could still be presented.
- Restart **fails closed** until active records are reconciled.
- Renewal cannot revive a closed job. Neither can a restart.
- Process-group kill is **insufficient**: descendants can escape it. Lifecycle control is
  supervisor-owned container/cgroup or equivalent, and test 9 explicitly starts a descendant.

Existing states cover Delivered | Paid | Failed via `is_finished()` (`store.rs:743`).
Cancellation and timeout have no store state (gap G-2), so the timeout path driven by
`job_timeout_secs` must be wired to close explicitly. An unwired timeout is a job that stays
open past its budget — the failure this contract most wants to avoid.

### Residual, recorded rather than solved

**Vendor operations already accepted can outlive local cancellation.** Closing a job stops our
calls; it does not undo a send, a charge or a publish the vendor already accepted. Counters
bound *admission*, not consequence. This residual is disclosed to the seller and is never
described as mitigated.

## Budgets

Reserve calls, items and bytes **atomically before execution**, using the profile-declared
**maximum** effects, not the observed ones. Refuse operations whose maximum is unbounded.

- Counters survive restart.
- Counters do **not** reset on token renewal.
- Counters reset only for a separately authorized new job.
- Refund only reservations **proved** unused.

Concurrency is the interesting case: two calls whose combined declared maxima exceed the limit
must not both admit. That is why reservation precedes execution, rather than accounting
following it.

## Holder sharing

Separate holders for different parties and vendors. Where a holder is shared
(`party_scope: shared-holder`), **sharing a holder never grants one job another job's
resources** — the grant check above enforces it, and test 8 demonstrates it.

Seller hosting is the initial scope. Platform-hosted credential custody is deferred.

## Lifecycle failure

When the vendor expires authentication mid-operation, the holder:

1. returns a credential-expired result for the in-flight call;
2. marks itself unhealthy;
3. queues **one** seller notice, containing no secret;
4. pauses registration and refuses new work.

Recovery is protected re-enrollment plus a successful health check, after which a **newly
authorized** job may run. The job that failed stays closed. **No write auto-replays.**
