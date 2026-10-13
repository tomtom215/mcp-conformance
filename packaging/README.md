<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

Clause-by-clause conformance checking for the
[Model Context Protocol](https://modelcontextprotocol.io): record any MCP session
— any language, any SDK, stdio or streamable HTTP — and find out which of the
specification's requirements it met, which it broke, where, and which it never
exercised.

This package installs the project's release binaries for your platform — the
same files, byte for byte, that the GitHub release attaches and attests:

- `mcp-trace-capture` records a session: a transparent stdio wrapper and HTTP
  reverse proxy that forward bytes unchanged;
- `mcp-trace-validator` judges a recorded trace against the specification;
- `mcp-conformance` runs the official conformance suite against a server or
  client through the recorder, and reports both verdicts.

```text
npx mcp-conformance --version          # npm
pip install mcp-conformance            # PyPI
mcp-trace-capture -o session.jsonl stdio -- python my_server.py
mcp-trace-validator validate session.jsonl
```

Prebuilt for Linux (x86_64, aarch64; static, any distribution), macOS (x86_64,
arm64) and Windows (x86_64). Elsewhere, build from source with
`cargo install mcp-trace-capture mcp-trace-validator`.

Documentation: <https://mcp-conformance.com>. Source, issues and the
attestations that let you verify a binary's provenance:
<https://github.com/tomtom215/mcp-conformance>. MIT licensed.
