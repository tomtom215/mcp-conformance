<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0019: A Recording Tap Is Not a Gateway — Ship `mcp-trace-capture`

**Date:** 2026-09-25
**Status:** Accepted (narrows [ADR-0002](0002-product-scope.md)'s "not a gateway/proxy"
non-goal; adds a fifth published crate)
**Author:** Tom F.

---

## Context

The README's first sentence promises "record a trace of any MCP session — in any
language, over any transport". Nothing shipped to do it. The only recorders were the
everything server's tap (non-default feature, HTTP only, and only for traffic to this
project's own server) and the reference host's capture (only sessions it drives, at the
rmcp message seam). A maintainer of a Python or TypeScript server had no path from their
session to a verdict ([project review](../../reports/project-review-2026-09-24.md), H1).

The charter lists "not a gateway/proxy" as a non-goal, with agentgateway owning that
space in Rust. The obvious recorder for a streamable-HTTP server is a reverse proxy, and
for a stdio server a process wrapper — both are "proxies" in the literal sense.

## Decision

1. Ship `mcp-trace-capture`, a separate crate with two modes: `stdio -- <server>` (the
   client launches the wrapper in place of the server) and `http --upstream <url>` (a
   reverse proxy). It records the trace format the validator reads, using the schema
   types from `mcp-conformance-core`, and links no MCP SDK.
2. The non-goal is narrowed, not dropped: this project does not build a **gateway** —
   routing, authentication, policy enforcement, multiplexing, rate limiting, or anything
   that changes what a session does. A recording tap forwards bytes unchanged and
   exists only to produce a trace; that is the whole of its scope. A feature request
   that would make it route, filter, rewrite, or authenticate is out of scope under
   the original non-goal.
3. It is a crate of its own, not a validator subcommand, so the validator library stays
   free of an async runtime and an HTTP stack.
4. Its HTTP client is hyper with `hyper-rustls` (the `ring` provider), not reqwest's TLS
   features: those are shared with `mcp-reference-host` by Cargo feature unification,
   and enabling them here made the host panic in every workspace build — found by
   running the two binaries together, not by the test suite of either.
5. `cargo xtask conformance` gains a capture leg: reference-host sessions through the
   stdio wrapper must judge clean at both revisions, and the official suite must stay
   green through the HTTP proxy.

## Consequences

### Positive

- The README's headline claim has a tool behind it, verified end to end in CI.
- The recorder is SDK-independent, so a trace describes the wire, not one SDK's
  reading of it — which the host's message-seam capture could not claim.

### Negative

- A fifth published crate, and the first release that includes it needs the owner to
  bootstrap its crates.io trusted publishing (RELEASING.md §Publish order).
- New transitive dependencies (`hyper-rustls`, `rustls`, `ring`), and two accepted
  duplicate versions through `ring` (`deny.toml`).
- One trace per session remains the user's responsibility: concurrent clients through
  one proxy interleave.

## Alternatives Considered

- **A `capture` subcommand of `mcp-trace-validator`.** Rejected: it would put tokio and
  an HTTP stack behind a library whose design point is a pure `&[TraceEvent] -> Report`.
- **Document the trace format and let users write their own tap.** Rejected: that is
  the status quo the review found, and every user would re-solve ordering, SSE framing
  and header redaction — each of which this project's own tap got wrong at least once.
- **Extend the everything server's tap.** Rejected: it only sees traffic to that server.
- **Recommend an existing recorder (e.g. `mcpsnoop`).** Rejected as the only path: its
  JSONL is not this trace format, and a converter would still need the ordering and
  header guarantees above. Worth revisiting as an import format if users ask.
