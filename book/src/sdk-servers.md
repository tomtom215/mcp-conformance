<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Recording an SDK's server

[Capturing from your client](clients.md) records a session your own MCP host
drives. This chapter is the other way round: you have a **server** built on an
official SDK and want to know what it puts on the wire. It is worked through on
the TypeScript (both of its lines) and Python SDKs' own conformance servers —
the servers each SDK runs the official suite against — but nothing here is
specific to them: any Streamable HTTP server can stand in.

The commands are not copies. Each block below is included, by mdBook anchor,
from [`interop/matrix.sh`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/matrix.sh),
the script the weekly scheduled job runs to keep the
[cross-SDK matrix](sdk-matrix.md) true; `cargo xtask sdk-matrix --check` fails
if this chapter stops including them, or the job stops running the script. What
you read is what the job executed.

## Prerequisites

Build the recorder and the validator, then install the pinned SDKs:

```bash
cargo build -p mcp-trace-capture -p mcp-trace-validator
cd interop
./setup.sh
```

`setup.sh` installs each SDK at the version
[`interop/sdk-pins.env`](https://github.com/tomtom215/mcp-conformance/blob/main/interop/sdk-pins.env)
names, every one with integrity: npm from `package-lock.json` (`npm ci`),
Python from the hash-locked `requirements.txt` (`pip --require-hashes`), Go
from `go.sum`, Rust from `Cargo.lock`. A conformance server that lives in an
SDK's repository rather than its package is fetched from the exact commit the
pins name and refused unless its SHA-256 matches. It needs node 20+, Python 3.11+,
go, cargo and git.

## Recording one session

Every session is the same four steps — start the server, put
`mcp-trace-capture`'s proxy in front of it, drive one session through the
proxy, stop the proxy so it writes the trace — followed by judging it:

```bash
{{#include ../../interop/matrix.sh:record}}
```

The client is chosen by revision, and is the same for every server: the
TypeScript SDK's v1 client (`ts-client.mjs`) speaks `2025-11-25`, the Python
SDK's (`py-client.py`) speaks `2026-07-28`. Each lists what the server
advertises, calls a tool, calls one that does not exist, reads a resource and
gets a prompt. With `INTEROP_ERRORS=record` an error answer is logged and the
session carries on: the server's answer is the evidence, so the client must not
stop at it.

## The TypeScript SDK, v1

The v1 line is the single package `@modelcontextprotocol/sdk`, in maintenance.
The server is `test/conformance/src/everythingServer.ts` from the SDK's
repository at the release tag. It imports the SDK by its path inside that
repository; `setup.sh` rewrites those imports to the published package, so it
runs on the pinned npm release, and `tsx` runs the TypeScript directly:

```bash
{{#include ../../interop/matrix.sh:typescript}}
```

The second line is a session the server cannot serve: the TypeScript SDK's 1.x
line implements the specification up to `2025-11-25` (its `SUPPORTED_PROTOCOL_VERSIONS`
stops there), so the Python client's discovery falls back to the `initialize`
handshake. `py-client.py` exits 3 when that happens, and the script files the
recording as `…not-served.jsonl` rather than judging a session nobody asked for.

## The TypeScript SDK, v2

The v2 line is a set of packages, `@modelcontextprotocol/server` and its
companions, and it does serve `2026-07-28`: its server answers
`server/discover` with `supportedVersions` of `["2026-07-28"]`, and still answers
the `initialize` handshake for `2025-11-25`, from one process. The server is the
same path in the SDK's repository, `test/conformance/src/everythingServer.ts`,
at the release tag. Nothing is rewritten: the file imports the published packages by name, which
`setup.sh` installs from the lockfile (`@modelcontextprotocol/server`, `node` and
`express`, pinned together in `sdk-pins.env`). Both revisions are recorded:

```bash
{{#include ../../interop/matrix.sh:typescript-v2}}
```

## The Python SDK

The server is the SDK repository's `examples/servers/everything-server`, run on
the hash-locked `mcp` release:

```bash
{{#include ../../interop/matrix.sh:python}}
```

## Judging, and what comes out

```bash
./matrix.sh ../target/debug
```

prints one line per session — the verdict and the totals — and leaves each
trace and its JSON report in `interop/out-matrix/`. Read any one in full the
usual way:

```bash
../target/debug/mcp-trace-validator validate out-matrix/sdk-python-2026-07-28.jsonl
```

A finding is a result, not a failure of the script: it exits non-zero only when
a session could not be recorded at all. The same script records the Go and Rust
SDKs' servers, and the committed results of all of them are the
[cross-SDK matrix](sdk-matrix.md). To check a fresh run against them:

```bash
cargo xtask sdk-matrix --recaptured interop/out-matrix
```

## Recording your own server

Replace the server command in a `record` line with yours, and give it a free
port. If your server speaks stdio rather than HTTP, record it the way
[Getting started](getting-started.md) shows instead —
`mcp-trace-capture -o session.jsonl stdio -- <server>` — and point a client at
that command.
