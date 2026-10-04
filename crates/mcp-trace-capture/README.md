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

## stdio servers

Configure your MCP client to launch the wrapper instead of the server:

```text
mcp-trace-capture -o session.jsonl stdio -- python my_server.py --its-flags
```

The client and the server see each other's bytes unchanged; the server's stderr is
passed through untouched. When the session ends, the server's exit code is the
wrapper's exit code. Then:

```text
mcp-trace-validator validate session.jsonl
```

**Stopping.** The server runs in a process group of its own (on Unix), so a
launcher in front of it — `npx`, `uv run`, `sh -c` — cannot strand the real server.
`SIGINT`, `SIGTERM` and `SIGHUP` sent to the wrapper are relayed to that whole group;
the server gets 3 seconds to exit on its own terms, then the group is killed
(`SIGKILL`). A second signal skips the rest of the wait. Either way the trace is
closed with a final lifecycle event before the wrapper exits with the server's
status. (Elsewhere, Ctrl-C ends the server.)

### In a client's configuration

Clients launch a stdio server from a command and its arguments; the wrapper goes in
front of both. Two things matter when a client, not you, starts it: give `-o` an
**absolute path** (the client's working directory is its own), and pass **`--force`**
or a fresh path per run, because the wrapper refuses to overwrite an existing trace
and a client relaunches its servers.

The `mcpServers` shape most desktop clients use:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "mcp-trace-capture",
      "args": ["-o", "/tmp/my-server.jsonl", "--force", "stdio", "--", "python", "/path/to/my_server.py"]
    }
  }
}
```

From a test suite, through the official SDKs' stdio clients (Python's
`StdioServerParameters`, TypeScript's `StdioClientTransport`) — the server's command
moves behind `--`:

```python
StdioServerParameters(
    command="mcp-trace-capture",
    args=["-o", "/tmp/session.jsonl", "--force", "stdio", "--", "python", "my_server.py"],
)
```

```typescript
new StdioClientTransport({
  command: "mcp-trace-capture",
  args: ["-o", "/tmp/session.jsonl", "--force", "stdio", "--", "node", "build/index.js"],
});
```

After the session, `mcp-trace-validator validate /tmp/session.jsonl`; in CI,
`--format sarif` or `--format junit` and the exit code.

## Streamable HTTP servers

Start the proxy in front of the server and point the client at the proxy:

```text
mcp-trace-capture -o session.jsonl http --upstream http://localhost:3000 --listen 127.0.0.1:8080
# client URL: http://127.0.0.1:8080/mcp   (request paths are appended to --upstream's)
```

Stop it with Ctrl-C (or `SIGTERM`/`SIGHUP`). Open event streams — an MCP client keeps
a GET stream open for its whole session — are ended at once; other requests in flight
get 3 seconds to finish. The trace is then closed with a `transport-close` event and
the summary printed. A second signal stops at once. `https://` upstreams work (rustls, the platform's root
certificates). SSE streams are relayed as they arrive, event by event.

## What it guarantees

- **Transparent.** Bytes are forwarded unchanged. Over HTTP, the only changes are the
  ones a proxy must make: hop-by-hop headers (RFC 9110 §7.6.1), `Host` (names the
  upstream — `--preserve-host` keeps the client's instead), and `Accept-Encoding`
  (removed, so the server answers uncompressed and the body can be recorded). The
  official conformance suite scores the same through the proxy as without it; this
  repository's CI checks that on every run.
- **Causal order.** Each message is recorded before the bytes that complete it are
  forwarded, so a response is never recorded ahead of its request.
- **Nothing altered to fit the trace.** A message larger than `--max-message-bytes`
  (default 64 MiB) is forwarded intact, left out of the trace, and counted in the
  summary printed at exit. So is the rare message within the limit whose recorded
  line would not be (below), and, over HTTP, a body or SSE event that is not JSON.
- **Nothing on a stdio stream goes unjudged.** A stdio line that is not JSON — a log
  line on the server's stdout, a blank line, a truncated message — is forwarded
  intact and recorded as a message whose payload is the line as a JSON string
  (invalid UTF-8 replaced, line terminator removed). The validator reports it as
  not a valid MCP message (`TRAN-004`/`TRAN-005` at 2025-11-25, `TRAN-117` at
  2026-07-28), and the exit summary counts it.
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
  every value is: review a trace before sharing it.
- **Crash-safe.** Every event is flushed as it is written; a capture killed with
  `SIGKILL` keeps what it recorded. Any other request to stop closes the trace first.

## Limits, stated plainly

- **One session per trace.** The validator judges a trace as one session. Record one
  client at a time, or run one proxy per client: concurrent clients through one proxy
  interleave, and request ids reused across them read as reuse within one session.
- **The SDK-independent path.** The wrapper and proxy link no MCP SDK; what is recorded
  is what crossed the wire.

## Exit codes

| Code | Meaning |
|------|---------|
| server's | `stdio`: the wrapped server's exit code (128 + signal if a signal ended it, on Unix — 137 for a server killed after the grace period) |
| 0 | `http`: stopped cleanly with a complete trace |
| 128 + signal | `http`: a second signal stopped the proxy without waiting for open requests |
| 2 | Bad arguments, an existing output file (`--force` overwrites), a server that would not start, an address that would not bind |
| 3 | The session ran but the trace is incomplete (a write failed), when the code would otherwise be 0 |

## License

MIT
