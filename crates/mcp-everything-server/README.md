<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-everything-server

A Rust MCP server exercising every protocol capability — the reference artifact
SEP-1730's appendix asks each SDK to carry — built on
[rmcp](https://crates.io/crates/rmcp) (the official Rust SDK). Milestone M2 is
complete: **40/40 checks** on the official conformance suite's `2025-11-25`
server scenarios (pinned suite 0.1.16, enforced in CI via
`cargo xtask conformance`), and the server is offered upstream as
[rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902).

- `policy` — the default-secure HTTP transport policy (loopback-only
  `Host`/`Origin` allowlisting, fail-closed parsing; duplicate `Host` or
  `Origin` headers are denied outright) that closes the CVE-2026-42559
  DNS-rebinding class: disallowed requests get 403 before any MCP processing.
- `server::EverythingServer` — the rmcp `ServerHandler`, pinned to protocol
  `2025-11-25`, implementing the suite's full server surface: every
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
  `elicitation/create` and, on consent, the completion notification for the
  issued id — the host↔server round trip is pinned end to end in
  `mcp-reference-host`'s `agent_loop` tests.
- A binary serving both transports: `mcp-everything-server --transport stdio`
  or `--transport http` (`--bind` for the address; policy overrides via
  `--allowed-host` / `--dangerously-allow-any-host`).
- A session trace tap (feature `tap`, `--tap-dir`): records each admitted HTTP
  session as a validator-ready JSON Lines trace. Only the headers in the
  public `RECORDED_HEADERS` allowlist are ever captured — credential-bearing
  headers never reach a trace.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
