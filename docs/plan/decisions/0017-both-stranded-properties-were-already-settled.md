<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0017: Both Stranded Properties Were Already Settled — and the Rule That Settles One Is Thin

**Date:** 2026-08-27
**Status:** Accepted (corrects [ADR-0016](0016-no-reservation-primitive.md)'s third negative
consequence; extends its decision 3 to a second boundary)
**Author:** Tom F.

---

## Context

[ADR-0016](0016-no-reservation-primitive.md) closed the actuation-profile direction and, in its
negative consequences, left two properties open:

> **The two surviving observable properties are stranded.** Write idempotency under retry and
> bounded error recovery are judgeable and unclaimed, and this ADR does not pick them up […] If
> they matter they need their own decision, and until then this record has named a gap it
> declined to fill.

Picking them up was the next decision. It took one pass over the registry to establish that
**neither is a gap, and the sentence above is wrong on both counts** — written 2026-08-27 and
refuted the same day, before anything was built on it.

**Write idempotency under retry is claimed, and the spec answers it in the opposite direction.**
Retry at `2026-07-28` is MRTR — Multi Round-Trip Requests, SEP-2322, which replaces
server-initiated requests (register [1.5a](../01-ecosystem-context.md)). Five clauses cover it and
every one carries an implemented check: `MRTR-015` (`mrtr.retry-carries-input-responses`),
`MRTR-016` (`mrtr.request-state-echoed`), `MRTR-018` (`mrtr.no-unsolicited-request-state`),
`MRTR-020` (`mrtr.request-state-scoped-to-retry`), and `MRTR-019` with `TOOL-023`
(`mrtr.retry-id-differs`). `MRTR-019` is the one that settles the question, and it settles it
against the framing that produced it:

> The JSON-RPC `id` MUST be different between the initial request and the retry, as they are
> independent requests.

A retry is a **new request carrying echoed state**, not a redelivery of the old one. There is no
duplicate-suppression semantics to check because the protocol does not have any: the idempotency
question was imported from transports that redeliver, and MCP does not. The gap was in the
framing, not in the registry.

**Bounded error recovery is already a registry entry, already excluded, and the exclusion is
right.** `CACH-011` is the clause verbatim — "Implementations that do choose to poll MUST apply
jitter and backoff" — and it reads:

> Both the antecedent (this client is polling) and the obligation (its intervals are jittered and
> backed off) are properties of a request *schedule*, measurable only in elapsed time — which
> checks may not consult (LIFE-015) — and only across a run far longer than one recorded
> session.

Two independent barriers, either of which is sufficient. Both were already documented.

**The finding worth keeping is the rule those exclusions lean on.** Sixteen exclusions across both
revisions invoke "checks may not consult time" — the sole ground for most, one of several for
`COMP-009`, `LOG-011`, `MRTR-009` and `TRAN-126`. Four name `02-architecture.md` directly
(`LIFE-015`, `LIFE-016`, `LIFE-017`, `TRAN-033`); `COMP-005` names the determinism rule without the
document; the rest reach it by citing `LIFE-015`. The
citation is not dangling: `02-architecture.md` §`mcp-trace-validator` does carry it, as the closing
five words of the Determinism commitment — "no clocks, no randomness in the engine". But that is a
statement about **how the engine is built**, sitting in a paragraph otherwise concerned with RFC
8785 canonicalization and float round-tripping. The exclusions use it to mean something
categorically larger: that an entire class of requirement — everything measured in elapsed time —
is **beyond judgment for good**, whatever the engine is later built to do. An implementation note
is carrying a scope boundary, and the two are not the same claim. `TraceEvent.ts` exists and is
documented "Never consulted by checks" (`trace.rs:169`), which is the invariant behaving correctly
in code; nothing states it as a limit on what conformance can ever answer.

This is the same shape as [ADR-0016](0016-no-reservation-primitive.md)'s decision 3, where the
single-transport property lived only inside `BASE-065`'s exclusion string, and it is the failure
[ADR-0015](0015-the-tier-2-premise-is-gone.md) named at length: a claim held in one place and
depended on from another that does not carry it in the form the dependants need.

## Decision

1. **No clauses are added and no work is picked up for either property.** Idempotency under retry
   is covered by five checked MRTR clauses; bounded error recovery is `CACH-011`, correctly
   excluded on two independent grounds. Adding anything here would mean inventing requirements the
   specification does not state, which the registry's "spec as data" premise forbids — and the
   `2026-07-28` extraction is complete across all fifteen in-scope pages, so an un-extracted clause
   is not an available explanation.

