<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0016: No Reservation Primitive — the Trace Is One Transport and the Spec Went Stateless

**Date:** 2026-08-27
**Status:** Rejected (the investigation is the record; no reservation primitive is proposed
upstream and no actuation profile is added here). **Third negative consequence refuted the same
day by [ADR-0017](0017-both-stranded-properties-were-already-settled.md)** — see the amendment
below; the body is unedited, per this log's rule that old reasoning stays readable.
**Author:** Tom F.

---

## Context

Anthropic announced the Model Hardware Standard (MHS) on 2026-08-27 — a driver abstraction
for agents operating physical devices, with `read`/`write` primitives, natural-language tags
that generate a device reference file, and three control surfaces: "MCP, the command line
interface, and code files (APIs)". There is no public specification: `modelhardwarestandard.com`
is a single-page application behind a waitlist form, every path on it returns the same
10,430-byte shell, and no schema, reference implementation or conformance suite is published.
It is therefore not an anchor this register can cite, and nothing below depends on it.

What MHS did prompt is a real question about *this* project's reach. If agents drive hardware
over MCP, does the toolkit gain a physical-device profile — clauses for
confirmation-before-actuation, declared-limit enforcement, exclusive reservation of a stateful
device, write idempotency under retry, bounded error recovery, e-stop reachability? And does
the missing piece underneath them — MCP has no lease or reservation primitive for a resource
that only one caller may hold — belong upstream as a SEP, under the
[engagement policy](../07-ecosystem-engagement.md)'s upstream-first rule?

The concrete shape investigated was a lease: an acquire call returning a server-minted token
with a TTL, explicit release, server-side expiry, renewal, every write to the held resource
carrying the token, and a conflict error naming a retry-after. Its conformance clauses would
have been the obvious ones — a write without a valid lease is refused, a lease expires, two
concurrent holders are never granted.

Both halves fail, for reasons this repository had already written down.

**The spec removed statefulness one revision ago, deliberately.** Register
[1.3](../01-ecosystem-context.md) records that `2026-07-28` removes the `initialize`/`initialized`
handshake and the `Mcp-Session-Id` header; [1.5a](../01-ecosystem-context.md) records that
protocol sessions are gone and that cross-call state is carried by **server-minted handles
passed as ordinary tool arguments (SEP-2567)** — shipped, with its SDK implementation and
tracking epic closed (register row at 131). The registry encodes the consequences as normative
text: `BASE-063` makes reliance on prior requests a server **MUST NOT**, and `BASE-065` makes
refusing an operation *because it arrived on a different connection* a **SHOULD NOT** — which
is precisely the connection affinity a naive lease would need. A reservation primitive asks the
protocol to re-admit what it just finished removing, in the revision the roadmap is built on,
when the sanctioned mechanism for exactly this — a handle the server mints and the client hands
back — already shipped.

**The trace vocabulary cannot judge the clauses, and this is a scope boundary rather than a
gap.** Under the check-or-exclusion rule (`requirement.rs:9` — every clause carries a check *or*
"a documented exclusion explaining why the clause cannot be judged from a recorded trace"),
140 of 259 requirements are already excluded. The proposed profile lands almost entirely in that
set, and three exclusions were written before this investigation started:

| Proposed clause | Verdict | Already-recorded reason |
|---|---|---|
| Confirmation before actuation | Excluded | `TOOL-017`, `TOOL-018`, `TOOL-044`: "Binds the application's user interface — what it shows, when it asks, whether a person is there. **None of it is a message**, and MCP deliberately does not mandate an interaction model" |
| Exclusive reservation | Excluded | `BASE-065`: "two correlated connections in one recording, which the trace vocabulary does not span: **each recorded session is one transport**" |
| Declared-limit enforcement | Half-observable at best | The declaration is on the wire; enforcement is inside the device. A driver that silently clamps, or lies about its state, is indistinguishable from one that held |
| E-stop reachability | Not observable | A physical property of the installation, with no wire footprint |
| Write idempotency under retry, bounded error recovery | Observable | Generic distributed-systems properties. Real, and not about hardware |

The last row is the whole surviving yield, and it does not need a hardware framing to exist.

