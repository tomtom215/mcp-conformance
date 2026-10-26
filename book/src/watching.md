<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Watching a session live

`mcp-trace-validator` gives its verdict after the session; `mcp-conformance test` gives
one after the suite has run. While you are building a server or a client you want to
hear about a violation when it happens. `mcp-conformance watch` records a session exactly
as `mcp-trace-capture` does and judges it as it runs: every new finding is printed as the
exchange that shows it completes, a one-line tally keeps count, and the validator's full
report follows when the session ends.

> `mcp-conformance` is on `main`, not yet released
> ([ADR-0021](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0021-a-workbench-people-keep-open.md)).
> Build it from a checkout with `cargo install --path crates/mcp-conformance-cli`.

## Starting it

`watch` takes the recorder's two modes, and its flags by the same names — `-o` for the
trace file (with `{session}` for a file per session), `--force`, `--max-message-bytes`.
For a stdio server, configure your client to launch the wrapper in place of the server,
exactly as [capturing from your client](clients.md) describes for `mcp-trace-capture`:

```text
mcp-conformance watch stdio -- python my_server.py
```

For a streamable-HTTP server, point your client at the proxy instead of the server:

```text
mcp-conformance watch http --upstream http://localhost:3000/mcp
# the client connects to http://127.0.0.1:8080/   (the proxy's root is --upstream itself)
```

The proxy's root path is the `--upstream` URL itself, and any other path is appended to
it. With `--upstream http://localhost:3000` (no path), the client connects to
`http://127.0.0.1:8080/mcp` instead; giving the path in both places reaches `/mcp/mcp`,
which the server answers with a 404.

The proxy listens on `127.0.0.1:8080` unless you say otherwise:

```text
mcp-conformance watch -o traces/{session}.jsonl http --upstream http://localhost:3000/mcp --listen 127.0.0.1:9000
```

The trace file is the one `mcp-trace-capture` would have written, so everything else in
this book applies to it unchanged. Stop an HTTP `watch` with Ctrl-C; a stdio one ends
when your client stops the server.

## What it prints

Everything goes to stderr: in stdio mode stdout carries the session itself. A finding
looks like this — the outcome, the clause ID and its level, the event it points at, the
detail, then the clause and where it is published:

```console
mcp-conformance watch: WARN TOOL-006 (SHOULD) seq 5: tool name "weather lookup,v2!" contains characters outside A-Z, a-z, 0-9, underscore, hyphen, and dot: " ,!"
    spec: "The following SHOULD be the only allowed characters: uppercase and lowercase ASCII letters (A-Z, a-z), digits (0-9), underscore (_), hyphen (-), and dot (.)"
    see:  https://modelcontextprotocol.io/specification/2025-11-25/server/tools#tool-names
mcp-conformance watch: WARN TOOL-007 (SHOULD NOT) seq 5: tool name "weather lookup,v2!" contains characters outside A-Z, a-z, 0-9, underscore, hyphen, and dot: " ,!"
    spec: "Tool names SHOULD NOT contain spaces, commas, or other special characters."
    see:  https://modelcontextprotocol.io/specification/2025-11-25/server/tools#tool-names
mcp-conformance watch: 3 exchange(s); findings so far: 0 fail, 2 warn
```

