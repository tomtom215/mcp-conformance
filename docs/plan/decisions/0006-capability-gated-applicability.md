<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0006: Capability-Gated Applicability and the `not-applicable` Outcome

**Date:** 2026-06-09
**Status:** Accepted
**Author:** Tom F.

---

## Context

Most feature-area requirements in `2025-11-25` bind a party only when the corresponding
capability was negotiated: tools clauses bind servers that declared `tools`, subscription
clauses bind servers that declared `resources.subscribe`, roots clauses bind clients that
declared `roots`. The architecture fixed the intent early
([02-architecture.md](../02-architecture.md) §Capability matrix): requirements gated on
undeclared capabilities are reported as *not-applicable*, never as *passed* — vacuous
passes inflate scores and lose trust. What was not yet fixed is the registry encoding,
the resolution semantics against a recorded trace, and the reporting shape.

## Decision

### Registry encoding

A requirement may carry an optional `capability` member: a dotted path whose first
segment names the declaring party (`server` or `client`) and whose remaining segments
index into that party's declared capability object from the `initialize` exchange —
`server.tools`, `server.resources.subscribe`, `client.roots`. The path is a validated
type (`CapabilityGate` in `mcp-conformance-core`); malformed gates are registry-load
errors, not runtime surprises.

### Resolution semantics

Capabilities are read from the trace's `initialize` exchange: client capabilities from
the request's `params.capabilities`, server capabilities from the result's
`capabilities`. A gate is **declared** when every path segment resolves and the final
value is neither `false` nor `null` — so `"tools": {}`, `"tools": {"listChanged": true}`,
and `"subscribe": true` all declare, while an absent key or `"subscribe": false` does
not. When the trace contains no surface to read (no `initialize` request params for a
`client.` gate, no `initialize` result for a `server.` gate), the gate is undeclared:
the conservative reading, because nothing in the trace evidences the capability was
negotiated.

### Engine and report

A new outcome `not-applicable` joins the report model, first-class in totals like
`excluded` and `unsupported`. Precedence in the engine: a documented exclusion wins
(static registry fact), then a registry/build check mismatch (`unsupported` must stay
deterministic for a given registry and build, regardless of trace content), then the
capability gate, then the checks run. A `not-applicable` row carries the gate path that
failed to resolve, so the report says *why* the requirement was skipped. JUnit maps it
to `<skipped>`; the verdict and exit codes ignore it, exactly as they ignore
`excluded`.

### Deferred: `applies` revision ranges

The architecture's requirement shape also names an `applies` revision range. With one
revision in the registry there is nothing for a range to discriminate; it stays
unimplemented until `2026-07-28` entries land (roadmap M5), and is out of scope here.

## Consequences

### Positive

- Feature areas (tools, resources, prompts, logging, completion) can enter the registry
  without making every minimal trace fail — and without vacuously passing, either.
- The report distinguishes "this trace never negotiated the capability" from "the rule
  held", which is the distinction SEP-1730 tier evidence needs.
- Gates are data, so the coverage report can count gated requirements per area.

### Negative

- Violation corpus traces for gated checks must declare the capability or the check
  never fires; the corpus falsifiability test enforces this, which makes authoring
  violation traces slightly more ceremonial.
- A trace from a session that *used* an undeclared capability reports the feature-area
  requirements as not-applicable rather than judging them. That is deliberate: using an
  undeclared capability is its own violation (a lifecycle-area check), and judging
  feature clauses against a never-negotiated feature would assert more than the trace
  evidences.

## Alternatives Considered

### Run gated checks anyway and report findings as warnings

Rejected: it blurs the line the spec draws. A server that never declared `tools` is not
"warned" about tool-result shapes; those clauses simply do not bind it.

### Treat missing initialize exchange as "all capabilities declared"

Rejected: vacuous-pass by another door. A truncated or malformed trace would suddenly
activate every feature-area requirement against messages that may belong to a different
negotiation outcome.

### Encode gates as structured JSON (`{"party": "server", "path": ["tools"]}`)

Rejected for ergonomics: the dotted string is how maintainers talk about capabilities,
diffs one line, and validates just as strictly behind a newtype.
