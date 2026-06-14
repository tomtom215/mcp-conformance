<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# rmcp streamable-HTTP SSE-resumption gap — ready-to-file dossier

**What this is.** A ready-to-file upstream bug dossier for the official Rust SDK
([`rmcp`](https://github.com/modelcontextprotocol/rust-sdk)): its streamable-HTTP
*client* loses an in-flight request when the POST response's SSE stream closes
early, because that stream is wrapped without reconnection logic. Filing is a
maintainer action; this document carries the verified mechanism, the measured
wire behavior, and the exact text to post — the in-repo analogue of the
[#10](https://github.com/tomtom215/mcp-conformance/issues/10) dossier, kept as a
committed file rather than an issue. Tracked by the deferral-ledger row
`rmcp-sse-resumption-upstream-filing` ([deferrals.json](../plan/deferrals.json),
review-by 2026-07-01) and engagement backlog item 10
([07-ecosystem-engagement.md](../plan/07-ecosystem-engagement.md)); the register
record is [3.12](../plan/01-ecosystem-context.md).

## Verified mechanism (source re-confirmed 2026-06-13, rust-sdk head `266f870`)

All in `crates/rmcp/src/transport/streamable_http_client.rs` (head `266f870`,
the `main` tip at the 2026-06-13 confirmation — re-confirm at filing time with
the one-liner in the last section; line numbers are for that head):

1. `StreamableHttpClientWorker::raw_sse_to_jsonrpc` (line 303) carries the
   verbatim doc comment **"Convert a raw SSE stream into a JSON-RPC message
   stream without reconnection logic."** (lines 301–302).
2. It is the wrapper applied to every **POST** response SSE stream: the
   `Ok(StreamableHttpPostResponse::Sse(stream, ..))` match arms at lines
   777–779 and 803–804 spawn `Self::execute_sse_stream(Self::raw_sse_to_jsonrpc(stream), …)`.
   So when a POST's SSE response stream closes before the JSON-RPC reply arrives,
   the in-flight request is simply lost — there is no reconnect, no
   `Last-Event-ID` resume.
3. The wrapper that *does* honor `retry`/`Last-Event-ID` —
   `SseAutoReconnectStream` with its `retry_connection(last_event_id)` machinery
   in `crates/rmcp/src/transport/common/client_side_sse.rs` — guards only the
   **standalone GET** stream, which opens right after initialization. The POST
   path never reaches it.

The asymmetry is the bug: the spec's resumption contract
([`2025-11-25` streamable HTTP](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports),
SSE `retry` + `Last-Event-ID`) applies to *any* SSE stream that can carry a
pending response, but rmcp wires reconnection to the GET stream only.

## Measured on the wire (2026-06-12, suite `0.1.16`, `sse-retry` client scenario)

A probe binary driving an agent plan through rmcp 1.7's transport, run as the
SUT against `npx @modelcontextprotocol/conformance@0.1.16 client --scenario
sse-retry` (runner `checks.json` retained; full record in
[ADR-0009 §Amendment](../plan/decisions/0009-reference-host-on-rmcp-client.md)):

- `client-sse-retry-timing` — **FAILURE**: "reconnected too early (−53ms instead
  of 500ms)". The delay is *negative* because the only stream that reconnects is
  the pre-existing GET, opened before the POST stream closed — so the suite
  measures the wrong stream's timing.
- `client-sse-last-event-id` — **WARNING**: no `Last-Event-ID` offered on
  resumption (a SHOULD; client-side runs treat a WARNING as a failure).
- `tools/call` — **never completes**: the real resilience gap underneath the
  scenario's clock. The in-flight call is dropped with the closed POST stream.

## The fix is buildable on rmcp's *public* seam (this repo proves it)

`mcp-reference-host` (`resume` module, feature `http`) implements the compliant
resumption dance **on rmcp's public `StreamableHttpClient` trait** — no parallel
HTTP stack: it uses the official `post_message`/`get_stream` primitives, reads
the POST stream to its close, honors the server-named `retry` through
`RetryPolicy::delay_honoring_retry_after`, then issues a GET carrying
`Last-Event-ID` and reads the pending result. With that dance in place the same
suite scenario passes **3/3** (timing inside the −50/+200 ms window,
`Last-Event-ID` offered). That is the evidence the gap is closable inside rmcp
itself, not only by a downstream workaround.

## Ready-to-file text

> **Title:** streamable-HTTP client loses an in-flight request when its POST SSE
> stream closes — POST response streams are wrapped "without reconnection logic"
>
> **Body:**
> The streamable-HTTP client drops an in-flight request if the SSE stream of a
> POST response closes before the JSON-RPC reply arrives, and never offers
> `Last-Event-ID` to resume it. It fails the official suite's `sse-retry` client
> scenario. Affects `rmcp = 1.7.0` and current `main` (`266f870`).
>
> **Mechanism** (`crates/rmcp/src/transport/streamable_http_client.rs`,
> `266f870`): `StreamableHttpClientWorker::raw_sse_to_jsonrpc` (l. 303) is
> documented "Convert a raw SSE stream into a JSON-RPC message stream without
> reconnection logic" and is the wrapper used for every POST response SSE stream
> (`StreamableHttpPostResponse::Sse` at l. 777–779 and 803–804). The reconnecting
> wrapper `SseAutoReconnectStream` (with `retry_connection(last_event_id)`, in
> `transport/common/client_side_sse.rs`) guards only the standalone GET stream
> that opens after initialization. So a POST whose response stream closes early
> loses its in-flight request, and resumption with `Last-Event-ID` never happens
> on that path.
>
> **Reproduce:** run any rmcp-1.7 streamable-HTTP client as the SUT against the
> pinned suite —
> `npx -y @modelcontextprotocol/conformance@0.1.16 client --scenario sse-retry
> --command "<your-rmcp-client-binary>" --spec-version 2025-11-25`.
> Observed: `client-sse-retry-timing` FAILURE ("reconnected too early"),
> `client-sse-last-event-id` WARNING (no `Last-Event-ID`), and the in-flight
> `tools/call` never completes.
>
> **The fix is reachable on the public client seam.** A third-party reference
> host implements the compliant dance entirely on `StreamableHttpClient`'s public
> `post_message`/`get_stream` primitives — read the POST stream to close, honor
> the server-named `retry`, then GET with `Last-Event-ID` — and passes `sse-retry`
> 3/3. The same orchestration belongs on the POST path inside
> `StreamableHttpClientWorker`: apply a reconnecting wrapper (like
> `SseAutoReconnectStream`) to POST response streams too, threading the last seen
> event id. Happy to PR with a regression test built on the suite scenario.

## Reproducible method

```sh
# Confirm the source structure at the head you are filing against.
curl -s "https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/transport/streamable_http_client.rs" \
  | grep -nE 'raw_sse_to_jsonrpc|without[[:space:]]+reconnection'

# Measure on the wire: any rmcp-1.7 streamable-HTTP client as SUT.
npx -y @modelcontextprotocol/conformance@0.1.16 client \
  --scenario sse-retry \
  --command "<your-rmcp-client-binary>" \
  --spec-version 2025-11-25
# Read <output-dir>/sse-retry-<timestamp>/checks.json for the per-check verdicts.
```

## Action

- [ ] File the issue above on `modelcontextprotocol/rust-sdk` (maintainer action;
      re-run the source-confirm one-liner against the head at filing time).
- [ ] On maintainer interest: PR the POST-path reconnection with a regression
      test built on the `sse-retry` scenario.
- [ ] When fixed in a released rmcp we adopt: update register row 3.12, retire
      the deferral row `rmcp-sse-resumption-upstream-filing`, and note whether the
      reference host's `resume` dance can lean on rmcp directly.
