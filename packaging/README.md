<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

Clause-by-clause conformance checking for the
[Model Context Protocol](https://modelcontextprotocol.io): point one command at an
MCP server and find out which of the specification's requirements it met, which it
broke, where, and which it never exercised. Any language, any SDK, stdio or
Streamable HTTP.

```text
npx mcp-conformance check -- python my_server.py           # a stdio server
npx mcp-conformance check --url http://localhost:3000/mcp   # a Streamable HTTP server
pip install mcp-conformance                                 # the same binaries, from PyPI
```

`check` exercises the server as a conforming client would (read-only by default),
records the session and prints the report; it needs no client, no SDK and no
Node.js beyond the `npx` that fetched it.

This package installs the project's release binaries for your platform — the
same files, byte for byte, that the GitHub release attaches and attests:

- `mcp-conformance` is the front door: `check` (above), `watch` (findings as a
  session runs), `test` (the official conformance suite through the recorder, with
  both verdicts), `validate`, and `serve` (a local workbench over recordings);
- `mcp-trace-capture` records a session: a transparent stdio wrapper and HTTP
  reverse proxy that forward bytes unchanged;
- `mcp-trace-validator` judges a recorded trace against the specification.

```text
mcp-trace-capture -o session.jsonl stdio -- python my_server.py
mcp-trace-validator validate session.jsonl
```

Prebuilt for Linux (x86_64, aarch64; static, any distribution), macOS (x86_64,
arm64) and Windows (x86_64). Elsewhere, build from source:
`cargo install --locked --git https://github.com/tomtom215/mcp-conformance mcp-conformance-cli`
for `mcp-conformance`, and `cargo install mcp-trace-capture mcp-trace-validator` for
the other two.

Documentation: <https://mcp-conformance.com>. Source, issues and the
attestations that let you verify a binary's provenance:
<https://github.com/tomtom215/mcp-conformance>. MIT licensed.
