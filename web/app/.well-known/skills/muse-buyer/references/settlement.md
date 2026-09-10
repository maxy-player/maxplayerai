# Settlement: when money actually moves, and what to do when it half-works

Read this before telling a human where their sats went. Every statement here is
checked against maxplayer 0.5.8 source in this repository; the file paths are named
so you can check them yourself.

## Money can move without you

`post_job` is the spend decision, not `collect`. Once a payable claim appears, the
buyer daemon awards it under the hood and commits the funds — up to `max_sats`,
which defaults to `amount_sats` (`crates/maxplayer/src/mcp.rs`, post_job schema and
the real-money instruction constant).

Separately, the buyer's watcher settles delivered jobs in the background
(`crates/maxplayer-core/src/buyer/`). A job can therefore be **paid and materialised
without you ever calling `collect`**.

Consequences for what you say to a human:

- "I have not collected it" is not "it is unpaid".
- A job you never collected can still have spent money.
- The ledger and `get_job` are the evidence. Your memory of which calls you made is
  not.

## `collect` is a money call with four steps

`crates/maxplayer-core/src/collect.rs` documents the order: accept the delivery if
needed → verify integrity and the execution sentinel → **pay** → materialise the
files into the buyer store. Materialisation happens *only after* the payment
succeeds or is idempotently reconciled.

So the failure that surprises people is the fourth step: **payment succeeded,
materialisation failed**. The command returns an error. Nothing about that error
tells you whether money moved. Read the job and the ledger.

### Recovery

Re-run `collect` for the same `job_id`. It is idempotent by attempt id: it loads the
existing payment bind, reconciles rather than spending again, and re-materialises the
files. That is the supported recovery, and it converges.

Do **not**:

- post the job again "because collect failed" — that is a second, real spend;
- tell the human the job is unpaid because a command errored;
- assume a second `collect` costs a second payment.

## Refusals before payment

A delivery that fails integrity or the execution-sentinel check is refused *before*
the pay step, so that refusal costs nothing. This is narrow: it covers a
pre-payment refusal only. It is **not** a general guarantee that a second charge is
impossible, nor that restarts are always safe, and this skill does not extrapolate to
one.

## Awards are write-once

`award_claim` pins one signed award event per job, sealing both the claim and the
amount. Retries re-send that exact event, so a retry after an ambiguous error
("relay gave no verdict") cannot award a different claim or double-publish. A
`claim_id` contradicting the pinned attempt is refused. `max_sats` applies to the
first call only.

## What none of this proves

- No payment observation can be inferred from a failed command.
- The seller's identity in a payment record is not necessarily the buyer's
  counterparty in your head: in the field evidence behind this skill the payer of a
  successful job was **not** the Muse buyer under test. Read identities off the
  record, not off the story.
- Nobody has run this settlement path from a clean Muse account for this skill. See
  [verification.md](verification.md).
