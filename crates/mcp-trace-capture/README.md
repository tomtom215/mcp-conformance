<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-trace-capture

Record any [Model Context Protocol](https://modelcontextprotocol.io) session — any
language, any SDK — as a trace that
[`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator) judges clause by
clause against the specification.

```text
cargo install mcp-trace-capture mcp-trace-validator
```

> **This README is `main`'s.** `cargo install` gets the latest release, `0.6.0`,
> whose own README is the one on crates.io. Documentation for the release is at
> <https://mcp-conformance.com>, and for `main` at <https://mcp-conformance.com/dev/>.
>
> **Not yet released** (on `main` since `0.6.0`):
>
> - a stop giving the stdio server 1 second before its process group is killed (`0.6.0`:
>   3 seconds);
> - `{session}` numbers continuing after the highest present (`0.6.0`: the lowest free
>   one);
> - the proxy's `/` being the `--upstream` URL itself, and redirects into the upstream
>   pointed back at the proxy (`0.6.0` appended `/` to the upstream path);
> - the client URL printed at start, the non-2xx and 404 summary at exit, hints for
>   near-miss arguments, and the exit codes in `--help`;
> - a forced HTTP stop ending the trace with `transport-abort`; SSE events of a type
>   other than `message` left out; unfaithful JSON recorded as its raw line (stdio) or
>   left out (HTTP);
> - on Windows, the server's whole exit code.

## stdio servers

Configure your MCP client to launch the wrapper instead of the server:

```text
mcp-trace-capture -o session.jsonl stdio -- python my_server.py --its-flags
```

`-o` names the trace (without it, `mcp-trace.jsonl` in the working directory) and
may come before or after the mode. The client and the server see each other's bytes
unchanged; the server's stderr is passed through untouched. When the session ends, the server's exit code is the
wrapper's exit code. Then:

```text
mcp-trace-validator validate session.jsonl
```

**Stopping.** The server runs in a process group of its own (on Unix), so a
launcher in front of it — `npx`, `uv run`, `sh -c` — cannot strand the real server,
and a terminal's Ctrl-C reaches the server once, through the wrapper, not twice.
`SIGINT`, `SIGTERM`, `SIGHUP` and `SIGQUIT` sent to the wrapper first close the trace with a
`transport-close` event, then are relayed to that whole group; the server's processes
get 1 second from the signal to exit on their own terms, then the group is killed
(`SIGKILL`). A second signal skips the rest of the wait. What the server writes after
the close is still relayed to the client, and counted in the exit summary instead of
recorded. The wrapper then exits with the server's status. Other signals keep their
default action: one that ends a process (`SIGUSR1`, `SIGUSR2`, `SIGALRM`) ends the
wrapper as `SIGKILL` would, below, so send one of the four above.

That 1-second grace is deliberately shorter than the 2 seconds the official SDKs' stdio
clients wait between `SIGTERM` and `SIGKILL`. They send both to the wrapper alone
(TypeScript) or to the wrapper's process group (Python) — never to the server's — so
the wrapper must have killed the server before their `SIGKILL` arrives; the server has
already had the 2 seconds they wait after closing its stdin. A `SIGKILL` that reaches
the wrapper sooner, from anything else, leaves a server that ignores `SIGTERM`
running: no process can act on its own `SIGKILL`. The trace is closed either way,
since the close comes first.

### In a client's configuration

Clients launch a stdio server from a command and its arguments; the wrapper goes in
front of both. Two things matter when a client, not you, starts it: give `-o` an
**absolute path** (the client's working directory is its own), and put
**`{session}`** in the file name. A client relaunches its servers, and the wrapper
never overwrites a trace: with `{session}` each launch takes the number after the
highest one present (`my-server-001.jsonl`, `my-server-002.jsonl`, …), so every
session is kept and the files sort in session order. Without
it, a second launch refuses to start unless `--force` lets it overwrite the first.

The `mcpServers` shape Claude Desktop and Cursor use:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "mcp-trace-capture",
      "args": ["-o", "/tmp/my-server-{session}.jsonl", "stdio", "--", "python", "/path/to/my_server.py"]
    }
  }
}
```

VS Code's `.vscode/mcp.json` names the same fields under `servers`, with a `type`:

```json
{
  "servers": {
    "my-server": {
      "type": "stdio",
      "command": "mcp-trace-capture",
      "args": ["-o", "/tmp/my-server-{session}.jsonl", "stdio", "--", "python", "/path/to/my_server.py"]
    }
  }
}
```

The book's [client chapter](https://mcp-conformance.com/dev/clients.html) has each
client's file location and its HTTP form.

From a test suite, through the official SDKs' stdio clients (Python's
`StdioServerParameters`, TypeScript's `StdioClientTransport`) — the server's command
moves behind `--`:

```python
StdioServerParameters(
    command="mcp-trace-capture",
    args=["-o", "/tmp/traces/{session}.jsonl", "stdio", "--", "python", "my_server.py"],
)
```

```typescript
new StdioClientTransport({
  command: "mcp-trace-capture",
  args: ["-o", "/tmp/traces/{session}.jsonl", "stdio", "--", "node", "build/index.js"],
});
```

The directory must exist. After the sessions, `mcp-trace-validator validate
/tmp/traces/*.jsonl` judges each trace; in CI, `--format sarif` or `--format junit`
and the exit code.

## Streamable HTTP servers

Start the proxy in front of the server and point the client at the proxy:

```text
mcp-trace-capture -o session.jsonl http --upstream http://localhost:3000/mcp --listen 127.0.0.1:8080
# client URL: http://127.0.0.1:8080/   (the proxy's root is --upstream itself)
```

The proxy's root path is the `--upstream` URL itself, and any other request path is
appended to the upstream's: above, `/` reaches `/mcp` and `/x` reaches `/mcp/x`
(with `--upstream http://localhost:3000`, point the client at
`http://127.0.0.1:8080/mcp`). The proxy prints the client URL when it starts. At exit
it says how many responses had a status outside 2xx, and, when the server answered
404, asks whether the endpoint path is right — a client at `http://127.0.0.1:8080/mcp`
in front of `--upstream http://localhost:3000/mcp` reaches `/mcp/mcp`.

An upstream URL's own query (`https://host/mcp?key=…`) is kept, and each request's
query is appended to it. Credentials in the URL (`user:password@`) are refused: the
proxy would not send them, so have the client send them as a header. The proxy
listens on loopback unless `--listen` says otherwise, and warns when it does — anyone
who can reach a non-loopback address can send requests through it.

Stop it with Ctrl-C (or `SIGTERM`/`SIGHUP`/`SIGQUIT`). Standalone event streams — the GET
stream an MCP client keeps open for its whole session — are cut at once (aborted, not
ended cleanly, so the client can tell); every other request in flight, SSE responses
to POSTs included, gets 3 seconds to finish. The trace is then closed with a
`transport-close` event and the summary printed. A second signal stops at once, and
the trace ends with a `transport-abort` instead: requests still in flight were cut
off. Anything a cut-off exchange carries after the close is counted in the summary,
not recorded. `https://` upstreams work (rustls,
the platform's root certificates — loaded only for an `https` upstream, and checked
before the trace is created: where none can be found, the proxy refuses to start).
SSE streams are relayed as they arrive, event by event.

### `Host`, and DNS rebinding

By default the proxy sends the upstream's own name in `Host`, as a reverse proxy must
for a virtually hosted server, which routes on it; the trace records the `Host` the
client sent. The cost: a server that defends against DNS rebinding by checking
`Host` — the MCP specification's transport security advice — sees its own name on
every request the proxy forwards, so its check passes for all of them, including one
a web page has a browser send to the proxy's loopback address under a rebound domain
name. The proxy checks no `Host` itself. (`Origin` is forwarded unchanged, so a
server that checks `Origin` still sees the page's.)

When that matters — you are testing the server's `Host` check, or a browser you do
not trust can reach the proxy — run with `--preserve-host`: the server then sees, and
judges, the `Host` the client sent. Not in front of a virtually hosted remote server,
which needs its own name. This repository's CI runs the official conformance suite
through the proxy with `--preserve-host` (`xtask/src/conformance/capture.rs`), so the
suite's `dns-rebinding-protection` scenario exercises the server's own check.

## Windows

The HTTP proxy works as described. The stdio wrapper relays and records the same
way, but Windows has no signals and no process groups, so stopping differs:

- Only Ctrl-C asks the capture to stop (there is no `SIGTERM` or `SIGHUP`). For the
  stdio wrapper it closes the trace, then ends the server at once: there is no grace
  period. A console's Ctrl-C also reaches every process attached to it, the server
  included.
- Ending the server ends that one process. A launcher's children (`npx` starting
  `node`, `uv run` starting `python`) are not ended with it and can outlive the
  session: put the real server behind `--`, not a launcher.
- A client that force-stops the wrapper — the TypeScript SDK's `kill()` is
  `TerminateProcess` — ends it before it can write the trace's closing event, and
  leaves the server running unless the client ends the whole process tree (the
  Python SDK does, with a job object).
- The server's exit code is passed through whole, not cut to eight bits.

CI builds and tests the crate on Windows, but the tests of stopping — signals,
process groups, the SDKs' shutdown sequence — run only on Unix.

## What it guarantees

- **Transparent.** Bytes are forwarded unchanged. Over HTTP, the only changes are the
  ones a proxy must make: hop-by-hop headers (RFC 9110 §7.6.1), `Host` (names the
  upstream — `--preserve-host` keeps the client's instead), `Accept-Encoding`
  (removed, so the server answers uncompressed and the body can be recorded), and a
  redirect's `Location` that points into the upstream (pointed back at the proxy, by
  the `Host` the client used, so the client does not leave the recording; one the
  proxy cannot reach is relayed as sent, and `Location` is not recorded). The
  official conformance suite scores the same through the proxy as without it; this
  repository's CI checks that on every run.
- **Upstream failures as they happened.** An upstream that fails mid-response reaches
  the client as the same truncation — its status and headers, then a body cut off —
  and the trace records `transport-abort`. Only an upstream that cannot be reached at
  all gets the proxy's own `502`, whose body names the upstream (without its query)
  and the cause.
- **Causal order.** Each message is recorded before the bytes that complete it are
  forwarded, so a response is never recorded ahead of its request.
- **Nothing altered to fit the trace.** A message larger than `--max-message-bytes`
  (default 64 MiB) is forwarded intact, left out of the trace, and counted in the
  summary printed at exit. So is the rare message within the limit whose recorded
  line would not be (below), and, over HTTP, a body or SSE event that is not JSON or
  is JSON the trace cannot hold as sent (below), and an SSE event whose `event` type
  is not `message` — MCP clients, both official SDKs' among them, act on no other.
- **Nothing on a stdio stream goes unjudged.** A stdio line that is not JSON — a log
  line on the server's stdout, a blank line, a truncated message — is forwarded
  intact and recorded as a message whose payload is the line as a JSON string
  (invalid UTF-8 replaced, line terminator removed). The validator reports it as
  not a valid MCP message (`TRAN-004`/`TRAN-005` at 2025-11-25, `TRAN-117` at
  2026-07-28), and the exit summary counts it. So is a line of JSON the trace cannot
  hold as sent (below), counted apart, and what a stream ends with after its last
  newline — JSON or not: stdio frames a message by its newline, so the peer never
  received it as one.
- **Readable by the validator as recorded.** No trace line is longer than
  `--max-message-bytes` plus 1 MiB, checked on the line as written — which, at the
  default, is exactly the longest line `mcp-trace-validator` accepts by default. With
  a larger `--max-message-bytes`, the capture prints the `--max-line-bytes` value to
  validate with.
- **No credentials in the trace.** Only an allowlist of headers is recorded — the same
  list every capture in this project uses (`RECORDED_HEADERS` in
  `mcp-conformance-core`). `Authorization` and cookies are forwarded, never written.
  **Message content is recorded in full** — as parsed JSON, so whitespace, member
  order (written sorted) and number spelling (`1E2` as `100.0`) are not kept, but
  every value is: review a trace before sharing it. JSON whose parsed value would
  differ from what was sent — a number a double cannot hold exactly (an id past 64
  bits, which would come back as a non-integer), or a member name repeated in one
  object (only the last would remain) — is never recorded as parsed JSON: the
  validator reads payloads the same way, so no recorded value could say what was
  sent. It is treated as the transport treats non-JSON: over stdio, recorded as the
  line, over HTTP, left out; counted in the summary either way.
- **Crash-safe.** Every event is flushed as it is written; a capture killed with
  `SIGKILL` keeps what it recorded. Any other request to stop closes the trace first.

## Limits, stated plainly

- **One session per trace.** The validator judges a trace as one session. With
  `{session}` in `-o`, the HTTP proxy writes each client session to its own numbered
  file: a session begins with an `initialize`, or with an `Mcp-Session-Id` the proxy
  has not seen, and every later exchange naming that id goes to its trace. An
  exchange naming no id goes to the session begun last — counting one begun by an
  unseen id only once the server has answered it with a 2xx, so a stale id the
  server refuses gets its own trace without drawing in the rest.
  `2026-07-28` traffic carries no session id and no `initialize`, so the proxy cannot
  tell its clients apart: every `2026-07-28` session through one proxy lands in one
  trace, `{session}` or not, and concurrent `2026-07-28` clients need a proxy each. Without `{session}`, everything goes to the one file, and the
  capture warns at exit when it holds more than one session (more than one client
  `initialize` request, or more than one `Mcp-Session-Id`).
- **A response that is not an event stream is held until it is complete.** The
  proxy reads a JSON (or any other non-SSE) body whole — up to `--max-message-bytes`
  — before it relays the status, headers and body. That is how the status event and
  the message are written as adjacent lines, which is how the validator pairs a body
  with its headers when exchanges overlap, and how the message is recorded before the
  bytes that complete it. So such a response reaches the client only once it has
  ended or passed the limit (the rest then streams): a body sent slowly arrives all
  at once at the end, one that never ends (an endless ndjson stream) starts only past
  the limit, and each response in flight can hold up to `--max-message-bytes` in
  memory. Event streams are relayed event by event.
- **An SSE event's data, and nothing else.** Only the `data` of events of type
  `message` is recorded; `id` and `retry` fields are forwarded but not recorded, so a
  trace does not show which event a client could resume from (the `Last-Event-ID`
  request header is recorded).
- **The SDK-independent path.** The wrapper and proxy link no MCP SDK; what is recorded
  is what crossed the wire.

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | `http`: stopped with a complete trace |
| 1 | `stdio`: waiting for the server failed (an operating-system error); the trace keeps what it recorded, without a closing event unless a stop wrote one |
| 2 | A run that could not start: bad arguments (`-o -` among them: the trace needs a path; an `--upstream` that is not an absolute `http`/`https` URL, carries credentials, or whose root certificates cannot be loaded), an existing output file (`--force` overwrites), `{session}` outside the file name or more than once, a `{session}` path whose directory does not exist, a server that would not start, an address that would not bind. No trace file is created: the file is made only once the session can start. Also `http`, after it started: serving failed (the error is printed; the trace keeps what it recorded, without a closing event) |
| 3 | The session ran but a trace is incomplete (a write failed), when the code would otherwise be 0 |
| 128 + signal | `http`: a second signal stopped the proxy without waiting for open requests; the trace ends with `transport-abort` |
| server's | `stdio`: the wrapped server's exit code — on Unix 128 + signal if a signal ended it (137 for a server killed at the end of the grace period); on Windows the whole code |

## License

MIT
