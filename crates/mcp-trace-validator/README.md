<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-trace-validator

Deterministic offline validation of recorded MCP protocol traces: replay a JSON Lines
trace against the requirement registry and get requirement-level findings — the clause
verbatim with its link, the offending event `seq`, actionable detail — as human text,
JSON, JUnit XML, or SARIF.

```text
mcp-trace-validator validate session.jsonl                 # judged at the revision it declares
mcp-trace-validator validate --quiet session.jsonl         # only what needs attention
mcp-trace-validator validate - --format json < session.jsonl
mcp-trace-validator validate session.jsonl --format sarif > results.sarif
mcp-trace-validator validate session.jsonl --revision 2025-11-25 --revision 2026-07-28
mcp-trace-validator requirements --revision 2025-11-25
```

`--format sarif` is SARIF 2.1.0 for code scanning (GitHub, GitLab, IDE viewers): one
rule per violated clause, one result per finding at the trace line holding its event.
`--format json` is described by the JSON Schema at
[`schema/report.schema.json`](schema/report.schema.json)
(`report::JSON_SCHEMA`).

A trace is JSON Lines — one event per line with a capture-assigned `seq`,
`direction`, `transport`, and a `kind`-discriminated body (`message` carries the
JSON-RPC payload), specified by `mcp-conformance-core`'s trace-event JSON Schema:

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
