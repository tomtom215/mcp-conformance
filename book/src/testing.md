<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Testing a server or client

A recording judges only what its session did; most clauses read `not observed` because
nothing exercised them. The official conformance suite exercises an implementation on
purpose. `mcp-conformance test` runs the two together: the pinned suite drives your
server or client, `mcp-trace-capture` records the traffic as it goes, and every recorded
session is judged clause by clause. You get the suite's checks, the clause report, and
whether the two agree — from one command.

> `mcp-conformance` is on `main`, not yet released
> ([ADR-0021](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0021-a-workbench-people-keep-open.md)).
> Build it from a checkout with `cargo install --path crates/mcp-conformance`.

## Before you start

`test` needs Node.js 18 or newer, with `npx`, on `PATH`: the suite is a Node package.
It checks, and tells you what to install when Node is missing; it installs nothing
itself. npx fetches `@modelcontextprotocol/conformance` at exactly the version this build
pins, and `test` refuses to go on if the suite reports any other version.

## A server

Over streamable HTTP, give the endpoint URL. The suite reaches it through a recording
proxy on `127.0.0.1`:

```text
mcp-conformance test --url http://localhost:3000/mcp
```

Over stdio, give the command. It is split at whitespace and run without a shell:

```text
mcp-conformance test --command "python my_server.py"
```

The suite drives servers only over streamable HTTP, so `test` puts a loopback bridge in
front of a stdio server and starts one server process per suite session, each under the
recorder's stdio wrapper. The trace is the stdio wire your server spoke. Three of the
suite's scenarios exercise the HTTP transport itself — `dns-rebinding-protection`,
`server-sse-polling` and `server-sse-multiple-streams` — and would judge the bridge rather
than your server, so they are not run; the report lists them as not run, with why.

To run some scenarios rather than the suite's default set, name each:

```text
mcp-conformance test --url http://localhost:3000/mcp --scenario tools-list --scenario ping
```

## A client

Give the command that starts your client. The suite runs it through a shell, once per
scenario, with the URL of the suite's test server appended:

```text
mcp-conformance test --client-command "node my_client.js"
```

By default that is the suite's four protocol scenarios (`initialize`, `tools_call`,
`elicitation-sep1034-client-defaults`, `sse-retry`). The `auth/*` scenarios need the
suite's authorization server as well as its MCP one, which the recording proxy does not
stand in front of; name them with `--scenario` to run them anyway.

## Reading the result

The human report has three parts:

- **The official suite** — each scenario with its checks' statuses, and the message of
  any that failed or warned. The suite's verdict is its own: a `FAILURE` or a `WARNING`
  fails it.
- **The clause report** — one validator report per recorded session, as
  `mcp-trace-validator validate` prints it ([Reading a report](reading-a-report.md)). A
  session that holds nothing to judge (a request the server refused before any message
  was exchanged, say) is listed as not judged, with the reason.
- **Agreement** — every failing clause is a divergence from the suite's verdict unless a
  divergence baseline explains it, and every baseline entry that explained nothing is
  stale. Agreement holds when there is neither.

`--divergences` names a baseline in the format of this repository's
`conformance/agreement-divergences.json`: each entry a requirement ID, a triage class
(`our-bug`, `suite-bug` or `spec-ambiguity`) and an upstream link. `--expected-failures`
passes the suite its own baseline of scenarios expected to fail.

```text
mcp-conformance test --url http://localhost:3000/mcp --divergences divergences.json --expected-failures expected-failures.yaml
```

For CI, `--format json` carries all three parts; `junit` and `sarif` are the validator's
own renderings of the clause report ([Using it in CI](ci.md)), and the suite's checks and
the agreement go to stderr whatever the format. `-o` writes the report to a file:

```text
mcp-conformance test --command "python my_server.py" --format sarif -o conformance.sarif
```

The exit code is `0` when the suite passed and the two agree, `1` for findings — a suite
check that failed or warned, or a disagreement — and `2` when the run could not be made
or judged. The suite's results and every trace are kept in the work directory, printed at
the end (`--work-dir` chooses it): judge a trace again, or attach it to a bug report.

## This repository's own subjects

CI runs `test` against this repository's everything server, over both transports, and
its reference host, and holds each report to `cargo xtask conformance`'s results for the
same subject: the same scenarios with the same check statuses, every failing clause the
gate's recordings show, and nothing beyond them that `conformance/wire-findings.json`
does not list with why only the wire shows it. From a checkout, after
`cargo build -p mcp-everything-server -p mcp-conformance` and
`cargo build -p mcp-reference-host --features cli`:

```text
mcp-conformance test --command "target/debug/mcp-everything-server --transport stdio"
mcp-conformance test --client-command target/debug/mcp-reference-host --divergences conformance/client-agreement-divergences.json
```