Two claims made while scoping this deserve to be recorded as refuted, because both were
load-bearing and both were wrong. The first: that exclusive reservation is the message-observable
member of the set — `BASE-065`'s exclusion says it is not, for a structural reason about the trace
format rather than a missing feature. The second: that a regulatory deadline made an actuation
profile urgent. Regulation (EU) 2023/1230 applies from **14 January 2027** (Article 54; a widely
repeated secondary report gives 20 January and is wrong), and its third-party conformity route
reaches Annex I Part A item 5 — "Safety components with fully or partially self-evolving behaviour
**using machine learning approaches** ensuring safety functions". Recital 127 forecloses the rest
in terms: those provisions "should **not** apply to software incapable of learning or evolving,
and programmed only to execute certain automated functions." A static limits file is out of scope
by the regulation's own words; the regulation binds manufacturers placing products on the market,
not a lab writing a driver for its own rig; and functional safety already has an evidence regime
(ISO 13849-1, IEC 62061, IEC 61508) into which a JUnit file from a trace validator does not fit.
The EU AI Act's Annex I obligations, the other candidate deadline, moved to 2028.

Not verified from this session, and deliberately not claimed: SEP-2567's and SEP-2575's own text
were not read first-hand — the account above rests on register rows 1.3 and 1.5a and on the
registry's encoding of the resulting clauses.

## Decision

1. **No reservation or lease primitive is proposed to MCP.** The protocol removed sessions by
   design in the current revision and ships server-minted handles for cross-call state; a lease
   is an application-level concern expressed through that mechanism, not a protocol gap. Filing
   it would be the sixth instance of the pattern
   [07-ecosystem-engagement.md](../07-ecosystem-engagement.md) already names — a gaps-based offer
   overtaken upstream — and the first where the gap was closed *before* it was proposed rather
   than after.

2. **No physical-device or actuation conformance profile is added.** Its clauses are unjudgeable
   from a recorded trace under the rule this project already enforces, and three of them are
   excluded in the shipped registry. Adding them would mean either weakening the
   check-or-exclusion rule or shipping a profile that is mostly exclusions — a coverage claim
   with nothing behind it, which is the failure mode [ADR-0006](0006-capability-gated-applicability.md)
   and [ADR-0012](0012-not-observed-outcome.md) exist to prevent.

3. **The single-transport property of the trace vocabulary is promoted from a per-clause
   exclusion to a stated scope boundary.** "Each recorded session is one transport" currently
   lives inside `BASE-065`'s exclusion string, where it reads as an incidental limitation. It is
   not: any requirement whose falsification needs two correlated transports is out of reach by
   construction, and that is a property of the format, not of any one clause. It belongs in
   [02-architecture.md](../02-architecture.md) beside the capability matrix, stated once, so the
   next person meets it before designing against it rather than after.

4. **Neither MHS nor LAP is added to the register as an anchor.** MHS publishes no
   specification. LAP (arXiv 2606.03755, v1 only, 2 June 2026) is authored from Shiyanjia Lab, a
   commercial research-services vendor rather than a research institution or standards body, and
   self-describes as having "no normative or implemented status yet". Neither supplies a fact a
   backlog item could be anchored to, and a row that cannot decay is not a register row.

## Consequences

### Positive

- The check-or-exclusion rule holds without an exception carved for a fashionable domain. The
  rule's value is that it is applied when the answer is unwelcome, and this is that occasion.
- Decision 3 converts a finding into a boundary someone can read. The single-transport property
  was discoverable only by grepping exclusion strings; the next person considering multi-party
  conformance now meets it in the architecture document.
- Upstream credibility is not spent on a proposal that contradicts the revision the ecosystem
  just shipped. Under the engagement policy, review bandwidth is the scarcest upstream resource,
  and the cheapest way to waste it is a confident proposal that a maintainer refutes by citing
  their own changelog.
- The MHS question is answered with evidence rather than deferred. When a specification is
  published, this record says what was already checked and what would have to change.

### Negative

- **The project declines a growth direction with no replacement.** Agents driving physical
  devices is a real and expanding class of MCP deployment, and this ADR's answer is that the
  toolkit has nothing to say about its safety-relevant properties. That is honest and it is
  still a smaller ambit than the one considered.
- **Decision 3 is the only shipped artifact.** Everything else is a refutation. An ADR whose
  net output is one paragraph moved into another document is a thin return on the investigation
  behind it, and it should be judged that way.