2. **[ADR-0016](0016-no-reservation-primitive.md)'s third negative consequence is recorded as
   refuted**, by amendment on that ADR rather than by editing its body. It is corrected the day it
   was written, which is the whole point: [ADR-0015](0015-the-tier-2-premise-is-gone.md) found the
   MSRV refutation propagating to nothing for twenty-nine days, and the lesson was that the
   derivation chain is the part that gets skipped. This ADR is the chain being walked.

3. **The time rule is promoted to a stated scope boundary in
   [02-architecture.md](../02-architecture.md), beside the single-transport boundary.** The
   Determinism commitment keeps "no clocks, no randomness in the engine" as an engine property,
   unchanged; the boundary states separately what the sixteen exclusions actually rely on — that a
   requirement whose falsification needs elapsed time is out of reach by construction. The two
   boundaries now sit together, which is where someone designing against either will meet them.

## Consequences

### Positive

- ADR-0016 is correct within a day rather than within a month, and the record shows the correction
  arriving through the chain rather than only at the source.
- The sixteen exclusions now cite a boundary that says what they use it for. A reader checking
  `LIFE-015` against `02-architecture.md` previously found a sentence about the engine and had to
  infer the rest.
- The idempotency finding is worth more than the decision it closes. "A retry is an independent
  request" is a fact about MCP that the framing had wrong, and it would have produced a
  wrong-headed check had this gone the other way.
- Two boundaries stated together are likelier to be read as a set — the beginning of an honest
  answer to "what can trace validation never tell you", which is a better thing for this project
  to be able to state than another profile.

### Negative

- **This ADR ships less than ADR-0016 did, and ADR-0016 already shipped little.** Its output is one
  paragraph in the architecture document and an amendment on another record. Two consecutive
  decisions whose net artifact is documentation is a fair thing to hold against the direction that
  produced them.
- **Three investigations have now closed with nothing built.** The MHS-adjacent thread has produced
  two rejections and a correction. That is the evidence working, and it is also three passes of
  effort with no change to the shipped validator; if the fourth also closes this way, the direction
  itself is the thing to re-examine, not the individual questions.
- **The boundary statement is prose, protected by nothing.** No gate checks that an exclusion citing
  a rule cites a rule that exists, or that the architecture document still carries it. The same
  coupling ADR-0015 accepted for `register-currency` reading Markdown tables applies here with less
  excuse, because this one could plausibly be checked: exclusion strings are structured data and the
  documents they name are in the tree.
- **Sixteen was counted by pattern-matching exclusion prose**, not by a typed relation. The number is
  a floor rather than an exact count — an exclusion that relies on the time rule without using any of
  the matched phrasings would not have been found, and nothing prevents one.

## Alternatives Considered

### Add idempotency and backoff clauses anyway, as project-defined requirements

Rejected, and it is the alternative that would do real damage. The registry's authority rests
entirely on every clause being the specification's text with the specification's RFC 2119 level,
verified live by `spec-drift` (412 quotes, 0 drifted). A clause this project wrote itself would be
indistinguishable in the report from one MCP wrote, and would turn a conformance verdict into an
opinion. If a requirement should exist and does not, the route is the SEP process
([07-ecosystem-engagement.md](../07-ecosystem-engagement.md)), not the registry.

### Propose jitter/backoff judgeability by making checks time-aware

Rejected. It inverts the determinism commitment for one excluded SHOULD. A verdict that depends on
elapsed time is not reproducible from a committed file, which is the property the whole product is
built to have ([trace-validation.md](../../design/trace-validation.md) §4, "a verdict that flickers is
not a verdict"). `CACH-011`'s second barrier survives regardless: the conduct spans a run far
longer than one recorded session.

### Fold this into ADR-0016 as an amendment and write no new record

Rejected on the log's own rule 4 — one decision per ADR, linked decisions get linked records.
ADR-0016 decided not to build an actuation profile. This decides that two specific properties are
settled and states a second boundary; it also *corrects* ADR-0016, and a record that amends itself
into being right is worth less than one that is corrected from outside and says by whom.

### Build the gate that checks exclusion citations resolve

Rejected for now, and it is the strongest of these. It is the mechanism the third negative
consequence asks for, and the data is structured enough to make it plausible. It is declined here
only because it is a separate decision with its own design surface — which citation forms are legal,
what a rule reference resolves *to*, whether the check belongs in `xtask ci` or the weekly job under
[ADR-0010](0010-deferral-ledger-and-scheduled-reverification.md) — and folding it in would violate
the same rule 4 invoked above. It should be the next thing considered in this area.
