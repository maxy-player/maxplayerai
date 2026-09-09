# 07 — Intended test entrypoints and evidence layout

Paper artifact. **These are planned tests, not existing commands, and not measured security
guarantees.** Nothing here has been run. Anchored in plan v3 §5 and §6.

## Fixture defaults (plan v3 §5)

No internet. Fake vendor only. Synthetic credentials only. Parties `P`, `Q`; jobs `J`, `K`;
holders `H`, `G`; resource `R` allowed, `S` forbidden. Maximum 20 simulated invocations and
64 KiB generated data per case, reset between cases. No real spend, no external writes.

Tests apply to the stage-2 MCP-container CLI template unless noted. Deferred transport tests
are **SKIPPED with their deferred reason** — never quietly omitted, never counted as passes.

## Result vocabulary

| Result | Meaning |
| --- | --- |
| PASS | oracle satisfied, with evidence |
| FAIL | oracle not satisfied |
| SKIPPED | a dependency failed, or the case is deferred; **never a fake PASS** |

A manifest fault and a defective runtime are different fixtures and must not share one case.
Dependency failure propagates as SKIPPED to dependents, so a broken fixture reads as a broken
fixture rather than a clean sweep.

## Proposed entrypoints

**PROPOSED** — no such crate, binary or test target exists today (gaps G-1, G-6).

Location follows the kit crate proposed in [01](01-integration-survey.md):

| Entrypoint | Form | Purpose |
| --- | --- | --- |
| `cargo test -p maxplayer-tool-kit --test fixture_suite` | Rust integration test | checks 1–12 against the fake vendor |
| `cargo test -p maxplayer-tool-kit --test negative_controls` | Rust integration test | runtime mutants; each has a **required FAIL** |
| `maxplayer-tool-kit check --manifest <path> --fixtures` | CLI subcommand | seller-facing fixture checker |
| `maxplayer-tool-kit check --manifest <path> --live` | CLI subcommand | live checker, bounded per plan v3 §5 |
| `crates/maxplayer-tool-kit/fixtures/fake-vendor/` | fixture service | records every call; owns the authoritative call counter |
| `crates/maxplayer-tool-kit/profiles/` | reviewed profiles | content-addressed; drift resets acceptance |

Two entrypoint properties that matter more than their names:

1. **The call counter belongs to the fake vendor, not the holder.** "Zero child calls" is
   asserted from the vendor's record. A holder that reports zero while having called is exactly
   the defect the negative controls hunt.
2. **The clock is a fixture clock.** Expiry tests set time explicitly; none sleep.

## Check map

Each row: the plan v3 §5 check, its oracle, and its declared budget.

| # | Check | Oracle | Budget |
| --- | --- | --- | --- |
| 1 | Schema/profile: valid manifest loads; embedded hole, unknown profile, unsafe constant, text-to-URL mapping each reject | validation error **and** zero vendor calls | 0 calls for negatives |
| 2 | Discovery/result: list equals approved verbs; `render(R)` returns known bytes | independent expected file + digest; **not** exit code, **not** server self-report | 1 call |
| 3 | Operand grammar: `-x`, `@file`, `x;touch y` reject; `hello-world` arrives as exactly one literal operand | zero calls for negatives; expected bytes and no forbidden-effect marker for the positive | 1 positive call |
| 4 | Artifact consumption: forged handles; driver swaps symlinks/renames **during** staging and **after** validation | no outside-file-read marker; consumed bytes match staged digest; valid staged object may succeed only with its original digest | ≤2 calls, 8 KiB |
| 5 | Authentication: missing/garbage signature, wrong holder/party/service/job, natural expiry, post-expiry, each close outcome | reject with zero calls, against fixture clock and authoritative records; valid `J/H/P` succeeds once; includes same token after restart and after renewal against a closed record | 1 positive call |
| 6 | Grant authority: unauthorized opener, excessive grant request, forged claim grant version, verb/resource outside `J`, **`K`'s valid token against `J`** | zero unauthorized calls; seller-authorized `J/R` succeeds once | 1 positive call |
| 7 | Leakage: synthetic auth secret may appear **only** in the declared fake-vendor auth channel; a separate non-auth canary appears nowhere outside its forbidden store | every captured channel inspected, success **and** failure paths; failing to authenticate at the permitted sink also fails the positive oracle | 2 calls |
| 8 | Isolation: concurrent `J`/`K` cannot read each other's HOME/input/output markers; sequential `K` cannot see `J`'s config/cache mutations | instrumented child observes mounts and identities; forbidden-read marker zero; credential lookup still succeeds at its permitted store | 4 calls, 8 KiB |
| 9 | Cleanup: fixture child **with a descendant**; close/cancel/timeout `J` | no owned process or job filesystem within 5 s; repeated calls refuse; unrelated `K` still usable; repeat with natural expiry during an active child | ≤4 calls |
| 10 | Budgets: call=3, item=3, byte=8 set independently; exact boundary, one over, concurrent reservations exceeding the limit | only admitted effects occur; renewal/restart cannot reset; new authorized `K` has separate counters | ≤10 requests, 16 B/subcase |
| 11 | Lifecycle: fake vendor expires auth mid-operation | credential-expired returned, holder unhealthy, exactly one seller notice, registration paused, new work refused; after simulated protected re-enrollment + health, a **newly authorized** job runs; failed job stays closed; **no write auto-replays** | 4 calls |
| 12 | Registration: fail each mandatory check; pinned version/profile drift | rejection/pause; drift invalidates prior acceptance | 1 registration/variant, 0 vendor effects |

