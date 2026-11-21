<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-reference-host

A native Rust MCP host (client) and bounded agent loop, built on
[rmcp](https://crates.io/crates/rmcp). The
[mcp-conformance](https://github.com/tomtom215/mcp-conformance) project runs it as
the official conformance suite's client under test, and uses it to drive servers
through recorded sessions. It is also a quick way to exercise an MCP **server**, as below.

```text
cargo install mcp-reference-host
mcp-reference-host --server-cmd "python my_server.py" --sweep --trace-dir traces/
mcp-reference-host --url http://127.0.0.1:3000/mcp --sweep --revision 2026-07-28
mcp-trace-validator validate traces/*.jsonl
```

`--sweep` drives prompts, resources, templates and completion after the tool
loop; `--trace-dir` records the session (the recording sits at the message seam,
so it carries no HTTP headers — record with `mcp-trace-capture` when transport
behaviour matters). `--help` lists the rest.

What the library contains:

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
  `mcp-everything-server`. The generic plan passes against that server with
  default flags: it allows one turn per listed tool, and plans the suite's
  `test_error_handling` *expecting* its `isError: true` result, which is
  recorded rather than counted (that tool succeeding, or any other tool
  failing, still counts against the zero error budget).
- `diagnose` — errors for humans: a session that cannot start is reported as
  its cause (`cannot connect to http://127.0.0.1:1/mcp: connection refused`,
  `…/wrong: the server answered 404 Not Found — is the MCP endpoint path
  right?`, a stdio server that exited or was not found), found by walking the
  error's source chain rather than printing rmcp's transport types.
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
  `StreamableHttpClient` seam because rmcp's own transport lost an in-flight
  request when its POST SSE stream closed early (measured at rmcp 1.7; not
  re-measured since the workspace moved to 3.x).
- `oauth` (feature `oauth`, **off by default** and not part of `cli`) — OAuth
  for the official suite's `auth/*` client scenarios, and for nothing else.
  Build with `--features oauth` and pass `--oauth-conformance <url>`: the host
  then answers the server's `401` and `403 insufficient_scope` by running an
  authorization and sending the request again. The OAuth itself is rmcp's
  `auth` feature — protected-resource and authorization-server discovery,
  registration (a pre-registered client, a Client ID Metadata Document, or
  dynamic registration), PKCE `S256` always, the `state` check, the RFC 8707
  `resource` parameter, scope selection and step-up, and the
  `client_credentials` grant. The host adds what rmcp leaves to its caller.

  **This is a conformance harness mode, not a way to sign in.** Nobody is
  asked for consent: the host requests the authorization URL itself and reads
  the code off the redirect. It is therefore fenced: the authorization endpoint
  must be `localhost` or a loopback address (anything else is refused before a
  request is made), the redirect must go to the host's own redirect URI, a
  session makes at most three authorization requests (one sign-in, two
  escalations) and then believes the server's refusal, and tokens, codes and
  client secrets live in memory only — never on disk, in a trace or in any
  message the host prints. Which client to be comes from the suite's
  `MCP_CONFORMANCE_CONTEXT`. It is a separate feature because rmcp's `auth`
  brings the `oauth2` crate and older lines of `rand`, `sha2`, `thiserror` and
  `base64` with it, which `cargo install mcp-reference-host` should not pay
  for.

  At the pinned suite 0.1.16, 19 of its 22 `auth/*` scenarios pass and are
  gated by `cargo xtask conformance`. `auth/2025-03-26-oauth-metadata-backcompat`
  fails (the scenario's metadata carries an `issuer` that does not match where
  it was fetched from, which RFC 8414 says must not be used and rmcp refuses);
  `auth/client-credentials-jwt` and `auth/cross-app-access-complete-flow`
  (both suite extensions) are not implemented and not run.
- the binary (feature `cli`, on by default: `cargo install mcp-reference-host`
  installs it; library users opt out with `default-features = false`) — the
  official suite's client SUT: the runner
  appends the scenario server's URL as the final argument and names the
  scenario in `MCP_CONFORMANCE_SCENARIO`; `scenario.rs` is the one table
  mapping names to plans. All four `2025-11-25` protocol scenarios pass at
  the pinned 0.1.16 (`initialize`, `tools_call`, `sse-retry` 3/3 including
  the retry-timing window, `elicitation-sep1034-client-defaults` 5/5). The
  suite's `auth/*` (OAuth) client scenarios need the `oauth` feature above. The
  host owns a hard `--deadline-secs` (default 25): the runner's 30 s kill
  reaches only the `sh -c` wrapper it spawns, so a host that outlives its
  server must exit by itself rather than wedge the runner.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
