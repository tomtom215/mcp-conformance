<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-everything-server

A Rust MCP server exercising every protocol capability — the reference artifact
SEP-1730's appendix asks each SDK to carry — **under construction at milestone M2**,
built on [rmcp](https://crates.io/crates/rmcp) (the official Rust SDK). What exists
now, with nothing claimed beyond it:

- `policy` — the default-secure HTTP transport policy (loopback-only `Host`/`Origin`
  allowlisting, fail-closed parsing) that closes the CVE-2026-42559 DNS-rebinding
  class before any listener exists.
- `server::EverythingServer` — the rmcp `ServerHandler`, pinned to protocol
  `2025-11-25`, advertising **only what it implements** (a conformance reference
  that over-claims capabilities would fail the suite it exists to pass). Implemented
  today: tools (`echo`, `add`, mirroring the TypeScript everything server's
  phrasing).
- A `stdio` binary: `mcp-everything-server --transport stdio`.

Streamable HTTP (behind `policy`), resources, prompts, logging, and completions land
module-by-module; each is advertised the commit it is implemented.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
