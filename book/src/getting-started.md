<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Getting started

Three steps: install the two tools, record a session of your server or client, and
judge the recording. Nothing here depends on the language or SDK your implementation
uses — the tools read and write bytes on the wire.

## Install

```text
cargo install --locked --git https://github.com/tomtom215/mcp-conformance mcp-trace-capture mcp-trace-validator
```

That builds the current source, which is what this book describes. From the `0.6.0`
release on, `cargo install mcp-trace-capture mcp-trace-validator` installs the
published crates, and `cargo binstall mcp-trace-capture mcp-trace-validator` fetches
prebuilt binaries instead of compiling (Linux x86_64 and aarch64, macOS, Windows
x86_64). Until then crates.io serves `0.5.1`, which judges every trace against
`2025-11-25` and has no capture tool.

## Record a session

`mcp-trace-capture` sits between a client and a server, forwards every byte unchanged,
and writes what crossed the wire to a trace file.

**A stdio server.** Have your client launch the wrapper, with the server's own command
after `--`:

```text
mcp-trace-capture -o session.jsonl stdio -- python my_server.py
```

When a client launches it from its configuration file, give `-o` an absolute path and
add `--force` (clients relaunch servers, and the wrapper otherwise refuses to overwrite
a trace). [The capture tool's README](https://github.com/tomtom215/mcp-conformance/tree/main/crates/mcp-trace-capture#in-a-clients-configuration)
shows the configuration for desktop clients and for the Python and TypeScript SDKs'
stdio clients.

**A Streamable HTTP server.** Run the proxy in front of it and point the client at the
proxy instead:

```text
mcp-trace-capture -o session.jsonl http --upstream http://localhost:3000
# the client connects to http://127.0.0.1:8080/mcp
```

Stop the proxy with Ctrl-C when the session is over.

Record **one session per trace**: one client connection per capture run. A trace is
judged as one session, so two clients recorded together look like one client breaking
the rules (the validator says so when it sees more than one handshake).

## Judge it

```text
mcp-trace-validator validate session.jsonl
```

For a session that reuses a request id, the report reads:

```text
MCP trace validation — revision 2025-11-25 (declared by the trace)
  FAIL  BASE-003 (MUST NOT)
        seq 5: request "ping" reuses id 2, already used by the same party at seq 3
        spec: "The request ID MUST NOT have been previously used by the requestor within the same session."
        see:  https://modelcontextprotocol.io/specification/2025-11-25/basic#requests
totals: 16 pass, 1 fail, 0 warn, 87 excluded, 0 unsupported, 14 not applicable, 24 not observed
verdict: fail
```

Each finding names the clause (`BASE-003`) and its strength (`MUST NOT`), the event in
the trace (`seq 5` — every event carries a `seq`, its position in the recording), what
was wrong, the sentence of the specification it breaks, and a link to it. The exit
status is the verdict: `0` pass, `1` a clause failed, `2` the run could not judge (a
bad invocation, or a trace with nothing in it), `3` the trace is malformed.

## Reading a report

The report lists what needs attention; `--all` lists every clause. The totals always
count every clause the revision has, in seven outcomes:

| Outcome | Meaning |
|---------|---------|
| `pass` | The session carried traffic the clause binds to, and every check of it found nothing wrong. |
| `fail` | A MUST or MUST NOT clause was broken. Each finding says where. |
| `warn` | A SHOULD or SHOULD NOT clause was not followed. Warnings do not fail the run unless you pass `--strict`. |
| `not observed` | The session never did the thing the clause is about (nothing was paginated, no error was returned). Not a pass: there was no opportunity to break it. |
| `not applicable` | The clause applies only after a capability was negotiated, and this session did not negotiate it. |
| `excluded` | The clause cannot be judged from a recording at all — it is about an implementation's internals, or its user interface. The registry gives the reason for each; `--all` prints it. |
| `unsupported` | This build cannot judge a clause its registry names. It means a mismatched installation, and exits `2`. |

So a pass verdict says *nothing this session did broke a clause*. How much it
covers is in the totals: a short session observes few clauses, and a richer session
(more features, errors, pagination, both transports) puts more of the specification
to the test.

**Which revision.** The validator judges the revision the session declares — in
`initialize`, in each request's `_meta`, or in the `MCP-Protocol-Version` header —
and says how it chose: `(declared by the trace)`. `--revision 2025-11-25` overrides
it; naming two (`--revision 2025-11-25 --revision 2026-07-28`) judges the session
under both and shows where they differ. This build knows `2025-11-25` and
`2026-07-28`. A session of an older revision, such as `2025-06-18`, is refused with an
explanation rather than judged against rules it was not playing by.

**Several traces.** `mcp-trace-validator validate traces/*.jsonl` judges each, prints a
section per trace and a closing tally, and exits with the worst trace's status.

**Other formats.** `--format json` is the whole report as data, described by a
[JSON Schema](https://github.com/tomtom215/mcp-conformance/blob/main/crates/mcp-trace-validator/schema/report.schema.json);
`--format junit` is a test report for CI; `--format sarif` is a SARIF 2.1.0 log for
code scanning. [Using it in CI](ci.md) covers all three.
