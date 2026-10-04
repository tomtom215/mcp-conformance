<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Using it in CI

The shape is the same everywhere: the tests you already run drive your server or
client *through* `mcp-trace-capture`, and a later step judges the traces they leave.
The validator's exit status is the verdict, so a failing clause fails the build.

## Recording from a test suite

Wherever your tests start the server, put the capture in front of it. With the
official SDKs' stdio clients, the server's command moves behind `--`:

```python
StdioServerParameters(
    command="mcp-trace-capture",
    args=["-o", "traces/{session}.jsonl", "stdio", "--", "python", "my_server.py"],
)
```

For an HTTP server, start the proxy before the tests and stop it after:

```text
mcp-trace-capture -o 'traces/http-{session}.jsonl' http --upstream http://127.0.0.1:3000 &
proxy=$!
npm test          # with the client pointed at http://127.0.0.1:8080/mcp
kill -INT $proxy
wait $proxy
```

`{session}` gives each test session its own numbered trace: each launch of the stdio
wrapper, and each client session through the proxy (the `traces/` directory must
exist). The repository's
[`interop/`](https://github.com/tomtom215/mcp-conformance/tree/main/interop) directory
does exactly this with the official TypeScript and Python SDKs, and its `run.sh` is a
working example of both transports.

## GitHub Actions

The repository is itself an action. After the step that records the traces:

```yaml
- uses: tomtom215/mcp-conformance@<commit or tag>
  with:
    traces: traces/*.jsonl
- uses: github/codeql-action/upload-sarif@<sha>
  if: always()
  with:
    sarif_file: mcp-conformance.sarif
    category: mcp-conformance
```

The action builds `mcp-trace-validator` from the ref you name (or, with
`version: 0.6.0`, downloads that release's prebuilt binary and checks its checksum),
prints the findings to the log and the job summary, writes `mcp-conformance.sarif` and
`mcp-conformance-junit.xml`, and fails the step when a clause fails. Uploading the
SARIF log needs `permissions: security-events: write` on the job; findings then appear
in the repository's code scanning view, each on the trace line that holds the event.
The inputs — `revision`, `strict`, `fail-on-findings`, the output paths — are listed in
[`action.yml`](https://github.com/tomtom215/mcp-conformance/blob/main/action.yml).

## Any other CI

```text
mcp-trace-validator validate traces/*.jsonl                       # findings in the log
mcp-trace-validator validate --format junit traces/*.jsonl > mcp-conformance.xml
mcp-trace-validator validate --format sarif traces/*.jsonl > mcp-conformance.sarif
```

Each invocation exits with the same status, so run the human report as the gate and
the other two for the reports your CI ingests (GitLab, Jenkins and most others read
JUnit). Several traces give one document: a JUnit suite per trace and revision, one
SARIF run with each result in its own trace file.

## Choosing what fails the build

| You want | Use |
|----------|-----|
| Fail on any broken MUST | the default |
| Fail on SHOULDs too | `--strict` (JUnit and SARIF then report warnings as failures and errors) |
| Judge against a fixed revision, whatever the traces declare | `--revision 2026-07-28` |
| Never fail, only report | `fail-on-findings: false` in the action, or ignore the exit status |

A trace that cannot be judged — empty, malformed, or of a revision this build does not
know — exits `2` or `3`, never `0`: a broken recording step fails the build rather
than passing it.
