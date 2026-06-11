<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0003: Crate Naming and Namespace Strategy

**Date:** 2026-06-09
**Status:** Accepted
**Author:** Tom F.

---

## Context

The repository name is `mcp-conformance` (decided at creation). Crate names must be
registered on crates.io, where names are first-come, global, and `-`/`_` equivalent. The
namespace was verified directly against the crates.io API on **2026-06-09** (404 =
available); earlier desk research had assumed availability of names that turned out to be
taken, which is why every name below was checked, not guessed:

| Name | Status 2026-06-09 | Notes |
|------|-------------------|-------|
| `mcp-conformance` | Available | **But**: the official rust-sdk's internal conformance package is already named `mcp-conformance` (`publish = false`) — [register 3.3](../01-ecosystem-context.md) |
| `mcp-conformance-core` | Available | |
| `mcp-trace-validator` | Available | |
| `mcp-everything-server` | Available | `mcp-everything` also free |
| `mcp-reference-host` | Available | `mcp-host-runtime`, `mcp-conformance-host` also free |
| `mcp-host` | **Taken** | Active crate, 33 releases in 5 months (seuros/mcphost-rs) — [register 5.8](../01-ecosystem-context.md) |
| `mcp-tck`, `mcp-test`, `mcp-testing`, `mcp-validator` | Available | Held as alternates |
| `praxis` | **Taken** | A React-agent framework since 2025-11 — eliminated a previously considered project name |
| `mcp-spec` | **Taken (official)** | Reserved 2025-02-27 by MCP maintainers for rust-sdk — [register 5.9](../01-ecosystem-context.md) |

Additional forces: the `-rs` suffix convention adds noise on a registry that is all Rust;
names in this space are consumed fast (register 5.8); and an ecosystem-serving project must
not collide with official naming, formal or informal.

## Decision

1. **Publishable crates:** `mcp-conformance-core`, `mcp-trace-validator`,
   `mcp-everything-server`, `mcp-reference-host`. The validator CLI ships as a binary target
   of `mcp-trace-validator` (binary name `mcp-trace-validator`); no separate CLI crate until
   a concrete need exists.
2. **The bare name `mcp-conformance` is not published by us.** The official rust-sdk uses it
   internally; publishing it out from under them would be hostile-adjacent even though the
   registry permits it. If coordination with upstream ever concludes they will never want
   it, this decision can be revisited by a superseding ADR — until then the name stays
   untouched and the repo name alone carries it.
3. **No `-rs` suffixes** (`mcp-everything-server`, not `mcp-everything-rs`).
4. **Registration timing:** at M0/M1 the four names are taken with minimal-but-real `0.1.0`
   releases (compiling, documented, honest README stating maturity) — crates.io norms reject
   empty squats, and the five-month lifecycle of `mcp-host` shows deferral is the riskier
   side ([risk R5](../08-risk-register.md)).
5. **Final availability is re-verified at publish time** via
   `https://crates.io/api/v1/crates/<name>` (expects 404); this ADR records a snapshot, not
   a reservation.

Fallbacks if a name is lost before registration: host → `mcp-host-runtime` →
`mcp-conformance-host`; everything server → `mcp-everything`; validator → `mcp-validator`;
core → `mcp-conformance-types` (check at need).

## Consequences

### Positive

- Self-describing family: anyone seeing any crate infers the others; repo and crates
  cohere without claiming the contested bare name.
- Zero collision with official naming, present (`mcp-conformance` internal, `mcp-spec`
  reserved) or implied — consistent with the upstream-first posture
  ([07-ecosystem-engagement.md](../07-ecosystem-engagement.md)).
- Descriptive names do SEO work that invented names (the discarded `praxis` direction)
  would have needed marketing to achieve.

### Negative

- Long names (`mcp-conformance-core` is 20 characters) in `Cargo.toml` and imports.
- Forgoing the bare `mcp-conformance` crate means the repo and primary crate names differ —
  minor, permanent explanation burden in the README.
- Early `0.1.0` publishes start the public-API clock before the design has fully settled;
  mitigated by honest pre-1.0 semantics ([02-architecture.md](../02-architecture.md)).

## Alternatives Considered

### Publish the bare `mcp-conformance` as the umbrella/CLI crate

Rejected: collides with the official SDK's internal package name (register 3.3). The
registry would allow it; the relationship this project depends on would not.

### Invented project name (`praxis`-style brandable)

Rejected: the specific candidate was already taken (verified), and the category lesson
stands — invented names trade discoverability for distinctiveness this project doesn't
need. The repo serves an ecosystem function; its names should say so.

### Single mega-crate with feature flags

Rejected for the same reasons as a2a-rust's ADR-0001: dependency pollution across concerns
(the validator must not pull rmcp — [02-architecture.md](../02-architecture.md)), and
crate-level separation is the enforcement mechanism for the judge/subject boundary.

### Defer all registration until 1.0

Rejected: register 5.8 documents the namespace's burn rate; losing `mcp-trace-validator` to
an unrelated project would cost more than early publishing's API-clock pressure.
