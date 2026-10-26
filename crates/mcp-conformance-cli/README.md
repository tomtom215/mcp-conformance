<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

Run the official [Model Context Protocol](https://modelcontextprotocol.io) conformance
suite against your server or client *through* a recording tap, and get two verdicts on
the same traffic: the suite's checks, and
[`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator)'s clause-by-clause
report of every session the suite drove — with whether the two agree. Or keep it open
while you work: `watch` records a session as it happens and prints each finding as the
exchange that shows it completes.

> **Not yet released.** This crate is on `main` only
> ([ADR-0021](../../docs/plan/decisions/0021-a-workbench-people-keep-open.md), phases W1,
> W4 and W5);
> build it from the repository: `cargo install --path crates/mcp-conformance-cli`.

## What it needs

For `test`: Node.js 18 or newer, with `npx`, on `PATH`. The official suite is a Node package; `test`
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

## Watching a session

`watch` is `mcp-trace-capture` with a judge attached. It takes the recorder's two modes
and its flags, by the same names, and records the same trace file: configure your client
to launch the wrapper in place of the server,

```text
mcp-conformance watch stdio -- python my_server.py
```

or point it at the proxy in front of a streamable-HTTP server:

```text
mcp-conformance watch http --upstream http://localhost:3000/mcp --listen 127.0.0.1:8080
```

After every completed exchange — a response paired with its request, or a notification —
the trace so far is judged as `mcp-trace-validator validate` would judge it, and each
**new** `fail` or `warn` finding is printed on stderr with its clause ID, level, `seq`,
detail and the quoted clause, under a one-line running tally. At the end comes the
validator's report of the whole trace. Only findings are printed: a clause going from
`not observed` to `pass` is not one. A check that concludes from what the rest of the
trace does not hold (`LIFE-003`, an `initialized` notification that has not come *yet*)
waits for the end, and a finding the whole trace no longer shows is named as withdrawn.
Text quoted from the session is printed with control characters escaped. The exit code
is the recorder's: a stdio client still sees its server's.

Judging never touches the session. The recorder hands each line to a channel and goes
on; a thread of its own judges. Each pass re-judges the trace so far, so its cost grows
with the session — measured with `cargo bench -p mcp-conformance-cli --bench live`
(release profile; 4 vCPU Intel Xeon @ 2.10 GHz; criterion's 95% interval of the mean):

| Session | One pass |
|---------|----------|
| the largest committed capture, 41 events | 158–176 µs |
| synthetic, 10,001 messages | 74–86 ms |

Every exchange of a short session gets its own pass. To keep a long one from taking a
core, the next pass waits three times as long as the last one took, and the lines that
arrive meanwhile are judged together — a pass about every 320 ms at 10,000 messages, so
findings there lag by up to that much; the session never waits.

### Exporting spans

```text
mcp-conformance watch --otlp http://localhost:4318 stdio -- python my_server.py
```

`--otlp` sends one span per exchange to an OpenTelemetry collector over OTLP/HTTP with JSON
encoding (`POST /v1/traces`), named and attributed by the OpenTelemetry semantic
conventions for MCP (`mcp.method.name`, `jsonrpc.request.id`, `mcp.session.id`,
`mcp.protocol.version`, `gen_ai.tool.name`, `error.type`, …; span name
`{mcp.method.name} {target}`), each finding a span event carrying the requirement ID,
level and spec URL. Every span is `CLIENT`: a tap sees the request leave and the answer
return, the initiator's view, never the receiver's processing. Spans go in batches off
the session's path through a bounded queue; a collector that is down or slow costs only
the spans it was sent, and the summary at the end counts them. CI runs this against a
pinned OpenTelemetry Collector.

What a span carries is exactly this, and nothing else from a message: the method; the
request id; the session id and the protocol version; the transport, and for HTTP the
upstream's host and port; the tool or prompt name; the resource URI **up to its path** —
its userinfo, query and fragment are removed, because a presigned or tokenised URI
carries its credential there; on a JSON-RPC error, the numeric code and a fixed name for
it (`JSON-RPC error -32602 (invalid params)`), **never the server's message**, which is
free text; and, per finding, the requirement ID, level, outcome, revision, check, spec
URL, `seq` and detail. A finding's detail quotes the value it is about — the tool name
that broke `TOOL-006`, say — and is cut at 512 characters, with the cut marked. No
arguments, no results, no `_meta`, no headers, no error message.

| Exit code (`watch`) | Meaning |
|-----------|---------|
| server's | `stdio`: the wrapped server's own exit code, passed through |
| `0` | `http`: stopped with a complete trace |
| `2` | Bad arguments (an `--otlp` URL included), or a run that could not start |
| `3` | The session ran but its trace is incomplete (a write failed) |

Findings do not change the exit code — judge the trace with `mcp-trace-validator validate`
for that. The full table is `mcp-trace-capture`'s.

## What it runs, and what it does not

`test` runs your server or client, Node, and the pinned suite package npx fetches from
the npm registry. Everything it starts listens on `127.0.0.1` only, for the length of the
run. The stdio bridge answers only requests whose `Host` is its own loopback address and
whose `Origin`, if any, is loopback. Recordings hold message bodies in full: read a
trace before you share it ([the security model](../../docs/plan/05-security-model.md)).

`watch` runs what you give it (the stdio server's command, without a shell) and listens
only where `mcp-trace-capture` would: nowhere in `stdio` mode, and on `--listen` in
`http` mode (`127.0.0.1:8080` by default, with the recorder's warning for any other
address). It sends nothing anywhere unless `--otlp` names a collector, and then only the
span fields [listed above](#exporting-spans) — method, tool and prompt names, resource
URIs up to their path, request and session IDs, error codes (not messages), findings —
never a payload; an `http://` collector that is not this machine is warned about, since
those fields would cross the network unencrypted.

## The workbench

```text
mcp-conformance serve --library ./recordings --open
```

A local web page over a directory of recordings (traces, each with an optional
`<id>.meta.json` sidecar of tags, provenance and a cached verdict): search with a small
query language (`method:tools/call req:BASE-003 code:-32602 timeout`), each session as a
timeline with its findings at their events and an inspector, two sessions compared, a
trace that is still being written followed live, dashboards of verdicts over time, clause
coverage and findings, and administration — import (traces, `mcpsnoop` captures, MCP
Inspector exports, each with what its source does not record), tags, retention, delete,
export, and redacted copies. It listens on `127.0.0.1` and prints a link with a single-use
token; it sends nothing anywhere. [The workbench](../../book/src/workbench.md) chapter has
the details. An export names each recording once and holds at most 100 of them and
192 MiB of traces, since the archive is built in memory; and the page's JSON API answers
only the page itself and a link you open — browsers count every port of `127.0.0.1` as
one site and send the cookie to all of them, so a request made by a page on another local
port (`Sec-Fetch-Site: same-site`) is refused.

Two costs the workbench cannot design away, so it states them. **The cookie goes to every
local server.** Browsers scope a cookie to the host, never to the port: any other server on
`127.0.0.1` that the same browser visits while the workbench runs is sent its session
cookie (`HttpOnly`, so no page can read it — but that server receives it), and the startup
banner says so. **A large trace line is judged in memory, at a multiple of its size that
depends on its shape.** Measured on one five-event trace whose fifth line was 8 MiB (debug
build, peak resident set from `wait4`'s `ru_maxrss`, the binary's own 30–37 MiB baseline
subtracted): a line that is one 8 MiB string cost `mcp-trace-validator validate` about
1× its bytes and the workbench's loader (`library list`, which parses, judges and caches
the entry) about 3×; a line of 8 MiB of small JSON values — some 330,000 one-character
text blocks — cost them 41× and 72× (360 MiB and 611 MiB), because each JSON value is
parsed into a tree node and held for the whole judgment. The validator's
`--max-line-bytes` (65 MiB by default) is the only bound on a line, so a hostile recording
at that limit can cost the workbench several gigabytes: it is a library you administer,
not an upload surface, and its import body limit (192 MiB) is not a memory bound.

The same library from the command line, for scripts (`stats --format json` prints
exactly what the dashboards read):

```text
mcp-conformance library --library ./recordings stats --format json
mcp-conformance library --library ./recordings import capture.jsonl --from mcpsnoop
mcp-conformance library --library ./recordings redact ID --meta --pointer /params/arguments/apiKey
mcp-conformance library --library ./recordings prune --retain-days 30 --apply
```

`serve` and `library` exit `0` when they did what was asked and `2` when they could not
(a bad address, an unreadable library, an unknown entry, a rule that cannot apply). The
page and its JSON API are not a stable interface; `library stats --format json` is the
way to read the numbers from a script.

## The library

The crate's library is the small part `mcp-conformance test` shares with this
repository's own gate, `cargo xtask conformance`: the suite pin (`suite`) and the
agreement policy (`agreement`); `watch`'s live judge (`live`) and span encoding
(`otlp`); and the importers the workbench uses (`import`), which `cargo xtask
mcpsnoop`'s tests also exercise. With `default-features = false` it depends on the
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
