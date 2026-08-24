<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Charter

**Status:** Active
**Last reviewed:** 2026-08-24

---

## Mission

Build the Rust-native conformance toolkit for the Model Context Protocol: a machine-readable
requirement registry, a transport-agnostic protocol trace validator, a Rust "everything
server" reference implementation, and a reference host runtime — engineered to a standard the
official ecosystem can adopt, and operated upstream-first.

## Thesis

Conformance is the load-bearing mechanism of MCP's maturity model, and the Rust side of it is
underbuilt. Every clause below is verified in the
[ecosystem context register](01-ecosystem-context.md):

1. **Conformance gates SDK standing.** SEP-1730 (Final) classifies SDKs into tiers; Tier 1
   requires a 100% conformance pass rate, two-business-day issue triage, and seven-day
   resolution of critical (P0) bugs. On protocol-feature timing the two live sources
   disagree, and the operational one governs: the sdk-tiers requirement table reads "Before
   new spec version release, timeline agreed per release based on feature complexity", while
   the Final SEP text still says "two week window between Release Candidate and the new
   protocol version release" ([register 2.5](01-ecosystem-context.md)). The practiced
   `2026-07-28` cycle ran about ten weeks.
2. **Conformance gates the spec itself.** SEP-2484 (Final, Process) requires a matching
   conformance scenario and a `sep-NNNN.yaml` traceability file before a Standards-Track SEP
   can reach Final status — mapping every MUST, MUST NOT, SHOULD and SHOULD NOT (and the RFC
   2119 synonyms) to "a check or a documented exclusion"
   ([register 2.9](01-ecosystem-context.md)).
