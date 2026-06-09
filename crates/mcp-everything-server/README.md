<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-everything-server

A Rust MCP server that will exercise every protocol capability — the reference artifact
SEP-1730's appendix asks each SDK to carry. **The server itself is not built yet** (it
lands at roadmap milestone M2). What ships today is `policy`: the default-secure HTTP
transport policy (loopback-only `Host`/`Origin` allowlisting, fail-closed parsing) that
closes the CVE-2026-42559 DNS-rebinding class before any listener exists.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
