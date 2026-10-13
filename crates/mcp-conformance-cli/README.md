<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

Run the official [Model Context Protocol](https://modelcontextprotocol.io) conformance
suite against your server or client *through* a recording tap, and get two verdicts on
the same traffic: the suite's checks, and
[`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator)'s clause-by-clause
report of every session the suite drove — with whether the two agree.

> **Not yet released.** This crate is on `main` only
> ([ADR-0021](../../docs/plan/decisions/0021-a-workbench-people-keep-open.md), phase W1);
> build it from the repository: `cargo install --path crates/mcp-conformance-cli`.

## What it needs

Node.js 18 or newer, with `npx`, on `PATH`. The official suite is a Node package; `test`
checks for Node, says what to install when it is missing, and never installs anything
itself. npx fetches the suite at exactly the version this build pins
(`@modelcontextprotocol/conformance` `0.1.16`), and `test` reads the suite's own
`--version` back before running it: a different version is refused, never used.

## Testing a server

A streamable-HTTP server, at its endpoint URL:

```text
mcp-conformance test --url http://localhost:3000/mcp
```

A stdio server, by its command (split at whitespace, run without a shell):

```text
mcp-conformance test --command "python my_server.py"
```

The suite drives servers only over streamable HTTP, so for a stdio server `test` puts a
loopback bridge in front of it: one server process per suite session, each under the
recorder's stdio wrapper, so the trace records the stdio wire the server actually spoke.
The suite's three HTTP-transport scenarios (`dns-rebinding-protection`,
`server-sse-polling`, `server-sse-multiple-streams`) would judge the bridge rather than
the server, so they are not run, and the report says so.

## Testing a client

```text
mcp-conformance test --client-command "node my_client.js"
```

The suite runs the command once per scenario — by default its four protocol scenarios
(`initialize`, `tools_call`, `elicitation-sep1034-client-defaults`, `sse-retry`) — through
a shell, with its test server's URL appended; `test` puts the recorder's HTTP proxy
between the two. Name other scenarios with `--scenario`, repeatable.

## The report

```text
mcp-conformance test --url http://localhost:3000/mcp --format json -o report.json
```

`--format human` (the default) and `json` carry three parts: the suite's checks by
scenario; the clause report of each recorded session, as `mcp-trace-validator` renders
it; and the **agreement** — every failing clause either explained by a divergence
baseline (`--divergences`, the format of this repository's
[`agreement-divergences.json`](../../conformance/agreement-divergences.json)) or listed
as an unexplained divergence, and every baseline entry that explained nothing listed as
stale. `junit` and `sarif` are the validator's own renderings of the clause report, for
CI ingestion; the suite's checks and the agreement go to stderr in every format. The
suite's results and the traces are kept in the work directory (`--work-dir`, or a new
one under the system's temporary directory), whose path is printed.

| Exit code | Meaning |
|-----------|---------|
| `0` | The suite passed, and the validator agrees: no failing clause the baseline leaves unexplained, no stale baseline entry |
| `1` | Findings: a suite check failed or warned, or the two verdicts disagree |
| `2` | The run could not be made or judged: bad arguments, Node.js or npx missing, the suite did not run or reported another version, no recorded session judgeable, the report not written |

## What it runs, and what it does not

`test` runs your server or client, Node, and the pinned suite package npx fetches from
the npm registry. Everything it starts listens on `127.0.0.1` only, for the length of the
run. The stdio bridge answers only requests whose `Host` is its own loopback address and
whose `Origin`, if any, is loopback. Recordings hold message bodies in full: read a
trace before you share it ([the security model](../../docs/plan/05-security-model.md)).

## The library

The crate's library is the small part `mcp-conformance test` shares with this
repository's own gate, `cargo xtask conformance`: the suite pin (`suite`) and the
agreement policy (`agreement`). With `default-features = false` it depends on the
validator and the serde family only.

```rust
use mcp_conformance_cli::agreement::{Baseline, ValidatorFailure, reconcile};

let failures = [ValidatorFailure::new("001.jsonl", "BASE-003", "a request id reused")];
let reconciliation = reconcile(&Baseline::default(), &failures);
assert!(!reconciliation.holds(), "an unexplained failure is a divergence");
assert_eq!(mcp_conformance_cli::suite::VERSION, "0.1.16");
```

## License

MIT — see [LICENSE](LICENSE).
