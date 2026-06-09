<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-trace-validator

Deterministic offline validation of recorded MCP protocol traces: replay a JSON Lines
trace against the requirement registry and get requirement-level findings — spec quote,
offending event `seq`, actionable detail — as human text or machine JSON.

```text
mcp-trace-validator validate session.jsonl
mcp-trace-validator validate - --format json < session.jsonl
mcp-trace-validator requirements
```

Exit codes: `0` pass (warnings allowed unless `--strict`), `1` violations, `2`
invocation/registry problem, `3` malformed trace.

The library engine (`default-features = false`) has no CLI dependencies and performs no
I/O. Part of [mcp-conformance](https://github.com/tomtom215/mcp-conformance); see the
repository's `docs/plan/` for scope and roadmap.

License: MIT
