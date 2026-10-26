<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Cross-SDK interop check and matrix

Real sessions between the official MCP SDKs, recorded through
`mcp-trace-capture` and judged by `mcp-trace-validator`. Nothing from this
workspace speaks MCP here except the recorder and the judge, so a pass is
evidence about independent traffic, not about this project's own server.

Two scripts, two jobs:

- **`run.sh`** — the per-push check (CI's `interop` job). Every session must
  judge `pass`; a regression in the validator's reading of independent traffic
  fails the build.
- **`matrix.sh`** — the cross-SDK matrix (the weekly scheduled `sdk-matrix`
  job). Each Tier 1 SDK's own conformance server, at each revision it serves,
  recorded and judged. Findings are results here, not failures: the committed
  reports are the [matrix page](../book/src/sdk-matrix.md), and the job fails
  when a fresh run stops agreeing with them.

## `run.sh`

| Session | Client | Server | Transport | Expected |
|---|---|---|---|---|
| `ts-stdio` | TypeScript SDK 1.32.1 | `@modelcontextprotocol/server-everything` 2026.8.31 | stdio | `pass-with-warnings` at `2025-11-25`: only `BASE-086`, the server's draft-07 tool schemas |
| `ts-http` | TypeScript SDK 1.32.1 | the same, `streamableHttp` | HTTP proxy | `pass-with-warnings` at `2025-11-25`: only `BASE-086` |
| `py-stdio` | Python SDK (`mcp` 2.3.0) | [`py-server.py`](py-server.py) | stdio | `pass` at `2026-07-28` |
| `py-http` | Python SDK | `py-server.py http` | HTTP proxy | `pass` at `2026-07-28` |
| `ts-2025-06-18` | TypeScript SDK 1.13.0 | the everything server | stdio | refused, exit 2 (no `2025-06-18` registry) |

## `matrix.sh`

| SDK | Release | Its conformance server | Revisions |
|---|---|---|---|
| TypeScript v1 | `@modelcontextprotocol/sdk` 1.32.1 | `test/conformance/src/everythingServer.ts` at the release tag, imports pointed at the npm package | `2025-11-25` (the 1.x line does not serve `2026-07-28`) |
| TypeScript v2 | `@modelcontextprotocol/server` 2.3.1 (with `node` 2.1.1 and `express` 2.0.2) | `test/conformance/src/everythingServer.ts` at the release tag, unmodified: it imports the published packages by name | both, from one process |
| Python | `mcp` 2.3.0 | `examples/servers/everything-server` at the release tag | both |
| Go | `go-sdk` v1.8.0 | `conformance/everything-server`, shipped in the module | both |
| Rust | `rmcp` 3.7.0 | `conformance/src/bin/server.rs` at the release tag ([`rust/`](rust/Cargo.toml) builds it against the crates.io release) | both |

Every server is driven over Streamable HTTP through the capture proxy, by
[`ts-client.mjs`](ts-client.mjs) (the TypeScript v1 SDK's client) at
`2025-11-25` and by [`py-client.py`](py-client.py) at `2026-07-28` — one client
per revision, so two rows of a revision differ by the server alone. The book's
[Recording an SDK's server](../book/src/sdk-servers.md) walks through the
TypeScript (both lines) and Python sessions, quoting this script.

## Pins

[`sdk-pins.env`](sdk-pins.env) is the one place the SDK versions are
stated, with the upstream commit and SHA-256 of every file taken from an SDK's
repository. Each package manager keeps its own lockfile, and both `setup.sh`
and `cargo xtask sdk-matrix --check` (a per-change gate) refuse a lockfile
that disagrees with `sdk-pins.env` — so a dependency bot's SDK bump is red on
its own pull request, where the commit, the file hashes and the recorded
matrix have to move with it:

| SDK | Pinned by |
|---|---|
| TypeScript, both lines | `package.json` + `package-lock.json` (`npm ci`): `@modelcontextprotocol/sdk` for v1; `@modelcontextprotocol/server`, `node` and `express` for v2, which move together because the server file is taken from one commit |
| Python | `requirements.in` + hash-locked `requirements.txt` (`pip --require-hashes`) |
| Go | [`go/go.mod`](go/go.mod) + `go/go.sum` |
| Rust | [`rust/Cargo.toml`](rust/Cargo.toml) + `rust/Cargo.lock` (`cargo --locked`) |

A release is pinned once it has been public for a week, as the `cooldown` in
[`.github/dependabot.yml`](../.github/dependabot.yml) has it, and that goes for
the transitive packages a lockfile resolves too. The one exception is the Rust
leg: `rmcp` 3.7.0 (published 2026-10-22) is what this workspace builds against,
and the 3.5.x it replaces is inside the range of GHSA-4432-vg92-fqh6, fixed in
3.6.0 (register row 3.1).

## Running it

```bash
cargo build -p mcp-trace-capture -p mcp-trace-validator
cd interop
./setup.sh                 # every SDK, every conformance server
./run.sh                   # the per-push check
./matrix.sh                # the matrix; traces and reports in out-matrix/
cd .. && cargo xtask sdk-matrix --recaptured interop/out-matrix
```

`setup.sh` needs node 20+ (the TypeScript v2 packages declare
`engines.node >=20`), Python 3.11+ (`PYTHON` selects it), go 1.21+ (it
fetches the Go 1.25 toolchain `go/go.mod` asks for), cargo and git. `run.sh`
needs only the npm and Python halves (`npm ci`, then a venv with
`pip install --require-hashes -r requirements.txt`, as CI's `interop` job
does), and takes `PYTHON=.venv/bin/python`.

The scripts double as worked examples of the two ways to record a session:
launch the server through `mcp-trace-capture … stdio -- <server>`, or point
the client at `mcp-trace-capture … http --upstream <server-url>`.