Check 7's last clause is the one most often lost in implementation: a run where the secret
leaked nowhere *because authentication never happened* is a FAIL, not a PASS.

## Negative controls — runtime mutants

Schema-invalid manifests are not sufficient. Each mutant below is a deliberately defective
**runtime**, with a named required FAIL and named dependent SKIPPEDs.

| Mutant | Required FAIL | Dependent SKIPPED |
| --- | --- | --- |
| bypass signature validation | 5 | — |
| omit audience/party checks | 5, 6 | — |
| leak auth to output | 7 | — |
| expose another job's mount | 8 | — |
| leave descendants alive | 9 | — |
| skip quota reservation | 10 | — |
| retain closed tokens | 5, 6 | 9 |

**Do not demand exactly one failing line.** A mutant may legitimately trip several checks; the
requirement is that the named check fails, not that nothing else does.

## Live checks

Only seller-approved test resources. **One health request and one read per run**, with an
independent expected result.

A write-producing acceptance example requires a **separate explicit disposable-resource/effect
allowance**. Stop at that allowance: lack of permission is a blocker, not permission to
improvise.

Remote black-box checks cannot prove internal isolation. Pinned artifacts and local isolation
evidence are recorded **separately**, and registration must not label an arbitrary mutable
endpoint secure on the strength of live checks alone.

## Evidence layout

One directory per acceptance run, content-addressed and append-only:

```
evidence/<run-id>/
  manifest.json          run id, UTC start/end, operator, git commit of the kit
  pins.json              kit commit, server digest, schema version,
                         each profile name+version+digest, fixture image digest,
                         fake-vendor digest, tool version(s)
  oracles/               expected results RECORDED BEFORE THE RUN
    <case>.expected      bytes or digest, plus provenance of the oracle
  cases/
    <case>/
      result.json        PASS | FAIL | SKIPPED, reason, dependency if skipped
      command.txt        exact argv, redacted only where a redaction rule names the field
      observations/      captured channels: stdout, stderr, vendor call log,
                         egress log, output slot listing, marker counters
      budget.json        reserved vs consumed calls/items/bytes
  interventions.json     every human intervention, or an explicit empty list
  skipped.json           every SKIPPED case with its reason
  summary.md             counts by result; NOT a pass/fail verdict on its own
```

Rules that make the bundle evidence rather than decoration:

- **Oracles are written before the run** and hashed into `pins.json`. An oracle produced after
  seeing the output is not an oracle.
- **Redaction is by rule, not by judgement.** A redaction rule names the field; ad-hoc removal
  of an inconvenient log line is a defect.
- **`interventions.json` is mandatory and may be empty**, because the stage-2 agent criterion
  is *zero* human interventions and an absent file cannot demonstrate zero.
- **`summary.md` never overrides `cases/`.** The per-case results are authoritative.

## Acceptance run wiring (plan v3 §6 stage 2)

- Two services on tool one, plus one on a contrasting tool two selected **after** freeze, with
  **no edits to the pinned kit**. Petar or Josip selects tool two; this is also the unseen-tool
  test.
- Advisor independently assesses eligibility against frozen capabilities **before** execution,
  and records required outputs, forbidden effects and the safe effect allowance.
- A haiku-class agent receives only the pinned kit, task inputs and an already-enrolled test
  holder; it may edit only the seller manifest and job input data. Within 40 turns and two
  hours, zero human interventions, it must pass applicable checks **and** produce the real
  tool's expected result. A human-produced artifact or independent vendor read is the oracle.
  **No fixture-only success.**
- The agent cannot decide its own unsupported escape; the advisor adjudicates against the
  predeclared policy. Correct `unsupported` classification is a routing success but **does not
  satisfy the required successful second-tool onboarding** — another eligible sample must be
  selected, or acceptance remains incomplete.
- Across all three services: verify out-of-grant and closed-token rejection; run all negative
  controls, concurrency, cleanup, budget and expiry/re-enrollment cases; independently verify
  real outputs; test registration rejection and subsequent pause.

## Relationship to the authorized stage-1 platform

The stage-1 platform — custom CLI plus local fake authenticated service on Linux Docker,
synthetic credentials — exercises the fixture entrypoints above and demonstrates login
persistence, useful artifact output and job isolation.

Its evidence bundle is written to the same layout, and is labelled in `manifest.json` with a
`mechanism_only: true` marker, because: **a CLI written to fit the contract cannot falsify the
contract.** A stage-1 bundle is not independent real-tool acceptance, is not general onboarding
acceptance, and contributes **zero** credit toward the stage-2 acceptance run described above,
which is retained in full.
