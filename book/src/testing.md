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
> Build it from a checkout with `cargo install --path crates/mcp-conformance-cli`.

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
  `mcp-trace-validator validate` prints it ([Reading a report](reading-a-report.md)),
  viewed for your end of the session ([below](#whose-finding-it-is)). A session that
  holds nothing to judge (a request the server refused before any message was exchanged,
  say) is listed as not judged (`informational`), with the reason, and the run loses
  nothing. One the run recorded and could not judge is listed as an `error`: a trace that
  is malformed or unreadable, or a session whose server negotiated a revision this build
  has no registry for. That makes the run's verdict `incomplete` and its exit code `2`,
  whatever the suite and the sessions it did judge say, unless you name the session with
  `--expected-unjudged` (its name under the trace directory, or a directory's; naming one
  that did not go unjudged for that reason is an error too, so an expectation cannot
  outlive its cause).
- **Agreement** — every failing clause held against your end is a divergence from the
  suite's verdict unless a divergence baseline explains it, and every baseline entry that
  explained nothing is stale. Agreement holds when there is neither.

### Whose finding it is

The suite drives a server with a client of its own, and a client with a test server of
its own, and that end breaks clauses too. Its client calls `logging/setLevel` on a server
that never declared `logging`, uses `completion/complete` or `resources/subscribe` on one
that declared neither, and closes a session without `notifications/initialized`; its test
server answers a notification with HTTP 200 and a body. Each is a real finding about the
traffic, and none is yours to fix.

So every finding names the party it is held against, as `[client]` or `[server]` in the
report (`[unattributed]` when the trace cannot say), and `test` judges your end: a server
(`--url`, `--command`) by the findings held against the server, a client
(`--client-command`) by those held against the client. A finding held against nobody
counts, as `mcp-trace-validator validate --party` counts it, and is tagged so you can see
it was counted rather than attributed. The other end's findings are listed — under
"failing clauses held against the suite's own client" (or "test server") — and **set
aside**: they move neither a session's verdict nor the agreement, and the clause rows of
the session still show them failing, because that is what the traffic did. A divergence
baseline explains a failure of either end, and an entry stays live while it explains any,
so one baseline file serves `test` and `cargo xtask conformance`'s gate, which counts both
ends. In the JSON report, `agreement.party` and `sessions.party` name your end,
`agreement.divergences.set_aside` lists what was set aside (absent when nothing was),
and `sessions.findings` counts the findings counted, set aside and unattributed.

### A server that is not the suite's own

The pinned suite asks a server for tools, prompts and resources by fixed names
(`test_simple_text` is one). A server that defines its own fails the scenarios that look
for those, and the suite's message reads like a broken server (`Unknown prompt:
test_simple_prompt`). When the suite fails scenarios and the recorded traffic holds
requests for names that none of the server's own list answers in the run include, the
human report says so after the suite's verdict:

```text
note: 18 of the suite's 28 scenario(s) failed. In 21 of the 28 recorded sessions the suite asked for a tool, prompt or resource that none of this server's own list answers in this run includes:
  tools (12): test_audio_content, test_elicitation, test_elicitation_sep1034_defaults, test_elicitation_sep1330_enums and 8 more
  prompts (4): test_prompt_with_arguments, test_prompt_with_embedded_resource, test_prompt_with_image, test_simple_prompt
  resources (4): test://static-binary, test://static-text, test://template/123/data, test://watched-resource
```

The JSON report has every name, with the counts, under `suite.fixtures` (`null` when there
is no note). The note says what the requests show and no more: the recording does not say
which scenario's failure follows from a missing name — a scenario can pass on a server
that answers the missing tool with an error — so read each scenario's own message beside
it. A kind of fixture the run never saw the server list, or whose list was paged, is not
assessed, since the traffic cannot then show a name missing.

`--divergences` names a baseline in the format of this repository's
`conformance/agreement-divergences.json`: each entry a requirement ID, a triage class
(`our-bug`, `suite-bug` or `spec-ambiguity`) and an upstream link. `--expected-failures`
passes the suite its own baseline of scenarios expected to fail.

```text
mcp-conformance test --url http://localhost:3000/mcp --divergences divergences.json --expected-failures expected-failures.yaml
```

For CI, `--format json` carries all three parts; `junit` and `sarif` are the validator's
own renderings of the clause report ([Using it in CI](ci.md)), viewed for your end like
the human report (a failure set aside is a passing case in JUnit and a `note` in SARIF).
A human or JSON report on stdout holds the suite's checks and the agreement itself, so
stderr then says the verdicts and nothing the report repeats; a report in a file, and
`junit` or `sarif`, which carry only the clause report, leave the suite's checks and the
agreement on stderr whole. `-o` writes the report to a file:

```text
mcp-conformance test --command "python my_server.py" --format sarif -o conformance.sarif
```

The exit code is `0` when the suite passed and the two agree, `1` for findings — a suite
check that failed or warned, or a disagreement — and `2` when the run could not be made
or judged, including a recorded session it could not judge ([above](#reading-the-result)).
The report's top-level `verdict` is `pass`, `fail` or `incomplete` to match. The suite's results and every trace are kept in the work directory, printed at
the end (`--work-dir` chooses it): judge a trace again, or attach it to a bug report.

## This repository's own subjects

CI runs `test` against this repository's everything server, over both transports, and
its reference host, and holds each report to `cargo xtask conformance`'s results for the
same subject: the same scenarios with the same check statuses, every failing clause the
gate's recordings show, and nothing beyond them that `conformance/wire-findings.json`
does not list with its triage class and why only the wire shows it. From a checkout, after
`cargo build -p mcp-everything-server -p mcp-conformance-cli` and
`cargo build -p mcp-reference-host --features cli`:

```text
mcp-conformance test --command "target/debug/mcp-everything-server --transport stdio"
mcp-conformance test --client-command target/debug/mcp-reference-host --divergences conformance/client-agreement-divergences.json
```
