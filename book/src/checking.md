<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Checking a server

`mcp-conformance check` takes a server and returns its clause report. It needs no
client, no MCP SDK and no Node.js.

```text
# a stdio server: its command after `--`
mcp-conformance check -- python my_server.py

# a Streamable HTTP server: its endpoint
mcp-conformance check --url http://localhost:3000/mcp
```

It does three things, in order: starts the [recorder](trace-format.md) in front of
the server, talks to the server through it as a conforming client would, and judges
the recording exactly as [`validate --party server`](reading-a-report.md) does. The
report goes to stdout; the run's own account of what it exercised goes to stderr.

## What it sends

What a conforming client sends, addressed to what the server listed, and then a
few requests made wrong on purpose:

1. **Which revision.** By default it asks `server/discover`, the `2026-07-28` way to
   learn what a server supports. A server that answers as a `2026-07-28` server is
   spoken to in that revision; any other answer and it opens with the `2025-11-25`
   `initialize` handshake instead, as a client that supports both does. A stdio
   server that exits on the probe is started again and opened with `initialize`;
   an HTTP server whose stream ends without answering it is asked again on the
   same endpoint.
   `--spec-version 2025-11-25` or `2026-07-28` speaks one revision only.
2. **What the server declared.** `tools/list`, `resources/list`,
   `resources/templates/list` and `prompts/list`, each only if the server declared
   the capability, each followed through its pages.
3. **Reads.** `resources/read` for the listed resources, and `prompts/get` for the
   prompts that require no arguments.
4. **Tool calls, when the server says they are safe.** A tool is called only if the
   server annotates it `readOnlyHint: true` (and not `destructiveHint: true`).
5. **Probes.** The specification says how a server answers a request for something it
   does not have, and a session that only asks for what is offered never shows whether
   it does. So after each capability's ordinary requests `check` sends: `prompts/get`
   for a prompt the server did not list, and for a listed prompt without the arguments
   it requires; `resources/read` for a resource under a scheme the server names
   nowhere; `completion/complete` for a prompt that is not listed (or when completions
   were not declared at all); at `2025-11-25`, `logging/setLevel` with a level that is
   not one of the eight; at `2026-07-28`, `tools/list` with a cursor that is not a
   string. Each asks for something that is not there, so none can change anything on
   the server. The summary names every probe and how it was answered, and
   `--no-probes` leaves them out.

   Over HTTP, a server that protects itself with OAuth gets one more pair: a request
   that changes nothing (`ping`, or `server/discover`) with no credential, and — when
   that is refused `401` — the same request with a token nobody issued. A server that
   validates its tokens refuses that one too, with `401` and a `WWW-Authenticate:
   Bearer` challenge; one that serves it fails. The pair is sent when you gave a
   credential with `--header`, or when the server answered `401` before any session
   existed. In that second case there is nothing else to check, so `check` prints the
   report on how the server refuses, says which flag to pass, and exits 2. The token
   is the public constant `mcp-conformance-probe-invalid-token`; it is the one
   `Authorization` value a recording ever keeps, and your own credential is never
   written anywhere.

   Between the two, `check` fetches the server's Protected Resource Metadata (RFC
   9728), the document that tells a client where the authorization servers are: from
   the URL the `401`'s challenge names as `resource_metadata`, or, when it names none,
   from the well-known locations for the endpoint and then for the host, until one
   serves it. The request carries nothing of yours, not your credential and not your
   other `--header`s. A URL the challenge names is expected to serve a JSON document,
   and a document that comes back is expected to name at least one authorization
   server; a server that falls short of either fails. Only a URL on the origin you
   gave and under `/.well-known/` is fetched; the summary says so when the challenge
   names one that is not.

   Two of the probes are requests the revision's schema
   rejects, so the report holds them against the client (`[client]`), which is this
   tool; they do not count toward the server's verdict.

It declares no client capabilities, so a conforming server does not ask it for
sampling, elicitation or roots. If a server asks anyway, the request is refused
(`ping` is answered) and counted in the summary.

## What it does not do

- **It does not call tools that may change something**, unless you say so:
  `--call <tool>` (repeatable) calls a named tool whatever its annotations, and
  `--call-all` calls every listed tool. Most servers today annotate nothing, so the
  default run of such a server calls no tool; the summary says which tools were left
  and why.
- **It does not guess arguments.** They are built from the tool's own `inputSchema`:
  the required members only, from `const`, `default`, the first `enum` value, or the
  smallest value the type and its bounds allow, and a handful of string formats. The
  result is checked against the schema before it is sent. A `pattern`, an unknown
  `format` or an untyped required member means no arguments, and the tool is skipped
  with that reason.
- **It does not fill in resource templates or prompt arguments.** Nothing states what
  values would be valid, so they are listed as not exercised.
- **It does not fuzz.** The probes above are a short fixed list, each the stimulus a
  named clause is about and each a request for something absent. `check` sends no
  random or oversized input, never retries to find a crash, and is not a security
  scanner.

## The summary

