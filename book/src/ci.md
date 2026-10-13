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

```bash
mcp-trace-capture -o 'traces/http-{session}.jsonl' http --upstream http://127.0.0.1:3000 \
  2> proxy.log &
proxy=$!
# Wait until the proxy is listening; stop if it exited instead (proxy.log says why).
until grep -q 'listening on' proxy.log; do
  kill -0 "$proxy" 2> /dev/null || { cat proxy.log; exit 1; }
  sleep 0.2
done
npm test          # with the client pointed at http://127.0.0.1:8080/mcp
kill -INT "$proxy"
wait "$proxy"
```

The proxy prints its `listening on` line only once its port is bound, so waiting for
it — rather than probing the port with a request, which the proxy would forward and
record — keeps the first test from racing the proxy's start.

`{session}` gives each test session its own numbered trace: each launch of the stdio
wrapper, and each client session through the proxy (the `traces/` directory must
exist). The repository's
[`interop/`](https://github.com/tomtom215/mcp-conformance/tree/main/interop) directory
does exactly this with the official TypeScript and Python SDKs, and its `run.sh` is a
working example of both transports; its `matrix.sh` records each official SDK's own
conformance server through the proxy ([Recording an SDK's server](sdk-servers.md)).

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

### Recording with the action

The action can also take the recording: set `capture`, give it the `server` and the
`client` command that drives it, and it runs the session through
`mcp-trace-capture`, then judges every session recorded (with any `traces` you name
too). This is not yet released: it is on `main`, so pin a commit for it, and it
needs the action's own source build (`version` empty) until a release carries it.

```yaml
# stdio: the client launches $MCP_SERVER_COMMAND in place of the server.
- uses: tomtom215/mcp-conformance@<commit>
  with:
    capture: stdio
    server: python my_server.py
    client: pytest tests/mcp -k e2e   # its fixture launches $MCP_SERVER_COMMAND

# Streamable HTTP: the client talks to $MCP_PROXY_URL, which reaches `upstream`.
- uses: tomtom215/mcp-conformance@<commit>
  with:
    capture: http
    server: node dist/server.js --port 3000   # optional: started, waited for, stopped
    upstream: http://127.0.0.1:3000/mcp
    client: npm run e2e -- --url "$MCP_PROXY_URL"
```

| Input | Meaning |
|-------|---------|
| `capture` | `stdio` or `http`; empty (the default) records nothing |
| `server` | A bash command line, run from the workspace. stdio: the server to record (required). http: optional — started in the background, waited for until `upstream` accepts connections (60 s), and stopped after the client |
| `upstream` | http: the server's URL; the recording proxy forwards to it, and `MCP_PROXY_URL` reaches it (a path after the proxy's address is appended to its path) |
| `client` | A bash command line, run from the workspace, that drives the sessions. It gets `MCP_SERVER_COMMAND` (stdio: a single executable path that runs `server` under the recorder) or `MCP_PROXY_URL` (http) |
| `capture-dir` | Where each session is written as `NNN.jsonl`; default `mcp-conformance-capture`. It must not exist yet, or be empty |

Every launch of `MCP_SERVER_COMMAND`, and every session through the proxy, is its
own trace. The stdio wrapper carries the server's command line itself rather than
reading the environment, because clients commonly launch stdio servers with a reduced
one; the server runs with whatever environment the client gives it. A client that
exits non-zero fails the action — after the sessions it did record are judged and
reported — and its status is the `client-exit-code` output. CI exercises capture on
Linux runners (both transports, driving the repository's everything server with its
reference host); the stdio wrapper is a bash script, which a Windows client cannot
launch as a command.

## Container image

From the release that follows `0.6.0`, each release also publishes both CLIs as
`ghcr.io/tomtom215/mcp-conformance:<version>` (linux/amd64), built from the
repository's
[`Dockerfile`](https://github.com/tomtom215/mcp-conformance/blob/main/Dockerfile) and
attested like the binaries:

```text
docker run --rm -v "$PWD:/work" ghcr.io/tomtom215/mcp-conformance:<version> \
  mcp-trace-validator validate --format junit traces/*.jsonl > mcp-conformance.xml
gh attestation verify oci://ghcr.io/tomtom215/mcp-conformance:<version> \
  --repo tomtom215/mcp-conformance
```

The image is distroless: no shell and no package manager, running as UID 65532, with
`/work` as its working directory — mount the directory holding your traces there (and
make it writable by that user to record into it). With no shell, `mcp-trace-capture
stdio` can only launch a server that is in the image, so to record a stdio server in
a container, build `FROM` this image and add yours; `mcp-trace-capture http` needs
nothing more.

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
than passing it. `fail-on-findings: false` gives that up as well: the action's step
then succeeds whatever the exit status, `2` and `3` included, so a step that relies on
it should read the action's `exit-code` output and fail on `2` and `3` itself. When the
validator cannot start at all — a `traces` pattern matching nothing, or a lone trace it
cannot read — the action still writes both reports, as a SARIF run marked
`executionSuccessful: false` and a JUnit `<error>`, so an `if: always()` upload step
finds a valid file.
