<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0009: Reference Host on rmcp's Client Surface, Scripted Interaction as Data

**Date:** 2026-06-11
**Status:** Accepted (amended 2026-06-12)
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

## Amendment (2026-06-12): the measured client contract, and where rmcp ends

The transports/binary slice decoded the rest of the `0.1.16` client runner and
measured rmcp 1.7's transport against the `sse-retry` scenario. Three findings
bind this ADR's consequences:

1. **Client-side verdicts treat WARNING as failure.** The runner's result
   judgment (`bn`/`G` in the bundle) fails a scenario on any `FAILURE` *or*
   `WARNING` check, a timeout, or a non-zero exit — unlike server-side runs,
   where warnings are informational. `sse-retry`'s `Last-Event-ID` check is a
   SHOULD that emits WARNING when unmet, so it is effectively mandatory for a
   green client run. The expected-failures YAML is scenario-granular with
   separate `server:`/`client:` keys, and stale entries (listed but passing)
   fail the run — the same both-directions discipline as our agreement
   baseline. `--suite` runs scenarios in parallel; the gate runs the four
   protocol scenarios as sequential `--scenario` invocations so the
   `sse-retry` clock is never measured under parallel load.
2. **rmcp 1.7 cannot pass `sse-retry`, measured twice.** At source: POST
   response SSE streams are wrapped by `raw_sse_to_jsonrpc` — explicitly
   "without reconnection logic" (`streamable_http_client.rs:783`) — so an
   in-flight request is lost when its stream closes early, while the one
   wrapper that *does* honor `retry`/`Last-Event-ID`
   (`SseAutoReconnectStream`, `client_side_sse.rs:262-281`) guards only the
   standalone GET stream, which opens immediately after initialization. On
   the wire (probe binary forcing the agent plan through rmcp's transport,
   2026-06-12, runner `checks.json` retained):
   `client-sse-retry-timing` **FAILURE** — "reconnected too early (−53ms
   instead of 500ms)" (the pre-existing GET predates the stream close, so the
   measured delay is negative); `client-sse-last-event-id` **WARNING** — no
   `Last-Event-ID` offered; and the `tools/call` never completes, which is
   the real resilience gap underneath the scenario's clock. Upstream filing
   is tracked as an M4 engagement item (register row 3.12).
3. **The host therefore implements the resumption dance itself** (`resume`,
   feature `http`) — *on rmcp's public `StreamableHttpClient` seam*, not a
   parallel HTTP stack: `post_message`/`get_stream` are the official client
   primitives, and the dance adds only the spec's orchestration (read the
   POST stream to its close, honor the server-named `retry` through
   `RetryPolicy::delay_honoring_retry_after` — the consequence this ADR
   predicted — then GET with `Last-Event-ID` and read the pending result).
   Naming the seam's types makes `reqwest` (the trait's only shipped
   implementation), `futures`, and `sse-stream` direct dependencies of the
   `http` feature, all version-mirroring rmcp's own tree; the dependency
   table carries their rows.

Two consequences for the binary, found the hard way: the runner's 30 s kill
signals only the `sh -c` wrapper it spawns (`shell: true`), so a host whose
in-flight call hangs would orphan itself holding the runner's pipes open and
wedge the run — the host owns a hard `--deadline-secs` (default 25 s, inside
the runner's 30) and exits 1 with a diagnostic instead. And scenario results
land in `<output-dir>/<scenario>-<timestamp>/checks.json` (client mode), not
the server mode's `<scenario>/checks.json` — the xtask client gate globs
accordingly.

With the dance in place, all four protocol scenarios pass at the pin:
`initialize`, `tools_call` (1/1), `elicitation-sep1034-client-defaults`
(5/5), `sse-retry` (3/3 — timing inside the −50/+200 ms window,
`Last-Event-ID` offered).

## Amendment (2026-11-12): the host's response bound is rmcp's, and the probe's own

The first amendment says where rmcp's client ends and the host's orchestration
begins. One property sat on rmcp's side without being named: **how much of a
response the host will buffer.** The host sets no size limit of its own, so the
bound it has is the one rmcp's client applies, and until rmcp 3.9.0 that was
none for a non-streaming response ([RUSTSEC-2026-0462](https://rustsec.org/advisories/RUSTSEC-2026-0462.html),
register row 4.9): a server could answer an `initialize` with a JSON body that
never ended, and the host buffered it until the 25-second deadline, which bounds
time and not memory (measured against 3.7.0: 3,000 MiB sent, 3,041 MiB resident
in under three seconds). Being "an rmcp client, not a protocol reimplementation"
meant inheriting this as it meant inheriting the rest.

The decision stands, and its price is stated: **the host takes rmcp's bounds
rather than writing its own.** The workspace requires rmcp 3.9.0 or later
(16 MiB of JSON, 64 KiB of error body, an oversized `Content-Length` refused before
reading), and `crates/mcp-reference-host/tests/hostile_bodies.rs` pins that from
outside, against the real executable, so a future rmcp that loosens it fails a
test here rather than reaching an operator. A host-level wrapper that capped the
transport itself was the alternative; it would have departed from "the host is
rmcp's own client", and it would have been a second copy of a limit rmcp now owns.

What rmcp's bound does not reach is `--probe`: its requests are raw HTTP through
`reqwest`, deliberately, because rmcp's client will not emit a malformed one. That
path reads at most 1 MiB of any response (`probe/designated.rs`, `MAX_BODY`),
enough for a JSON-RPC error or a short error page.

rmcp 3.8 also changed how a 429 or a 5xx reaches the caller (a typed, body-free
`HttpStatusError` instead of text), which `diagnose.rs` reads directly; the status
read back out of text remains for every other non-success response.
