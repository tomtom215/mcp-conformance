<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Design note: trace-validation architecture and trade-offs

**Audience:** maintainers of MCP SDKs and the official conformance suite, and
anyone weighing whether (or how) to reuse this approach. It is written to be
linkable from an upstream issue or PR thread without the reader needing the
rest of this repository's planning documents. The internal workspace layout and
dependency rules live in [`02-architecture.md`](../plan/02-architecture.md); this
note is the *why*, stated as decisions and their costs.

The toolkit judges Model Context Protocol implementations for conformance to a
specific spec revision (`2025-11-25` today). The central design question is not
"what does the protocol require" — the official spec answers that — but **how do
you produce a conformance verdict that an ecosystem with its own authoritative
test suite has reason to trust.** Every decision below is in service of that.

## 1. Judge recorded traces, not live sessions

The validator's input is a *trace*: an ordered, serialized record of a protocol
interaction (JSON Lines; schema in [`02-architecture.md`](../plan/02-architecture.md)
and a worked example in the [README](../../README.md)). Its engine is a pure
function — `&[TraceEvent] -> Report` — with no network, no clock, no randomness,
and no I/O of its own. Capturing the trace is somebody else's job (the
everything-server's session tap, the reference host's capture wrapper, or any
external proxy); judging it is the validator's only job.

This split is the foundational decision, and it has a clear cost: **trace
capture fidelity becomes the hard problem.** Interleaving, partial writes, and
SSE framing all have to be recorded faithfully, by whichever component owns the
socket, with the total-order `seq` assigned *at capture* and never inferred
later. We pay that cost deliberately, because validating an artifact rather than
a live session buys three things a live harness cannot:

- **Determinism and replayability.** The same trace yields a byte-identical
  report forever. A regression is a diff, not a flake.
- **Decoupling of capture from judgment.** A trace can be produced by any
  language, any SDK, any transport, and judged by one engine. The judgment does
  not care how the bytes were obtained.
- **Auditability.** A verdict can be reproduced from a committed file by anyone,
  which is what makes a third-party conformance claim defensible rather than a
  "trust us."

## 2. The judge never links the SDK it judges

`mcp-trace-validator` does not depend on `rmcp` (the official Rust SDK), and the
dependency rule is enforced structurally, not by convention: the validator's
crate may depend on the pure-data core and nothing protocol-bearing. A judge
that links the SDK under test inherits that SDK's interpretation of the spec as
ground truth — and an SDK's interpretation is exactly the thing under
examination. If `rmcp` serializes a field a particular way, a validator built on
`rmcp`'s types would call that serialization correct *by construction*.

The cost is real: we re-express the protocol's normative content as our own data
(Section 3) instead of reusing the SDK's types. The benefit is that the
validator's verdicts are independent of any implementation, including the one
this project itself ships as a reference server.

## 3. The spec as data: the requirement registry

Every normative clause of a revision is extracted into a record:

```text
Requirement {
    id:        stable, never-reused identifier (area prefix + ordinal)
    level:     Must | MustNot | Should | ShouldNot | May   (RFC 2119)
    actor:     Server | Client | Both
    source:    { revision, section anchor, verbatim quote }
    capability: optional capability key gating applicability
    checks:    [check ids]   |   exclusion: documented reason
}
```

Three properties of this format carry weight:

- **Verbatim source quotes.** Each entry stores the exact spec text it encodes,
  not a paraphrase. This makes the registry auditable against the published spec
  and lets a scheduled job (`cargo xtask spec-drift`) re-verify every quote
  against the live text, so the registry cannot silently drift out of date as
  the spec is edited. The current `2025-11-25` registry is 140 entries (51
  judged by 47 checks, 89 documented exclusions).
- **`checks | exclusion` is an exclusive alternative.** A requirement either
  maps to one or more mechanical checks, or it carries a written reason it
  cannot be judged from a trace (e.g. server-internal state that never reaches
  the wire). There is no third "not yet looked at" state. "Every MUST on an
  in-scope page enters the registry — no exceptions" is the invariant; the cost
  of an unjudgeable MUST is a sentence explaining why, pointing at the test that
  proves the exclusion holds.
- **The shape is deliberately SEP-2484's traceability shape** (`sep-NNNN.yaml`:
  each MUST/MUST NOT mapped to a check or a documented exclusion). Storing the
  registry in the same shape means upstream review of conformance scenarios
  doubles as review of our registry — one format, not two.

## 4. Determinism: a verdict that flickers is not a verdict

All JSON is canonicalized before any comparison: object keys ordered by RFC 8785
(JCS) UTF-16 code-unit order, numbers serialized in the ECMAScript form RFC 8785
§3.2.2.3 requires, validated against the RFC's own Appendix B vectors. Parsing
relies on `serde_json`'s `float_roundtrip` feature — without it, float parsing
can be one ULP off the serializer's own output, which would break the
canonical-fixpoint property the corpus tests assert.

