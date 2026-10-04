<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Troubleshooting

Each entry starts from what you see.

## The validator

**`error: … judged no requirement at all — an empty or contentless trace is a capture
that failed, not a session that conformed`** (exit 2). The trace has no messages: the
client never connected, connected to the server directly instead of through the
capture, or the capture was stopped before the session. Check the capture's exit
summary, which counts what it recorded.

**`error: the trace declares protocol revision(s) 2025-06-18, which no available
registry describes`** (exit 2). The session ran an older revision than this build has
requirements for (`2025-11-25` and `2026-07-28`). Judging it against either would
report that revision's rules as violations, so it is refused. `--revision 2025-11-25`
judges it anyway, if the differences do not matter to you.

**`note: this trace records 2 sessions and is judged as one`.** Two clients, or one
client twice, were recorded into one file — a proxy left running across test runs, or
a client that reconnected. Findings that span them (a request id the second session
reuses, a session id that changed) come from recording them together. Record one
session per trace.

**`note: the trace declares no protocol revision; judging it against 2026-07-28`.**
The recording holds no `initialize`, no `_meta` protocol version and no
`MCP-Protocol-Version` header — often a trace that starts mid-session. Pass
`--revision` to say which rules apply.

**`error: malformed trace: …`** (exit 3). The file is not a trace as written by a
recorder:

| The error says | Cause and fix |
|----------------|---------------|
| `the document begins with a UTF-8 byte-order mark` | An editor or Windows tool added a BOM. Strip the first three bytes. |
| `the file is UTF-16` (a hint after `not valid UTF-8`) | Windows PowerShell's `>` writes UTF-16. Re-encode: `Get-Content in.jsonl \| Set-Content -Encoding utf8 out.jsonl`. |
| `the document is a single JSON value, not JSON Lines` | A pretty-printed file or a JSON array. The message names the `jq` command that converts it. |
| `blank line` | JSON Lines has no blank records; remove the empty line. |
| `record is N bytes, exceeding the …-byte limit` | A message larger than the default limit. Re-run with the `--max-line-bytes` value the hint gives. |
| `seq N is not greater than the previous event's seq` | Two recordings concatenated, or a hand-edited file. Each trace's `seq` must increase. |

## The capture tool

**`… exists; appending would break the trace's sequence numbers`** (exit 2). The
output file is there from an earlier run. Use a new path, or `--force` to overwrite —
the usual choice in a client's configuration, which relaunches servers.

**The server works directly but not through the HTTP proxy, with `403`.** The server
checks the `Host` header, which the proxy rewrites to name the upstream. Add
`--preserve-host`.

**The capture's exit summary counts messages that were not JSON.** The server writes
something other than protocol messages to stdout (log lines, a banner). That is itself
a stdio transport violation; send logs to stderr.

**A client configuration cannot find `mcp-trace-capture`.** Desktop applications often
start without your shell's `PATH`. Use the absolute path to the binary as the
`command`, for example the output of `which mcp-trace-capture`.

## Still stuck

Open an [issue](https://github.com/tomtom215/mcp-conformance/issues) with the command,
the full output, and — if you can share it — the trace. Traces record message content
in full; read one before attaching it.
