<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0002: Product Scope — Conformance Toolkit, Not Another SDK

**Date:** 2026-06-09
**Status:** Accepted
**Author:** Tom F.

---

## Context

A new Rust project in the MCP/agents space has many plausible shapes. The candidates were
evaluated against verified ecosystem facts (all anchors in the
[register](../01-ecosystem-context.md)) with one question: *where does a small, excellent,
independent project add durable value rather than duplicate, fragment, or build on sand?*

The decisive facts:

- Conformance is structurally load-bearing in MCP: SEP-1730 (Final) gates SDK tier standing
  on conformance pass rates, and SEP-2484 gates spec finalization itself on conformance
  scenarios (register 2.5, 2.9).
- The Rust SDK is officially Tier 2 with verified, specific gaps: no everything server
  (SEP-1730's appendix artifact), no MSRV, no RustSec advisory for its CVSS 8.8 CVE
  (register 2.8, 3.4, 3.5, 4.3).
- The offline half of conformance does not exist in any language: the official suite
  executes live scenarios from TypeScript; nothing validates recorded traces against the
  spec's normative requirements (register §2, §5).
- The 2026-07-28 revision restructures the protocol's lifecycle (register 1.3) — every
  implementation needs test targets on a deadline.
- Adjacent shapes are already owned or structurally doomed: official API SDKs are generated
  in-house by Anthropic post-Stainless with no Rust on the roadmap a community client could
  survive (register 6.1–6.2); Agent-SDK "ports" are subprocess wrappers around a vendor
  product (6.3); the Rust gateway/proxy space belongs to a Linux Foundation project with
  3,000+ stars (6.4); security scanning is a crowded, distinct product (§5).

## Decision

Build the Rust-native MCP conformance toolkit — four coupled artifacts, one workspace:
requirement registry (`mcp-conformance-core`), trace validator (`mcp-trace-validator`),
everything server (`mcp-everything-server`), reference host (`mcp-reference-host`) — operated
upstream-first toward the official Rust SDK and conformance suite.

Bind the scope with non-goals enforceable in review: not an SDK, not a Messages-API client,
not an Agent-SDK port, not a gateway, not a scanner, not a hosted service
([charter](../00-charter.md)). Any feature that would make a third party depend on this
project *instead of* rmcp triggers an automatic ADR and charter review
([risk R7](../08-risk-register.md)).

## Consequences

### Positive

- Durability: every spec revision increases the toolkit's value; no vendor release can moot
  it, because validating vendor releases is what it does.
- Leverage: the same artifacts serve our roadmap and rmcp's published Tier-1 path — effort
  compounds instead of competing (register 2.8).
- Credibility mechanics built in: the agreement check calibrates us against the official
  suite continuously ([03-conformance-strategy.md](../03-conformance-strategy.md)).
- A small, completable surface for a solo maintainer, with the hard problems (state
  machines, determinism, capability gating) being depth, not sprawl.

### Negative

- Dependence on upstream goodwill for the highest-value outcomes (everything-server
  adoption); mitigated but not eliminated by the backlog's small-contributions-first
  ordering ([risk R9](../08-risk-register.md)).
- Conformance tooling is infrastructure: adoption is slow, unglamorous, and measured in
  single-digit integrations for a long time.
- Tied to MCP's trajectory; a protocol-level upheaval larger than the 2026-07-28 rework
  would demand significant rework (accepted — the register and feature-gating strategy are
  the shock absorbers).

## Alternatives Considered

### Rust Messages-API client

Rejected: Anthropic owns industrialized SDK generation in-house (register 6.2) and lists
seven official languages with Rust absent but trivially addable (6.1); the community niche
is occupied; obsolescence risk is total and permanent.

### Native Claude Agent SDK port

Rejected: the Agent SDK is a fast-moving vendor product in Python/TypeScript (register 6.3);
existing Rust ports are CLI subprocess wrappers; a true native port chases a proprietary
product's internals without its team.

### Higher-level MCP server framework

Rejected: rmcp plus established community SDKs (register 5.7) already serve this;
a framework fragments the very ecosystem a conformance tool needs to serve neutrally.

### MCP gateway/proxy/router

Rejected: agentgateway is a Linux Foundation project in Rust with three orders of magnitude
more adoption than anything we could bootstrap (register 6.4); contributing there is the
rational move in that space, and it is not this project.

### MCP security scanner / tool-poisoning detection

Rejected as product, retained as posture: scanning intent is owned by established tools;
*protocol-level* security requirements (the CVE-2026-42559 class) belong in our registry and
defaults ([05-security-model.md](../05-security-model.md)) — conformance pressure, not
auditing.

### A2A↔MCP interop bridge

Rejected: technically interesting, strategically orphaned — neither ecosystem's governance
is asking for it, and it inherits two specs' churn for one niche's payoff.
