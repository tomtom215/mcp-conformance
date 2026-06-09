<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Charter

**Status:** Active
**Last reviewed:** 2026-06-09

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
   requires a 100% conformance pass rate, protocol features implemented inside the two-week
   window between release candidate and release, two-business-day issue triage, and seven-day
   resolution of critical (P0) bugs.
2. **Conformance gates the spec itself.** SEP-2484 (merged 2026-05-17) requires a matching
   conformance scenario and a requirement-traceability file before a Standards-Track SEP can
   reach Final status.
3. **The Rust SDK sits at Tier 2** in the officially published tier table (Tier 1: TypeScript,
   Python, C#, Go). The SDK already wires into the official conformance runner, but three
   verified gaps remain: no everything-server reference implementation (SEP-1730's appendix
   asks every SDK to carry one in-repo), no declared MSRV, and no RustSec advisory for
   CVE-2026-42559 — meaning `cargo audit` is silent on a CVSS 8.8 vulnerability in rmcp
   < 1.4.0.
4. **A protocol-revision storm is scheduled.** The 2026-07-28 release candidate removes the
   `initialize` handshake and the `Mcp-Session-Id` header — a structural rework that every
   implementation, and every conformance tool, must absorb on Tier-1 timelines.
5. **Nobody has built the offline half.** The official suite executes live scenarios from
   TypeScript. No tool in any language validates *recorded traces* of MCP traffic against the
   spec's normative requirements, and no Rust everything server exists. Adjacent community
   tools (tooltest, mcp-tester, mcp-wallfacer, mcp-probe) are low-adoption and none occupies
   either gap.

The opportunity is therefore durable rather than speculative: conformance tooling grows in
value with every spec revision, cannot be obsoleted by any vendor shipping a new SDK (it is
how new SDKs get validated), and directly serves the official Rust SDK's published path from
Tier 2 to Tier 1.

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
