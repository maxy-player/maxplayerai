# 02 — Manifest schema sketch

Paper artifact. **PROPOSED** throughout: no loader, validator or schema file exists in this
repository today (see [01](01-integration-survey.md)). Anchored in plan v3 §3.

## What a manifest is, and is not

A seller manifest is a **selection**, not a program. It names a reviewed command-policy
profile and fills the values that profile declares open. It cannot introduce a command, an
argument shape, an executable, an environment variable, an endpoint or an effect.

The server accepts **neither shell strings nor arbitrary executable/argv requests.** There is
no field in this schema whose value becomes a command line, and no field whose value is
substituted into another field. A manifest that needs a capability its profile does not
declare is a request for a *new reviewed profile version*, which is a human review event, not
a manifest edit. A seller-agent's claim that a command is safe is not an input to that review.

## Layering

```
policy profile   (authored + reviewed by the seller org; pins executable, subcommand,
   ↑ selected by   constants, env allowlist, credential lookup, effects, and the
   │               source-to-sink mapping for every open field)
manifest         (authored by the seller; selects a profile version and supplies
   ↑ bound at      values for declared open fields, plus grant policy)
   │  job open
job grant        (issued by the holder per job; see 04)
```

Each layer may only narrow the layer above it. No layer may widen one.

## Top-level shape

```yaml
schema_version: 1                 # integer; server rejects unknown majors outright

offering:
  service_id: "invoice-render"    # stable id; equality-checked against the job record
  display_name: "Invoice rendering"
  party_scope: "per-party"        # per-party | shared-holder ; see 04 custody rules

holder:
  holder_id: "H-invoice-1"        # names an already-enrolled holder; never creates one
  enrollment: "interactive"       # interactive | vendor-browser ; declarative only —
                                  # the manifest never carries or triggers a credential

operations:                       # one entry per advertised verb; the union of these
  - verb: "render"                # verbs is the discovery list checked by test 5.2
    profile:
      name: "acme-render-cli"
      version: 3                  # exact; no ranges, no "latest"
      digest: "sha256:…"          # pins profile content; drift invalidates acceptance
    bindings:                     # values for fields the profile declares open
      format: "pdf"               # must be a member of the profile's enum
      page_limit: 20              # must lie inside the profile's integer bounds
      title: "hello-world"        # must satisfy the profile's literal-text grammar
    effects:                      # seller's ceiling; may only be <= the profile maximum
      max_calls: 3
      max_items: 3
      max_bytes: 8192

grant_policy:                     # who may open a job against this offering; see 04
  allowed_openers: ["opener:marketplace-core"]
  allowed_parties: ["party:P", "party:Q"]
  resources:
    allow: ["res:R"]
    # everything not listed is denied; there is no deny-list and no wildcard
  max_job_lifetime: "PT15M"
```

## Field types a manifest may supply

Exactly the seven types in plan v3 §3, and no others:

| Type | Constraint | Expands to |
| --- | --- | --- |
| bounded enum | member of the profile's fixed list | one complete argument, or one schema-defined field |
| bounded integer | inside the profile's inclusive min/max | one complete argument |
| boolean | true/false | presence or absence of one profile-declared flag |
| grant-bound resource identifier | must be a member of the job's granted resource set at call time, re-checked against the authoritative record | one complete argument |
| bounded literal text | satisfies the profile's declared grammar | one complete argument, as data |
| opaque uploaded-artifact handle | a handle the holder issued; never a path | the holder-private staged path (see 03) |
| holder-created destination slot | a slot the holder created | the holder-private output path |

Each expands to **one** complete argument or **one** schema-defined field. Never an embedded
substring of an argument, never a fragment concatenated with anything, and never recursively
expanded. A value that would produce two arguments, or half of one, is rejected at validation.

## Fixture literal-text grammar

For fixture profiles (plan v3 §3), `bounded literal text` is:

- ASCII letters, digits, space, underscore, dot, hyphen;
- length 1–64;
- first character alphanumeric.

Consequences, which the checker asserts as *examples of the grammar*, not as a universal
safety rule: `-x` rejects (leading hyphen), `@file` rejects (`@`), `x;touch y` rejects (`;`),
`hello-world` is valid literal data.

**Real profiles may declare a different grammar.** Tests for a real profile derive from *that*
profile's recorded grammar, and never assume that a `--` terminator makes an operand safe. A
terminator helps a parser; it does not establish what the vendor does with the operand.

## Validation order

A manifest is rejected before any child process exists. The server, in this order:

1. rejects unknown `schema_version` majors and unknown top-level keys;
2. resolves `profile.name` + `version`, and verifies `digest`; an unresolvable or drifted
   profile rejects;
3. checks each binding's declared type, bound and grammar against the profile;
4. rejects any binding for a field the profile does not declare open, and any missing
   required field;
5. rejects any embedded hole, template marker or substitution syntax in any value, and any
   value that would expand to other than exactly one argument/field;
6. checks the profile's own constants against the constant policy in [03](03-command-policy-mapping.md)
   — an unsafe constant rejects the profile even if every manifest field is clean;
7. checks `effects` ceilings are `<=` the profile maxima, and `grant_policy` is well-formed.

**Oracle for every rejection: a validation error and zero child calls.** "Zero child calls" is
measured by the fake vendor's own call counter, not by the server's self-report.

## Explicitly out of scope for a manifest

- Any executable path, image reference or subcommand — pinned by the profile.
- Any environment variable name or value — the profile's allowlist governs; environment
  starts empty except reviewed tool/runtime variables.
- Any credential, credential path or credential selector — see [04](04-token-grant-contract.md).
- Any host path. No raw host path enters the API in either direction.
- Any endpoint, account or tenant selector — profile constants bind these to seller policy.
- Deriving the allowed operation/resource set from offer text. Deferred by plan v3 §1.