```text
mcp-conformance check: python my_server.py (stdio)
  revision   2025-11-25 — `server/discover` was not answered as a 2026-07-28 server answers it, so the session fell back to `initialize`
  server     my-server 1.2.0
  tools      9 listed, 3 called
  resources  1 listed, 1 read
  prompts    not declared by the server, so not asked for
  not called:
    create_entities, delete_entities — not annotated read-only (`readOnlyHint: true`); name it with --call, or use --call-all
  requests   10 sent
  trace      /tmp/mcp-conformance-check-31192-984222902/session.jsonl
```

Read it before the report. A verdict of `pass` over a session that called no tool
says nothing about the server's tools: those clauses are reported *not observed*, and
this is where you see why. `--summary FILE` writes the same account as JSON.

## The report, and the exit status

The report is [`validate`'s](reading-a-report.md), viewed for the server: every
finding is listed with the party it is held against, and only the server's (and those
held against nobody) decide the verdict. The flags are `validate`'s too:
`--format json|junit|sarif|html`, `--strict`, `--all`, `--area`, `--level`,
`--baseline`, `--write-baseline`, `--allow-stale-baseline`, `--html-report`.

With no flag at all you also get the report as a page: `check` writes `report.html`
beside the recording and prints its path, and ends with one plain sentence saying whether
the server broke a rule. `--html-report FILE` puts the page where you say (and is how to
get one together with `-o`, or beside a JUnit or SARIF report in CI).

| Exit | Meaning |
|-----:|---------|
| `0` | Judged, and no MUST-level finding is held against the server (warnings pass unless `--strict`) |
| `1` | A clause failed |
| `2` | The run could not be made or judged: bad arguments, the server could not be started or reached, no session was established, the server negotiated a revision this build does not judge, or the output could not be written |
| `3` | The trace was not written whole |
| `130` | Interrupted |

A server that negotiates a revision older than `2025-11-25` ends the run with `2`:
this build has no registry for it, and judging it under another revision's rules
would be a wrong verdict. `check` closes the session, as a client that does not
speak a revision does.

## Adopting it on a server that already has findings

```text
mcp-conformance check --write-baseline mcp-baseline.json -- python my_server.py
mcp-conformance check --baseline mcp-baseline.json --strict -- python my_server.py
```

The first command records the findings present today; the second fails only on a
finding that is not in the file. Accepted findings stay in every report, marked and
counted apart, and an entry that no longer matches anything is *stale* and fails the
run, so a baseline cannot outlive the defect it excuses. Each recorded entry carries
a reason saying nobody has reviewed it; edit the reason when someone has, and add an
`expires` day (`YYYY-MM-DD`) to put an entry on a clock. Reading a baseline is covered in
[Reading a report](reading-a-report.md#a-baseline-of-accepted-findings).

## Bounds

| Flag | Default | Bounds |
|------|--------:|--------|
| `--max-requests` | 100 | Requests sent in the run |
| `--max-tool-calls` | 25 | Tools called |
| `--max-reads` | 25 | Resources read, and prompts fetched |
| `--max-pages` | 20 | Pages followed for one listing |
| `--timeout` | 30 | Seconds waited for one answer |
| `--max-time` | 300 | Seconds for the whole run |
| `--max-response-bytes` | 2097152 | Bytes read for one answer |

No request is retried and none is sent concurrently. A bound that stops the run is
named in the summary (`stopped early: …`), and the report judges what was recorded.

## HTTP servers that want a credential

`--header "Authorization: Bearer <token>"` (repeatable) is sent with every request.
The recorder writes the names of the headers it knows to the trace and never an
`Authorization` value; a URL's query is neither printed nor recorded in the summary.
The trace still holds every message body, so treat it as you treat the server's own
traffic.

## The recording

`-o session.jsonl` names the trace (`--force` overwrites it); without `-o` it is a new
file in a new private directory under the system's temporary one, with `report.html`
beside it, and both paths are printed. It is an ordinary [trace](trace-format.md):

```text
# judge it again, for both parties
mcp-conformance validate session.jsonl

# one self-contained page, beside the text report
mcp-conformance validate --html-report report.html session.jsonl

# keep it in the workbench's library
mcp-conformance library import session.jsonl
mcp-conformance serve
```

## How `check`, `test` and `watch` differ

| | Drives the server with | Reports |
|---|---|---|
| `check` | A small client of its own, read-only by default | The clause report of that session |
| [`test`](testing.md) | The official suite's scenarios (needs Node.js), which call the suite's own fixture tools | The suite's checks, the clause report of every session, and whether the two agree |
| [`watch`](watching.md) | Your client | Findings as they happen, then the clause report |

`check` is the place to start with a server of your own. `test` is the way to run the
official suite, which remains the authority on what "conformant" means; against a
server that is not the suite's fixture server most of its scenarios fail for want of
the fixtures, and `test` says so.

## How it is held to account

`check`'s client is written by hand from the specification and shares no code with
the validator that judges it, so the two are independent readings. In CI
(`cargo xtask exerciser`, a leg of `cargo xtask conformance`) `check --call-all` runs
against this repository's everything server over stdio and HTTP at both revisions,
and each recording must judge clean under `validate --strict` with *no* party view: a
finding held against the client there is a defect in `check`, and fails the build.
The design and what it deliberately leaves out are
[ADR-0024](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0024-check-an-exerciser-that-does-only-what-a-client-does.md).
