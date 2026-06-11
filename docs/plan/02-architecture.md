<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Architecture

**Status:** Active
**Last reviewed:** 2026-06-11

---

## Workspace layout

```
mcp-conformance/
├── Cargo.toml                  # workspace root: shared metadata, lints, profiles
├── crates/
│   ├── mcp-conformance-core/   # spec-as-data: requirement registry, traceability, trace schema
│   ├── mcp-trace-validator/    # trace replay + validation engine; CLI binary
│   ├── mcp-everything-server/  # reference server exercising every capability (on rmcp)
│   └── mcp-reference-host/     # reference host / agent loop (on rmcp)
├── xtask/                      # cargo xtask: orchestration of official-suite runs (publish = false)
├── corpus/                     # recorded trace corpora + golden reports (test fixtures, not a crate)
├── fuzz/                       # cargo-fuzz targets
└── docs/                       # this plan, ADRs, later an mdBook
```

Naming was decided against verified crates.io availability in
[ADR-0003](decisions/0003-crate-naming.md). The bare name `mcp-conformance` is deliberately
**not** published — the official Rust SDK uses it for an internal package
([register 3.3](01-ecosystem-context.md)).

## Crate responsibilities and dependency rules

| Crate | May depend on | Must never depend on | I/O |
|-------|---------------|----------------------|-----|
| `mcp-conformance-core` | `serde`, `serde_json` (+ `schemars` if schemas are emitted) | rmcp, tokio, any transport or HTTP crate | None. Pure data + pure functions. |
| `mcp-trace-validator` | `mcp-conformance-core` | rmcp | File/stdin reading in the CLI layer only; the engine is `&[TraceEvent] -> Report`, no I/O. |
| `mcp-everything-server` | rmcp, tokio, `mcp-conformance-core` (for self-description) | `mcp-trace-validator` | stdio + streamable HTTP server. |
| `mcp-reference-host` | rmcp, tokio | `mcp-trace-validator` | stdio + streamable HTTP client. |
| `xtask` | anything (dev-only, unpublished) | — | Spawns SUTs and the official runner. |

The arrows only point one way: **core ← validator**, and **core ← {server, host}** for
self-description. The validator never links rmcp — that is what keeps its verdicts
independent of the SDK it may be asked to judge. The server and host never link the
validator — they are subjects, not judges.

## `mcp-conformance-core` — the spec as data

The crate that makes everything else mechanical. Three data models, no I/O:

### Requirement registry

Every normative clause of a spec revision, extracted into a record:

```text
Requirement {
    id:          "LIFE-001"            // stable, never reused; area prefix + ordinal
    level:       Must | MustNot | Should | ShouldNot | May
    actor:       Server | Client | Both
    source:      { revision, section anchor, verbatim quote }
    applies:     revision range (introduced .. removed)   // survives the 2026-07-28 rework;
                 // deferred until a second revision lands (ADR-0006) — one-revision data has nothing to range over
    capability:  optional capability key gating the requirement
    checks:      [check ids]  |  exclusion: documented reason
}
```

The `checks | exclusion` alternative is deliberately the same shape as SEP-2484's
`sep-NNNN.yaml` traceability files ("mapping each MUST/MUST NOT to a check or a documented
exclusion" — [register 2.9](01-ecosystem-context.md)), so registry entries and SEP
traceability are one format, not two. The `2025-11-25` inventory covers the core protocol surface — the README's generated
coverage table is the authoritative count; this document fixes the shape, not the
contents.

### Trace schema

A trace is an ordered sequence of events, serialized as JSON Lines:

```text
TraceEvent {
    seq:        u64                    // total order within the trace
    direction:  ClientToServer | ServerToClient
    transport:  Stdio | StreamableHttp
    kind:       Message(JSON-RPC) | Http { status, headers subset } | Lifecycle(open/close/abort)
    payload:    canonicalized JSON
}
```

Transport-level events are first-class because real requirements live there: `Host`-header
validation (CVE-2026-42559 class), session headers, SSE resumption. A message-only trace
format could not express them.

### Capability matrix

A pure function from the negotiated capability sets to the active requirement subset.
Requirements gated on undeclared capabilities are reported as *not-applicable*, never as
*passed* — inflating pass rates with vacuous checks is how conformance tools lose trust.

## `mcp-trace-validator` — deterministic judgment

The engine replays a trace through a **typed session state machine** per spec revision and
evaluates every active requirement. Design commitments:

1. **Determinism.** Same trace, same registry version → byte-identical report. All JSON is
   canonicalized before comparison — object-key ordering per RFC 8785 (UTF-16 code-unit
   order, implemented and edge-tested) and number serialization in the ECMAScript form
   RFC 8785 §3.2.2.3 requires, validated against the RFC's own Appendix B vectors;
   parsing relies on serde_json's `float_roundtrip` feature (without it, float parsing
   may be 1 ULP off its own output — caught by the canonical fixpoint property tests).
   Map iteration order never leaks; no clocks, no randomness in the engine.
2. **Explicit state machines.** `2025-11-25`: `Connecting → Initializing → Initialized →
   Ready → Closing` with error edges. The `2026-07-28` rework removes the handshake states —
   modeled as a second state-machine variant behind a feature gate (below), not as a fork.
3. **Requirement-addressed findings.** Every finding carries a requirement ID, the spec
   quote, the offending event `seq`, and the expected-vs-actual detail. A report a maintainer
   cannot act on is noise.
