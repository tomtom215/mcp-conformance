<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-reference-host

A native Rust MCP host and bounded agent loop — the client-side system-under-test
for the conformance toolkit (roadmap M3, ADR-0009), built on
[rmcp](https://crates.io/crates/rmcp) like the rest of the workspace. What ships:

- `script` — every behavior a model or user would supply, as data: the sampling
  reply, the elicitation policy (SEP-1034 schema defaults, fixed content,
  decline, cancel), URL-mode consent, and the roots list. Zero model-provider
  network use holds by construction: no code path could perform it.
- `handler` — the `rmcp::ClientHandler` answering from a script, with an event
  log making every server-initiated interaction assertable, and a pending-id
  set enforcing the spec's URL-elicitation client MUST (unknown or
  already-completed `elicitationId`s in `notifications/elicitation/complete`
  are ignored, observably).
- `run` — the bounded tool-use loop: scripted calls or discover-and-call-once
  with schema-derived arguments (local `$ref`s resolved, enum shapes sampled),
  under an explicit stop-condition lattice — cancellation, turn limit, error
  budget, completion — every variant a tested stop reason against the real
  `mcp-everything-server`.
- `retry` — the deterministic exponential-backoff policy (caller-supplied
  jitter, `Retry-After` honoring with hard caps) the transport layer builds on.
- `connect` — the two real transports, from rmcp's official client features:
  child-process stdio (feature `proc`) and streamable HTTP over reqwest
  (feature `http`).
- `capture` — host-side trace capture: a `Transport` wrapper recording every
  message as a validator-ready JSON Lines trace. Redaction by construction:
  the message seam never sees HTTP headers, so a host trace cannot leak
  credentials — and correspondingly carries no `kind: http` events for the
  validator's header-level checks to judge.
- `resume` (feature `http`) — the spec's SSE-resumption dance (server-named
  `retry` delay honored through `RetryPolicy::delay_honoring_retry_after`,
  `Last-Event-ID` offered on the GET reconnect), implemented on rmcp's public
  `StreamableHttpClient` seam because rmcp 1.7's own transport loses an
  in-flight request when its POST SSE stream closes early (measured; ADR-0009
  §Amendment).
- the binary (feature `cli`) — the official suite's client SUT: the runner
  appends the scenario server's URL as the final argument and names the
  scenario in `MCP_CONFORMANCE_SCENARIO`; `scenario.rs` is the one table
  mapping names to plans. All four `2025-11-25` protocol scenarios pass at
  the pinned 0.1.16 (`initialize`, `tools_call`, `sse-retry` 3/3 including
  the retry-timing window, `elicitation-sep1034-client-defaults` 5/5). The
  `auth/*` set is deferred exactly as the registry's TRAN-009 records. The
  host owns a hard `--deadline-secs` (default 25): the runner's 30 s kill
  reaches only the `sh -c` wrapper it spawns, so a host that outlives its
  server must exit by itself rather than wedge the runner.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