- **The two surviving observable properties are stranded.** Write idempotency under retry and
  bounded error recovery are judgeable and unclaimed, and this ADR does not pick them up — it
  only records that they do not need the hardware framing that motivated them. If they matter
  they need their own decision, and until then this record has named a gap it declined to fill.
- **The refutation rests on register rows, not on the SEP texts.** Rows 1.3 and 1.5a are
  well-anchored and recently verified, but a decision this categorical would be better supported
  by reading SEP-2567 and SEP-2575 directly. That was not done here and the conclusion should be
  revisited if either row moves.
- **Recording a rejection does not stop it being re-proposed.** "MCP needs a lease primitive" is
  an obvious-looking idea that will occur to the next reader as readily as it occurred here. This
  ADR is the only thing standing between that reader and the same three days of work, which is a
  weak mechanism and the reason decision 3 exists at all.

## Alternatives Considered

### Propose the lease primitive upstream anyway and let maintainers decide

Rejected. The engagement policy's first rule is to search upstream for prior or in-flight work
before building, and the search found not an absence but an opposite: the mechanism exists
(SEP-2567), and the direction the proposal argues against shipped as SEP-2575. "Let them decide"
is a reasonable posture toward a question upstream has not answered. This one is answered in the
changelog.

### Add the actuation profile with most clauses marked as exclusions

Rejected. It would technically satisfy the check-or-exclusion rule while defeating its purpose.
A profile whose rows are nearly all exclusions reports that the toolkit examined hardware safety
and found nothing to say — a sentence better written in an ADR than encoded as a coverage table
that readers will mistake for capability.

### Extend the trace vocabulary to correlated multi-transport recordings

Rejected for now, and this is the one that was close. It would genuinely unlock `BASE-065` and
the exclusivity clauses, and it is the only alternative that attacks the real obstacle rather
than working around it. But it changes the trace format's central assumption — one recording, one
transport — which every check, the golden corpus, and
[ADR-0013](0013-golden-report-format.md)'s pinning model are built on. That is a foundational
revision undertaken for a handful of clauses whose demand is currently hypothetical. Revisit if a
second, independent motivation appears; one speculative domain is not enough to move the
foundation.

### Build the actuation profile against MHS once its specification is published

Rejected as premature rather than wrong. It is the only version of this idea that could still
become correct, and it cannot be evaluated before there is a specification to read. Note the
sequencing trap for whenever that happens: the research preview is access-gated by application,
and preview terms plausibly carry confidentiality that would contaminate an independent
implementation. For MCP and A2A, reading the public specification was the qualification to build
independently; here, privileged access may be the disqualification. That should be checked in the
actual terms before anyone applies, not after.

## Amendment (2026-08-27): the two "stranded" properties were never stranded

The third negative consequence above says write idempotency under retry and bounded error
recovery "are judgeable and unclaimed". Both halves are wrong, and
[ADR-0017](0017-both-stranded-properties-were-already-settled.md) records the check that found it
— run the same day, before anything was built on the claim.

Idempotency under retry is **claimed**: retry at `2026-07-28` is MRTR (SEP-2322), and
`MRTR-015`, `MRTR-016`, `MRTR-018`, `MRTR-019`, `MRTR-020` and `TOOL-023` all carry implemented
checks. `MRTR-019` also answers the question in the opposite direction from the framing that
produced it — "The JSON-RPC `id` MUST be different between the initial request and the retry, as
they are independent requests" — so a retry is a new request rather than a redelivery, and there
is no duplicate-suppression semantics for a check to have an opinion about.

Bounded error recovery is **not judgeable**: it is `CACH-011` verbatim, already in the registry
and already excluded, because jitter and backoff are properties of a request schedule measurable
only in elapsed time (which checks may not consult, `LIFE-015`) and observable only across a run
far longer than one recorded session.

What survives is not a gap but a second boundary, and ADR-0017 states it: sixteen exclusions rest
on "checks may not consult time", and the rule they lean on appears in `02-architecture.md` only as
an engine property inside the Determinism commitment. Decision 3 of this ADR promoted one boundary
out of an exclusion string; ADR-0017 promotes its sibling for the same reason.