4. **Reports as artifacts.** Output formats: human (terminal), JSON (machine), JUnit XML
   (CI). Exit codes: `0` pass, `1` findings, `2` invalid invocation, `3` malformed trace —
   the a2a-rust TCK convention, extended.
5. **No network.** The validator never dials anything. Capturing traces is the job of the
   host, the server's tap, or any external proxy; validating them is the validator's.

## `mcp-everything-server` — the Rust reference server

The SEP-1730 appendix artifact ([register 2.6](01-ecosystem-context.md)): a server
exercising **every** protocol capability, mirroring the TypeScript everything server's
coverage ([register 2.10](01-ecosystem-context.md)) — tools (echo, structured output,
resource links, sampling triggers, elicitation triggers including URL mode, long-running
operations with progress), resources (static, templated, subscriptions, update
notifications), prompts (simple, parameterized, embedded resources), logging level toggles,
completions, and pagination — over stdio and streamable HTTP.

Design commitments:

- **Tier-1 bar:** 100% pass on the official suite's server scenarios for the current
  revision, enforced in CI ([06-roadmap.md](06-roadmap.md) M2).
- **Secure by default:** streamable HTTP binds loopback, `Host`/`Origin` validation on, with
  explicit opt-outs — see [05-security-model.md](05-security-model.md).
- **Self-describing:** the server can emit its own capability-coverage manifest (from
  `mcp-conformance-core` types) so the gap between "what it claims" and "what the suite
  exercised" is itself machine-checkable.
- **Upstream-shaped:** structured so the server (or its scenario fixtures) can be offered to
  `modelcontextprotocol/rust-sdk` with minimal rework — rmcp idioms, no exotic dependencies,
  Apache-2.0-compatible licensing posture ([ADR-0003](decisions/0003-crate-naming.md)).

## `mcp-reference-host` — the client side of the proof

A native Rust MCP host on rmcp (not a CLI wrapper): connects to N servers, negotiates
capabilities, and drives a bounded tool-use loop. It exists because client conformance is
half the suite ([register 2.2](01-ecosystem-context.md)) and because a toolkit that never
*consumes* its own everything server proves nothing.

Design commitments:

- **Typed session lifecycle** shared conceptually with the validator's state machines.
- **Bounded agent loop:** explicit turn limit, deterministic stop conditions, cooperative
  cancellation tokens, bounded concurrent tool calls with backpressure.
- **Transport posture:** stdio and streamable HTTP; retries with jittered exponential
  backoff honoring `Retry-After`; resumption via SSE event-id cursors where the spec allows.
- **Sampling/elicitation/roots handlers** implemented for real (scriptable for CI), since the
  official client scenarios exercise them.
- **No model-provider coupling.** The loop's "LLM" is a trait; CI uses a scripted
  implementation. Zero API credits in CI ([04-engineering-standards.md](04-engineering-standards.md)).

## `xtask` — orchestration and the agreement check

`cargo xtask conformance` is the one command that ties the room together:

1. Build and start `mcp-everything-server`.
2. Run the **pinned** official runner (`npx @modelcontextprotocol/conformance@<pinned>
   server --url …`) against it; collect its verdicts.
3. Capture the same session as a trace; run `mcp-trace-validator` on it.
4. **Diff the two verdicts.** Disagreement fails CI and is triaged as either our bug, an
   official-suite bug (filed upstream), or a spec ambiguity (filed upstream).

The agreement check is the toolkit's credibility mechanism: the validator is continuously
calibrated against the authority rather than asking anyone to trust it.

## Protocol-revision strategy

- `2025-11-25` is the default revision everywhere.
- `2026-07-28` support lands behind a `draft-2026-07-28` cargo feature while the RC is in
  flux; registry entries gain `applies` ranges at roadmap M2.5 so the stateless rework is a
  data change plus a state-machine variant, not a rewrite
  ([register 1.2–1.5b](01-ecosystem-context.md)). The feature gate drops (becomes default)
  only after the final spec text ships, M2.5 completes, and the official suite's scenarios
  for it stabilize.
- Versioning of our own crates follows SemVer with `#[non_exhaustive]` on protocol-facing
  enums and structs; pre-1.0 minor bumps may break, mirroring the honesty of the spec's own
  RC process.

## Hard problems, named early

| Problem | Position |
|---------|----------|
| Stateless rework lands differently than the RC | Feature gate + `applies` ranges localize the blast radius to registry data and one state-machine variant. Re-scoped at the roadmap's RC-reconciliation gate, not discovered in a rewrite. |
| Trace capture fidelity (interleaving, partial writes, SSE framing) | Capture at message boundaries with transport events recorded by the component that owns the socket; `seq` assigned at capture, never inferred later. |
| Verdict divergence from the official suite | The agreement check makes divergence a CI failure with a triage path, not a slow credibility leak. |
| Concurrent tool calls and cancellation in the host | Cooperative cancellation tokens; bounded in-flight set; every stop condition enumerated and tested — no "the loop usually terminates". |
| Keeping the registry honest as the spec evolves | SEP-2484's own traceability format is our storage format, so upstream review of our scenarios doubles as review of our registry. |

## Module discipline

The a2a-rust rules apply verbatim ([04-engineering-standards.md](04-engineering-standards.md)):
≤ 500 lines per file, thin `mod.rs`, single responsibility per module,
`#![forbid(unsafe_code)]` at every crate root, no panics on untrusted input — a malformed
trace is a `3` exit, never a backtrace.