That is the corpus's `tool-006-name-charset` session replayed through `watch stdio`
(the test that replays it checks this page's lines against what `watch` printed).

On a terminal the tally is one line rewritten in place; in a log it follows each batch of
new findings. When the session ends, `watch` prints `mcp-trace-validator`'s report of the
whole trace — the same report `validate` prints — and the tally once more.

Text quoted from the session (a method name, a tool name, an error message) is printed
with control characters escaped as `\u{…}`, as the validator's own report prints it: a
recording is untrusted input, and an escape sequence in a tool name must not reach your
terminal raw.

## When a finding appears

After every completed exchange — a response paired with its request, a notification, or
anything that is not a request still waiting for its answer, such as a line that is not
JSON-RPC at all — the trace so far is judged at the revision it declares. A finding is
printed the first time a pass shows it, and never again; the end-of-session report lists
it with the rest. A violating response or notification is reported by the pass its own
arrival triggers; a violating request, by the pass of the exchange its response completes
(a request never answered is judged at the end).

Two things are deliberately not findings:

- **An outcome that moves.** As the session reaches a clause it goes from
  `not observed` to `pass`. That is progress, not news, and `watch` says nothing.
- **A verdict about the future.** Right after the `initialize` response, the trace so far
  has no `initialized` notification, so `LIFE-003` fails — until the client sends it a
  moment later. Checks that conclude from what the rest of the trace does *not* hold wait
  for the end of the session. The repository holds this to the corpus: every committed
  trace, replayed line by line, prints nothing live that the whole trace does not show,
  and prints everything else by the pass at or after the line that makes it visible.
  Should a session the corpus does not cover still retract a finding, the end of the run
  names it as **withdrawn**.

## It never slows the session

`watch` is read-only, like the recorder it is built on: bytes are forwarded unchanged,
in order, and nothing waits for judging. The recorder hands each line it writes to a
channel and goes on; judging happens on a thread of its own. If judging falls behind, the
findings lag and the session does not — a test holds this, with a judge that sleeps 50 ms
a pass behind a server sending 200 notifications 2 ms apart.

Each pass re-judges the whole trace so far. Measured in release builds (4 vCPU Intel Xeon
@ 2.10 GHz, `cargo bench -p mcp-conformance-cli --bench live`), a pass takes about
0.17 ms on the largest committed capture (41 events) and about 80 ms on a synthetic
10,001-message session. So `watch` paces itself: after a pass that took `d`, the next
waits `3d`, and whatever arrived meanwhile is judged in one pass. A short session gets a
pass per exchange; a 10,000-message one gets one about every 320 ms, its findings lagging
by up to that much, and judging never takes more than a quarter of a core.

## Spans for your tracing stack

`--otlp` sends one span per exchange to an OpenTelemetry collector, over OTLP/HTTP with
JSON encoding. Give the collector's base URL (`/v1/traces` is appended) or the full traces
URL:

```text
mcp-conformance watch --otlp http://localhost:4318 stdio -- python my_server.py
```

The spans follow the OpenTelemetry
[semantic conventions for MCP](https://github.com/open-telemetry/semantic-conventions-genai/blob/5293effb97f18ddb0602da884dadee6119d67679/docs/gen-ai/mcp.md)
(read at that commit; their status is Development, so they may still change):

- **Name** `{mcp.method.name} {target}`, the target being the tool of a `tools/call` or
  the prompt of a `prompts/get` (`tools/call get_weather`), else the method alone.
- **Kind** `CLIENT`, for every exchange, whoever started it. The conventions define the
  client span as the initiator's view, from sending the request to receiving the answer,
  and the server span as the receiver's processing. A tap on the wire sees exactly the
  first and never the second, so it claims no timing it does not have.
- **Attributes** `mcp.method.name`; `jsonrpc.request.id` (not for a notification);
  `mcp.session.id` (HTTP); `mcp.protocol.version`; `jsonrpc.protocol.version` when not
  `2.0`; `network.transport` (`pipe` for stdio, `tcp` for HTTP) with
  `network.protocol.name` and `network.protocol.version` for HTTP; `server.address` and
  `server.port` (the upstream) for a client's request over HTTP; `gen_ai.tool.name` with
  `gen_ai.operation.name = execute_tool`, `gen_ai.prompt.name`, `mcp.resource.uri` (the
  URI up to its path: userinfo, query and fragment are dropped, since a presigned URI
  keeps its credential there).
- **Errors** set `error.type` and `rpc.response.status_code` to the JSON-RPC code and the
  span status to `ERROR`, described by the code and a fixed name for it
  (`JSON-RPC error -32602 (invalid params)`), never by the server's own message, which can
  echo what it was sent; a tool result with `isError: true` sets
  `error.type = tool_error`; a request the session ended before answering, `unanswered`.
- **Findings** are span events named `mcp_conformance.finding`, with
  `mcp_conformance.requirement.id`, `.requirement.level`, `.outcome`, `.revision`,
  `.spec.url`, `.check`, `.detail` (cut at 512 characters, marked) and `.trace.seq` — this
  tool's own namespace, since the conventions have none for conformance. A finding goes on the span of the exchange that
  holds its `seq`, or, when that span has already been sent, on the span of the exchange
  that surfaced it.

Each session is one trace id, each exchange a span id, both random per run from the
operating system's generator. None of the conventions' opt-in attributes — tool
arguments, tool results, prompt variables — is ever set: `watch` exports no payload.

Spans travel in batches, off the session's path, through a bounded queue. A collector
that is down, slow or refusing costs the spans sent to it and nothing else; the summary at
the end says how many were exported and how many were not, and why. CI sends the spans of
a corpus replay through a pinned OpenTelemetry Collector and reads back what it wrote.

## What it listens on and what it sends

`watch` listens only where `mcp-trace-capture` would: nowhere in `stdio` mode, and on
`--listen` in `http` mode — loopback by default, with the recorder's warning for any other
address, since anyone who can reach the proxy can send requests through it. It sends
nothing anywhere unless `--otlp` names a collector, and then only the span fields above.
An `http://` collector that is not this machine gets a warning, because those fields —
method and tool names, resource URIs up to their path, session ids, error codes,
findings — would cross the network unencrypted. The trace file holds message bodies in full, as every recording
does: read it before you share it.
