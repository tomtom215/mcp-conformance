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

**Not here yet** (next M3 slices, tracked in the roadmap): the binary, the
child-process and streamable-HTTP transports, official-suite client-scenario
wiring (`initialize`, `tools_call`, `sse-retry`,
`elicitation-sep1034-client-defaults` at pinned 0.1.16 — the `auth/*` set is
deferred exactly as the registry's TRAN-009 records), and host-side trace
capture.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