This is more pedantic than it first looks, and a real bug found the edge:
canonicalization must satisfy `canonical(parse(canonical(v))) == canonical(v)`
for *every* value, including those JCS folds (`-0.0 -> 0`, `2.0 -> 2`). An
earlier fuzz target asserted representational identity instead and survived only
until a fuzzer first generated `-0.0`. Determinism is a property you have to
*test as a fixpoint*, not assume.

## 5. Applicability: not-applicable is not pass

A requirement gated on a capability that was never negotiated is reported as
**not-applicable**, never as passed. Inflating a pass rate with vacuous checks —
counting "the server correctly did nothing about a feature it never advertised"
as a win — is precisely how a conformance tool loses credibility. The capability
matrix is a pure function from the negotiated capability sets to the active
requirement subset; the report distinguishes pass / fail / warn / not-applicable
/ excluded as separate totals so a reader can see what was actually exercised.
(The decision and its rationale: [ADR-0006](../plan/decisions/0006-capability-gated-applicability.md).)

## 6. Calibration: the agreement check

This is the credibility mechanism, and the part most worth borrowing. The
validator does not ask to be trusted; it is **continuously calibrated against the
authority.** On every CI run, the everything-server is driven by the *official*
conformance suite, the same sessions are captured as traces, and the validator's
verdicts are diffed against the official runner's verdicts over those sessions.

- **Agreement is the default and disagreement fails CI.** Any divergence must be
  triaged into exactly one of three classes — our bug, an official-suite bug
  (filed upstream), or a spec ambiguity (filed upstream) — and recorded with the
  upstream link. An unexplained divergence is a build failure.
- **The gate holds in both directions.** A recorded divergence that no longer
  explains anything in the current run is *stale* and also fails CI, so the
  triage ledger cannot rot into a pile of permanent excuses.

On the server side this currently reconciles 30 tapped sessions at zero
unexplained divergence (the first run surfaced one real suite bug and one
informational warning, both recorded); the client side reconciles the reference
host's captured sessions the same way. The point is structural: a validator
whose verdicts are diffed against the recognized authority on every commit is
calibrated, not merely asserted to be correct.

## 7. Falsifiability: every check has a killer trace

Every check is killed by at least one committed injected-violation trace in the
corpus — a trace the check *must* flag, or the test suite fails. A check with no
killer trace is unfalsifiable: it could be a no-op and nothing would notice. The
corpus is therefore both the regression suite and the evidence that each check
does what it claims. Mutation testing closes the same loop from the other side:
a surviving mutant is a behavior change no test observed, and the gate treats
zero survivors as the floor.

## 8. What this buys, and what it costs

| Decision | Buys | Costs |
|----------|------|-------|
| Validate traces, not live sessions | Determinism, replayability, language/SDK-agnostic input, auditable verdicts | Capture fidelity is now a first-class engineering problem |
| Judge never links the SUT's SDK | Verdicts independent of any implementation | The spec must be re-expressed as our own data |
| Spec as data with verbatim quotes | Auditable, drift-checkable, SEP-2484-shaped | Manual extraction and upkeep per revision |
| RFC 8785 canonicalization everywhere | Byte-identical reports across platforms/runs | Fixpoint correctness must be fuzzed, not assumed |
| Capability-gated applicability | Pass rates mean something | More report states to model and explain |
| Agreement check against the official suite | Continuous calibration; credible third-party claims | A Node-based official runner in CI orchestration (never in the validator) |

## 9. Versioning, and the cost of a behavioral break

A conformance tool's *verdicts* are part of its public contract, not just its
Rust API. A change that makes a previously-passing trace fail — a requirement
moving from a documented exclusion to a judged check, say — is a breaking change
even though no function signature changed. The project treats both kinds of
break the same way: named explicitly in the changelog, with the version bumped
accordingly (pre-1.0, minor releases may break and say so). API-signature
compatibility is additionally checked mechanically — `cargo xtask semver` runs
`cargo-semver-checks` against the published crates.io baseline — so the
*behavioral* breaks the changelog must call out are never confused with
accidental API breaks it failed to declare.

## 10. Why this lives here, and how it connects upstream

The default home for generically useful protocol work is the official
repositories; this toolkit exists for what does not fit there — an independent,
trace-based judgment layer and the reference implementations that prove it. The
registry's SEP-2484-shaped storage, the everything-server's suite parity, and
the agreement check are all designed to be *offered* upstream rather than to
compete: the everything-server has been offered to `rust-sdk`, suite
discrepancies found by the agreement check are filed against the conformance
repo, and the registry format is the one the SEP process already uses. If a
piece belongs upstream, the aim is to move it there with this note as the
rationale a reviewer can read first.