3. **The Rust SDK reached Tier 1, and one published gap survives: it has no everything
   server.** The tier table at `/docs/sdk` now places Rust in Tier 1 alongside TypeScript,
   Python, C# and Go, and rust-sdk's own `ROADMAP.md` reports every SEP-1730 Tier 1
   requirement met. SEP-1730's appendix asks each SDK to carry an everything server in-repo
   ("We want to check it into each SDKs repo as it will serve as an example for server
   implementers"); rmcp still has none, and its own client examples drive the *TypeScript*
   everything server over `npx`. *This premise read "sits at Tier 2… three verified gaps" until
   the 2026-08-24 sweep refuted three of them — the tier, the missing MSRV (declared 1.88
   since rmcp `3.0.0-beta.2`) and the missing RustSec advisory (`RUSTSEC-2026-0189` now
   exists). See [register 2.8, 3.4, 3.5, 4.3](01-ecosystem-context.md) and
   [ADR-0015](decisions/0015-the-tier-2-premise-is-gone.md).*
4. **The protocol-revision storm arrived.** The `2026-07-28` revision shipped on schedule and
   is the current version: no `initialize` handshake, no `Mcp-Session-Id`, per-request
   version negotiation in `_meta` ([register 1.1, 1.3](01-ecosystem-context.md)). Every
   implementation and every conformance tool is absorbing it now, not preparing to.
5. **Nobody has built the offline half.** The official suite executes live scenarios from
   TypeScript, and its answer to "what did this revision require" is a frozen per-revision set
   of those same live scenarios ([register 2.18](01-ecosystem-context.md)). No tool in any
   language validates *recorded traces* against the spec's normative requirements, and no Rust
   everything server exists. The authority has said so itself: SEP-2484 supersedes SEP-1627
   with "SEP-1627's golden-trace approach was not carried forward… SEP-1627's protocol-debugger
   ideas remain valuable future work" ([register 2.12](01-ecosystem-context.md)). Adjacent
   community tools remain low-adoption and none occupies either gap — three of the six have
   published nothing for four months or more ([register §5](01-ecosystem-context.md)).

The opportunity is therefore durable rather than speculative: conformance tooling grows in
value with every spec revision, and cannot be obsoleted by any vendor shipping a new SDK (it
is how new SDKs get validated). **Premise 5 is the load-bearing one.** Premise 3 used to carry
equal weight — a Tier-2 SDK with four nameable gaps — and after the 2026-08-24 sweep it
carries one gap instead of four. The scope decision does not rest on it; the reasoning is
worked through in [ADR-0015](decisions/0015-the-tier-2-premise-is-gone.md), including what
gets weaker as a result. Independently, SDK "conformance with the specification" is now a
named priority area on the roadmap published 2026-08-22
([register 1.9](01-ecosystem-context.md)).

## What we ship

Four coupled artifacts in one Cargo workspace (boundaries in
[02-architecture.md](02-architecture.md)):

| Artifact | Crate | One-line definition |
|----------|-------|---------------------|
| Requirement registry | `mcp-conformance-core` | The MCP spec's normative clauses as data: stable IDs, RFC 2119 levels, source quotes, applicability, per-revision validity — plus the SEP-2484 traceability format. |
| Trace validator | `mcp-trace-validator` | Library + CLI that replays a recorded protocol trace through a typed session state machine and reports pass/fail per requirement, deterministically, for any implementation in any language. |
| Everything server | `mcp-everything-server` | A Rust server on rmcp exercising every protocol capability; built to pass the official suite's server scenarios at the Tier-1 bar and offered upstream. |
| Reference host | `mcp-reference-host` | A native Rust MCP host/agent-loop on rmcp — the client-side system-under-test that proves the toolkit from the other side of the wire, with secure-by-default transport posture. |

The official TypeScript suite remains the authority on what "conformant" means. This project
extends its reach: to Rust reference implementations, to offline/CI trace analysis without a
Node toolchain, and to requirement-level traceability.

## Goals

| # | Goal | Measured by |
|---|------|-------------|
| G1 | A Rust everything server passing the official suite's server scenarios at 100% on the current spec revision | CI job wired to the pinned official runner; results published in-repo |
| G2 | A trace validator any SDK can embed in CI | Validator verdicts agree with the official runner on shared scenarios; at least one external project adopts it |
| G3 | A reference host demonstrating client-side conformance over stdio and streamable HTTP | Official client scenarios pass; host drives a complete tool-use loop against the everything server |
| G4 | Measurable upstream contributions to `modelcontextprotocol/rust-sdk` and the conformance repo | Merged PRs; RustSec advisory filed for CVE-2026-42559 in coordination with maintainers; MSRV proposal; conformance scenarios |
| G5 | Engineering quality at or above the bar set by [a2a-rust](https://github.com/tomtom215/a2a-rust) | Every gate in [04-engineering-standards.md](04-engineering-standards.md) green on every commit to `main` |

## Non-goals

Explicitly out of scope, with the reasoning preserved in
[ADR-0002](decisions/0002-product-scope.md):

- **Not an MCP SDK.** rmcp is the official SDK; this project builds *on* it and contributes
  *to* it. Anything generically useful to the SDK is offered upstream first.
- **Not a Messages-API client.** Anthropic generates its official SDKs in-house; a community
  Rust API client is a high-obsolescence dead end and several already exist.
- **Not a Claude Agent SDK port.** The Agent SDK is a vendor product in Python and TypeScript;
  existing Rust "ports" are CLI subprocess wrappers.
- **Not a gateway/proxy.** agentgateway (Linux Foundation) owns that space in Rust.
- **Not a security scanner.** Tool-poisoning and server-auditing scanners (mcp-scan, agentox)
  are a different product; our security surface is protocol conformance and secure defaults.
- **Not a hosted service.** Everything runs locally or in CI.

## Success criteria

The project is succeeding when, and only when:

1. Every roadmap milestone closes against its definition of done
   ([06-roadmap.md](06-roadmap.md)) with all engineering gates green.
2. The everything server or its tests are referenced, reviewed, or adopted by the official
   Rust SDK — or a documented decision records why upstream declined and what changed.
3. At least one implementation other than ours runs the trace validator or the everything
   server in its CI.
4. A published tier-gap report for rmcp is concrete enough that closing it is a checklist,
   not a research project.

Adoption signals are the test that the work is genuinely useful rather than performative; a
toolkit nobody runs is a portfolio piece, not infrastructure.

## Operating principles

1. **Upstream-first.** The default home for generically useful work is the official repo;
   this repo holds what does not fit there ([07-ecosystem-engagement.md](07-ecosystem-engagement.md)).
2. **The spec is the source of truth.** Where the official suite and the spec text disagree,
   we file the discrepancy upstream rather than silently choosing a side.
3. **Verified facts only.** Claims about the ecosystem cite the
   [register](01-ecosystem-context.md); claims about ourselves cite CI.
4. **Secure by default.** The CVE-2026-42559 class (DNS rebinding via unvalidated `Host`
   headers) is designed out, not patched in ([05-security-model.md](05-security-model.md)).
5. **Quality is not negotiable.** The a2a-rust bar is the floor, on every file, in every
   commit ([04-engineering-standards.md](04-engineering-standards.md)).
