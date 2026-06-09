<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-reference-host

A native Rust MCP host and bounded agent loop — the client-side system-under-test for
the conformance toolkit. **The host itself is not built yet** (it lands at roadmap
milestone M3). What ships today is `retry`: the deterministic exponential-backoff
policy (caller-supplied jitter, `Retry-After` honoring with hard caps) the host's
transport layer will be built on.

Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
