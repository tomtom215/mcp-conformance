<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Architecture

This chapter orients you in the design. The authoritative treatment — ten
decisions, each with its cost — is the standalone design note, written for an
upstream audience:

> 📄 **[Design note: trace-validation architecture and trade-offs](https://github.com/tomtom215/mcp-conformance/blob/main/docs/design/trace-validation.md)**

The internal workspace layout and dependency rules live in
[`02-architecture.md`](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/02-architecture.md).

## The foundational split: capture, then judge

The validator judges a recorded **trace**, not a live session. Its engine is a
pure function with no network, clock, or I/O. Whoever owns the socket — the
server's session tap, the host's capture wrapper, or any external proxy —
produces the trace, assigning the total-order `seq` *at capture*. The judge only
judges.

This buys determinism, replayability, SDK-agnostic input, and auditable verdicts.
It costs one hard thing: **capture fidelity becomes a first-class engineering
problem** — interleaving, partial writes, and SSE framing all have to be recorded
faithfully by the component that holds the bytes.

## The decisions that follow, in brief

- **The judge never links the SDK it judges.** `mcp-trace-validator` does not
  depend on `rmcp`. A validator built on the SDK under test would inherit that
  SDK's interpretation of the spec as ground truth — exactly the thing being
  examined. The cost is re-expressing the protocol as our own data.
- **The spec as data.** Every normative clause becomes a registry record with its
  RFC 2119 level, actor, a *verbatim* source quote, an optional capability gate,
  and either one or more mechanical checks **or** a written reason it cannot be
  judged from a trace. There is no "not yet looked at" state. A scheduled job
  re-verifies every quote against the live spec text so the registry cannot
  silently drift.
- **Determinism by canonicalization.** All JSON is canonicalized (RFC 8785)
  before any comparison, so a report is byte-identical across platforms and runs.
  Correctness here is a *fixpoint* property, tested as one — a real bug hid until
  a fuzzer first generated `-0.0`.
- **Not-applicable is not pass.** A requirement gated on a capability that was
  never negotiated is reported as not-applicable, never as a vacuous pass.
  Inflating a pass rate with checks for features the server never advertised is
  how a conformance tool loses credibility.
- **Calibration against the authority.** The agreement check (see the
  [Introduction](introduction.md)) diffs our verdicts against the official
  runner's on every CI run; an unexplained divergence fails the build, and a
  stale recorded divergence fails it too.
- **Falsifiability.** Every check is killed by at least one committed
  injected-violation trace; a check with no killer trace is unfalsifiable. The
  [corpus](corpus.md) is both the regression suite and the evidence each check
  does what it claims.
- **Verdicts are a contract.** A change that makes a previously-passing trace
  fail is a breaking change even when no function signature moved; it is named in
  the changelog and the version is bumped accordingly. API-signature compatibility
  is *additionally* checked mechanically against the published crates.io baseline.
