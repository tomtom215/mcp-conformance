<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Capturing from your client

A client records nothing by itself: you put `mcp-trace-capture` between it and the
server. For a **stdio** server, the client launches the capture wrapper, which
launches the server. For a **streamable-HTTP** server, the client talks to the
capture proxy, which forwards to the server. Either way the bytes pass through
unchanged and the trace is written as they go.

Two rules hold for every client configuration below:

- **Give `-o` an absolute path.** A desktop client starts servers in a working
  directory of its own choosing; a relative path lands somewhere you will not look.
  Without `-o`, the trace is `mcp-trace.jsonl` in that directory.
- **Put `{session}` in the file name.** Clients relaunch their servers — at startup,
  on reconnect, when you reopen a window — and the wrapper never overwrites a trace.
  With `{session}`, each launch writes the next number (`my-server-001.jsonl`,
  `my-server-002.jsonl`, …); without it, the second launch refuses to start. The
  directory must exist.

Desktop applications often start without your shell's `PATH`, so use the absolute
path to `mcp-trace-capture` as the command if the client cannot find it (`which
mcp-trace-capture` prints it). Restart the client after editing its configuration.

## Claude Desktop

The configuration file is `claude_desktop_config.json`, at
`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS and
`%APPDATA%\Claude\claude_desktop_config.json` on Windows (Settings → Developer →
Edit Config opens it). Servers go under `mcpServers`; move the server's own command
behind `--`:

```json
{
  "mcpServers": {
    "my-server": {
      "command": "mcp-trace-capture",
      "args": [
        "-o", "/Users/me/traces/my-server-{session}.jsonl",
        "stdio", "--",
        "python", "/Users/me/src/my_server.py"
      ]
    }
  }
}
```

On Windows, write the paths with escaped backslashes (`"C:\\Users\\me\\traces\\my-server-{session}.jsonl"`).
The server's stderr still reaches the client's own log, as without the wrapper.

## VS Code

A workspace's servers are in `.vscode/mcp.json`, under `servers`. A stdio server:

```json
{
  "servers": {
    "my-server": {
      "type": "stdio",
      "command": "mcp-trace-capture",
      "args": [
        "-o", "/home/me/traces/my-server-{session}.jsonl",
        "stdio", "--",
        "node", "/home/me/src/my-server/build/index.js"
      ]
    }
  }
}
```

A streamable-HTTP server is an entry with `"type": "http"` and a `url`: start the
proxy in front of the server, and give VS Code the proxy's URL instead of the
server's:

```text
mcp-trace-capture -o '/home/me/traces/http-{session}.jsonl' http --upstream http://127.0.0.1:3000/mcp
```

```json
{
  "servers": {
    "my-http-server": {
      "type": "http",
      "url": "http://127.0.0.1:8080/"
    }
  }
}
```

The proxy's root path is the `--upstream` URL itself, so `http://127.0.0.1:8080/`
reaches `http://127.0.0.1:3000/mcp`; the proxy prints the URL to use when it starts.
Stop it with Ctrl-C when the session is over: the trace is closed then.

## Cursor

Cursor reads `~/.cursor/mcp.json` (every project) and `.cursor/mcp.json` (one
project), with servers under `mcpServers`. A stdio server has the same shape as
Claude Desktop's above. A remote server is an entry with a `url`, which for a
recording is the capture proxy's, as for VS Code:

```json
{
  "mcpServers": {
    "my-http-server": {
      "url": "http://127.0.0.1:8080/"
    }
  }
}
```

## Other clients

Most clients that launch stdio servers take a command and its arguments, in one of
the two shapes above; the wrapper goes in front of both, with the server's command
after `--`. A client that connects to a URL is pointed at the proxy. The official
SDKs' stdio clients, for test suites, are covered in
[Using it in CI](ci.md#recording-from-a-test-suite).

## No client at all

To exercise a **server** without a client application, `mcp-reference-host` drives
it: it lists the tools and calls each once with arguments derived from its schema,
and with `--sweep` also drives prompts, resources, templates and completion.
`--trace-dir` records the session:

```text
cargo install mcp-reference-host
mcp-reference-host --server-cmd "python my_server.py" --sweep --trace-dir traces/
mcp-reference-host --url http://127.0.0.1:3000/mcp --sweep --trace-dir traces/
mcp-trace-validator validate traces/*.jsonl
```

`--server-cmd` is split on spaces, so a path containing one needs a wrapper script.
`--protocol-version 2026-07-28` speaks the stateless revision instead of
`2025-11-25`. The host's own recording sits at the message seam and carries no HTTP
headers; when transport behaviour matters, record through `mcp-trace-capture`
instead, with the host pointed at the proxy. `mcp-reference-host --help` lists the
rest — `--subscribe`, `--cancel`, `--log-level` and `--probe` each drive clauses a
plain run leaves unobserved.

To exercise a **client**, run `mcp-everything-server --transport http` behind the
proxy and point the client at it; with a known-good server on the other end, a
finding is almost certainly the client's.
