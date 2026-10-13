<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-everything-server

A Rust MCP server, built on [rmcp](https://crates.io/crates/rmcp) (the official
Rust SDK), that exercises the protocol's capabilities at both `2025-11-25` and
`2026-07-28`. The [mcp-conformance](https://github.com/tomtom215/mcp-conformance)
project uses it to calibrate its validator: on every change CI runs the official
conformance suite against it — **40/40 checks** on the `2025-11-25` server
scenarios at the pinned suite 0.1.16 — and compares the suite's verdicts with the
validator's. It is also a known-good peer for testing an MCP **client**.

> **This README is `main`'s.** `cargo install` gets the latest release, `0.6.0`,
> whose own README is the one on crates.io. Documentation for the release is at
> <https://mcp-conformance.com>, and for `main` at <https://mcp-conformance.com/dev/>.
>
> **Not yet released** (on `main` since `0.6.0`):
>
> - `--revision` (an alias of `--protocol-version`) and `--listen` (an alias of
>   `--bind`), which the examples below use;
> - the endpoint URL the HTTP server prints after its readiness line.
>
> With `0.6.0`, spell them `--protocol-version` and `--bind`.

## Quick start

```text
cargo install mcp-everything-server
mcp-everything-server                                  # stdio, 2025-11-25
mcp-everything-server --transport http                 # prints its endpoint URL
mcp-everything-server --transport http --revision 2026-07-28
```

**Testing a client.** Serve over HTTP, put
[`mcp-trace-capture`](https://crates.io/crates/mcp-trace-capture) in front, point
your client at the proxy, then judge the recording:

```text
mcp-everything-server --transport http --listen 127.0.0.1:3000
mcp-trace-capture -o client.jsonl http --upstream http://127.0.0.1:3000
# client URL: http://127.0.0.1:8080/mcp   (Ctrl-C the proxy when done)
mcp-trace-validator validate client.jsonl
```

The report judges both parties; with this server on the other end, a finding
is almost certainly the client's.

## What it contains

- `policy` — the default-secure HTTP transport policy (loopback-only
  `Host`/`Origin` allowlisting, fail-closed parsing; duplicate `Host` or
  `Origin` headers are denied outright) that closes the CVE-2026-42559
  DNS-rebinding class: disallowed requests get 403 before any MCP processing.
- `server::EverythingServer` — the rmcp `ServerHandler`, serving protocol
  `2025-11-25` by default, implementing the suite's full server surface: every
  suite-defined tool (sampling and elicitation included), resources with
  templates and subscriptions (capped per session), prompts, completions, and
  logging-level filtering — plus `get-structured-content`, the TypeScript
  everything server's structured-output tool (`outputSchema` +
  `structuredContent`), which the suite does not exercise but the spec
  defines.

  One TypeScript-server feature is a deliberate delta at this revision, not
  an omission: **async sampling** (the tasks pattern, which `2025-11-25` does
  not define — SEP-2663 moves tasks to an extension in `2026-07-28`).
  **URL-mode elicitation** closed when the reference host landed its
  URL-capable handler: `test_url_elicitation` sends a `mode: "url"`
  `elicitation/create` (only to a client that declared `elicitation.url`; a
  bare `elicitation: {}` is form mode only) and, on consent, the completion
  notification for the issued id — the host↔server round trip is pinned end to end in
  `mcp-reference-host`'s `agent_loop` tests.
- `server::ServedRevision` — the protocol revision an instance serves, chosen
  at construction (`EverythingServer::serving`, or `--protocol-version` on the
  binary). `2026-07-28` serves the **stateless** surface instead: no
  `initialize`, no sessions (SEP-2575), `server/discover` for capability
  advertisement, per-request `_meta` required at the transport, and SEP-2549
  caching hints on cacheable results. An `initialize` sent to it is refused
  with `-32022` naming the versions it does speak, rather than negotiated into
  a handshake that leads nowhere. The suite's `2026-07-28` scenarios pass
  **42/42** against this mode at the pinned `0.2.0-alpha.12`, and the
  repository's own requirement registry
  evidences **118 of the 141 judgeable clauses** across five committed captures
  of it over both transports, with 0 fail on the conforming ones and everything
  the sessions never touched reported *not observed* rather than counted as a
  pass. Server-to-client requests go by SEP-2322's MRTR pattern at this
  revision — `elicitation/create` and `sampling/createMessage` are *returned*
  inside an `input_required` result and the client retries — because the
  revision forbids sending them independently. `logging/setLevel` is gone with
  it: log notifications ride only requests whose `_meta` asked for them.
  `test_url_elicitation` is not listed at all, its feature having been removed.
- A binary serving both transports: `mcp-everything-server --transport stdio`
  or `--transport http` (`--listen`/`--bind` for the address; policy overrides
  via `--allowed-host` / `--dangerously-allow-any-host`;
  `--revision`/`--protocol-version` for the revision).
- A session trace tap (feature `tap`, `--tap-dir`): records each admitted HTTP
  exchange as a validator-ready JSON Lines trace, session or not. The feature is
  off by default, so a plain `cargo install mcp-everything-server` has no
  `--tap-dir`; install with `cargo install mcp-everything-server --features tap`,
  or record with `mcp-trace-capture` in front of the server instead. Only the
  headers in the public `RECORDED_HEADERS` allowlist (and the
  `RECORDED_HEADER_PREFIXES` arm, for SEP-2243's tool-designated
  `Mcp-Param-*`) are ever captured — credential-bearing headers never reach a
  trace.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
