<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-trace-validator

Deterministic offline validation of recorded MCP protocol traces: replay a JSON Lines
trace against the requirement registry and get requirement-level findings — spec clause,
offending event `seq`, actionable detail — as human text, machine JSON, or JUnit XML.

```text
mcp-trace-validator validate session.jsonl
mcp-trace-validator validate - --format json < session.jsonl
mcp-trace-validator requirements
mcp-trace-validator validate session.jsonl --registry my-registry.json  # default: built-in 2025-11-25
```

A trace is JSON Lines — one event per line with a capture-assigned `seq`,
`direction`, `transport`, and a `kind`-discriminated body (`message` carries the
JSON-RPC payload verbatim):

```jsonl
{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"my-host","version":"1.0.0"}}}}
```

Findings name the requirement, quote-backed by the registry, and the offending
event: `seq 3: request "tools/list" reuses id 1, already used by the same party
at seq 0`.

Exit codes: `0` pass (warnings allowed unless `--strict`), `1` violations, `2`
invocation/registry problem, `3` malformed trace.

The library engine (`default-features = false`) has no CLI dependencies and performs no
I/O. Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
