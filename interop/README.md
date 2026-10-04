<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Cross-SDK interop check

Real sessions between the official MCP SDKs, recorded through
`mcp-trace-capture` and judged by `mcp-trace-validator`. Nothing from this
workspace speaks MCP here except the recorder and the judge, so a pass is
evidence about independent traffic, not about this project's own server.

| Session | Client | Server | Transport | Expected |
|---|---|---|---|---|
| `ts-stdio` | TypeScript SDK 1.32.0 | `@modelcontextprotocol/server-everything` 2026.8.31 | stdio | `pass` at `2025-11-25` |
| `ts-http` | TypeScript SDK 1.32.0 | the same, `streamableHttp` | HTTP proxy | `pass` at `2025-11-25` |
| `py-stdio` | Python SDK (`mcp` 2.3.0) | [`py-server.py`](py-server.py) | stdio | `pass` at `2026-07-28` |
| `py-http` | Python SDK | `py-server.py http` | HTTP proxy | `pass` at `2026-07-28` |
| `ts-2025-06-18` | TypeScript SDK 1.13.0 | the everything server | stdio | refused, exit 2 (no `2025-06-18` registry) |

CI runs it on every push (the `interop` job). To run it locally:

```bash
cargo build -p mcp-trace-capture -p mcp-trace-validator
cd interop
npm ci
python3 -m venv .venv && .venv/bin/pip install --require-hashes -r requirements.txt
PYTHON=.venv/bin/python ./run.sh
```

Traces and JSON reports land in `out/`. The scripts double as worked examples
of the two ways to record a session: launch the server through
`mcp-trace-capture … stdio -- <server>`, or point the client at
`mcp-trace-capture … http --upstream <server-url>`.
