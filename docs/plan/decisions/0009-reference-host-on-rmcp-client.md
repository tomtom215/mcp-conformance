<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0009: Reference Host on rmcp's Client Surface, Scripted Interaction as Data

**Date:** 2026-06-11
**Status:** Accepted
**Author:** Tom F.

---

## Context

M3's definition of done requires a host that completes bounded tool-use loops
against the everything server over stdio and streamable HTTP, passes the official
suite's client scenarios as the SUT, answers sampling/elicitation (including URL
mode)/roots without any model-provider network use, honors `Retry-After`, and
captures validator-ready traces.

The pinned suite (0.1.16) defines the client-SUT contract, verified against the
published bundle: the runner splits `--command` on spaces, **appends the
scenario's server URL as the final argument**, spawns with a shell, sets
`MCP_CONFORMANCE_SCENARIO` (and `MCP_CONFORMANCE_CONTEXT` where a scenario
provides one), allows 30 s, and judges only what the client *did* against the
scenario's in-process server. Four `2025-11-25` protocol scenarios exist —
`initialize`, `tools_call`, `sse-retry`, `elicitation-sep1034-client-defaults` —
plus fourteen `auth/*` OAuth scenarios.

rmcp 1.7 already models the entire client surface this needs: `ClientHandler`
with `create_message`, `create_elicitation` (a `mode`-tagged enum whose
`UrlElicitationParams` variant carries `url` + `elicitationId`), `list_roots`,
and `on_url_elicitation_notification_complete`; client transports exist for
child processes (`transport-child-process`) and streamable HTTP
(`transport-streamable-http-client-reqwest`).

## Decision

1. **The host is an rmcp client, not a protocol reimplementation.** Like the
   everything server, it builds on the official SDK and exists to prove the
   toolkit's claims against it — divergence from rmcp behavior is a finding,
   not a feature. Transports come from rmcp's official client features; the
   reqwest stack enters the tree only through
   `transport-streamable-http-client-reqwest`, never as a direct dependency.
2. **Interaction is data, not code.** Every behavior a model or user would
   supply is an `InteractionScript` value: the sampling reply, the elicitation
   policy (SEP-1034 defaults / fixed content / decline / cancel), the URL-mode
   consent policy, and the roots list. CI runs are exactly reproducible;
   "zero model-provider network use" holds by construction because there is
   no code path that could perform it.
3. **The loop is bounded by construction.** `run` executes a deterministic
   call policy with an explicit stop-condition lattice — cancellation, turn
   limit, error budget, completion — checked in that order, every stop reason
   a tested variant. No "the loop usually terminates".
4. **Suite scenarios are scripts, not modes.** The binary maps
   `MCP_CONFORMANCE_SCENARIO` to an `InteractionScript` + `RunPlan`; unknown
   scenarios get the generic discover-and-call plan. Scenario knowledge lives
   in one table the suite pin governs.
5. **Auth scenarios are out of this milestone**, exactly as the registry's
   TRAN-009 exclusion already records for the server side: the suite's
   `auth/*` set requires a full OAuth client (metadata discovery, registration,
   PKCE flows) and is tracked as follow-on work, not silently skipped.

## Consequences

### Positive

- The four protocol scenarios exercise rmcp's client transports end to end —
  any gap found (SSE retry timing, `Last-Event-ID` resumption) is upstream
  evidence for M4, measured by the official runner.
- `retry.rs` (shipped in v0.1.0) becomes load-bearing: the SSE `retry` field
  is a server-named delay, which is precisely `delay_honoring_retry_after`.

### Negative

- reqwest's dependency tree enters the workspace (behind the host's `http`
  feature). Accepted: it is the official SDK's own client path, and
  `cargo deny` gates its licenses and advisories like everything else.
- Scripted handlers cannot prove UI-facing MUSTs (consent display, review
  affordances); those clauses remain registry exclusions, now with the host
  named where it enforces the wire-visible half.

## Alternatives Considered

### Hand-rolled JSON-RPC client (no rmcp)

Rejected: ADR-0002 scopes this project to building *on* the official SDK and
contributing back; a parallel client stack would fork the ecosystem instead of
proving it.

### Model-pluggable agent loop (real LLM behind a trait)

Rejected for M3: the DoD demands zero model-provider network use, and the
conformance value lies in deterministic, replayable behavior. A model-backed
policy can layer on top of `InteractionScript` later without changing the loop.
