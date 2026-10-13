<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# The workbench

`mcp-conformance serve` is a local web page over a **library** of recordings: search them,
read one as a timeline with each finding at its event, compare two, follow one that is still
being recorded, see what the library as a whole has exercised, and administer it — import,
tag, expire, delete, export, and make redacted copies to share. It is
[ADR-0021](../../docs/plan/decisions/0021-a-workbench-people-keep-open.md)'s decision 4; it
runs on your machine, reads and writes one directory, and sends nothing anywhere.

```text
mcp-conformance serve --library ./recordings
mcp-conformance serve: library ./recordings (12 recordings)
mcp-conformance serve: open http://127.0.0.1:41245/?token=ae0c…4f71
mcp-conformance serve: the link works once; Ctrl-C stops the workbench
```

Open the link (`--open` does it for you). It works **once**: the first visit trades the
token for a cookie that only that browser holds, and the address bar is redirected to a URL
without it. To open the workbench in another browser, restart it.

## The library

A library is a directory of traces, `<id>.jsonl`, each with an optional sidecar,
`<id>.meta.json`:

```json
{
  "format": 1,
  "tags": ["nightly", "sdk-go"],
  "source": { "kind": "imported", "importer": "mcpsnoop", "imported_from": "session.jsonl",
              "not_recorded": ["… what the source format does not record …"] },
  "added_at": "2026-10-13T09:12:00Z",
  "recorded_at": "2026-10-12T17:40:03Z",
  "verdict_cache": { "sha256": "…", "tool_version": "0.6.0", "reports": [ … ] }
}
```

The verdict is `mcp-trace-validator`'s, by the same rules `validate` applies; the cache is
used only when it was computed from the same bytes (SHA-256) by the same version, so editing a
trace or upgrading recomputes it. Point a recorder at the directory — `mcp-trace-capture
-o 'recordings/{session}.jsonl' …` — and the recording appears on the next page load. An id is
the file stem, limited to ASCII letters, digits, `.`, `_` and `-`; any other `.jsonl` file is
listed as not opened, and is never read.

## Searching

The search box takes terms separated by spaces; every term must match.

| Term | Finds the recordings that… |
|------|----------------------------|
| `method:tools/call` | sent or received that method |
| `tool:echo`, `prompt:greeting`, `resource:test://static-text` | called, got, read or listed it |
| `session:3f2a…` | carried that `Mcp-Session-Id` (or are that library entry) |
| `code:-32602` | hold a JSON-RPC error with that code |
| `req:BASE-003` | have a finding against that requirement |
| `server:everything`, `tag:nightly` | were recorded against that server, or carry that tag |
| `verdict:fail` | have that verdict: `pass`, `warn`, `fail`, `unsupported`, `unjudged` |
| `timeout`, `"two words"` | hold the text in a finding, a name, a tag or any value above |

A value ending in `*` matches by prefix (`method:resources/*`, `req:TRAN-*`); a leading `-`
excludes (`-tag:ci`); quotes hold spaces. Results are ranked — a field term counts for more
than free text, and more occurrences for more — and each links to the events it matched.

## A session

The session view is the HTML report's timeline, drawn with the report's own stylesheet:
every event in order, requests paired with their answers (with the latency when the trace
records timestamps), notifications, HTTP exchanges and their headers, transport lifecycle.
Findings are listed beside it; **Go to event** opens the event's inspector (its full payload,
up to 16 KiB shown), and each finding marked on an event links back. **Only events with
findings** filters the timeline; **HTML report** downloads the same page
`--format html` writes, to attach to a bug report.

**Follow live** polls the trace file once a second and adds what was appended, re-judging
as it grows: point it at a trace a recorder is still writing. A last line not yet finished
is left for the next poll.

Where the recording came from is stated above the verdict. For an import, that includes
**what the source does not record** — the clauses that would need it read *not observed*, and
nothing is filled in. For a redacted copy, it names the original and each rule applied.

## Comparing two sessions

Select two recordings and **Compare** (or **Compare with…** on a session). The diff lists
every clause whose outcome or number of findings differs at a revision both were judged at,
then the messages aligned by direction, method (a response takes its request's) and id, each
pair the same or changed, each unpaired message only in A or only in B; a changed pair opens
to both payloads. Recordings of the same client against two servers — two SDK versions, say —
align message for message.

## Dashboards

Verdicts by the day recorded, clause coverage per revision (how many of the registry's clauses
some recording judged, beside those the registry excludes from judging), and findings by area
and by server. Every number is reproducible from the command line, and the page names the
member of the output each chart draws:

```text
mcp-conformance library --library ./recordings stats --format json
```

prints exactly — byte for byte, which a test holds — what the dashboards read.

## Administration

Each action is a button in the page and a `library` subcommand:

| Action | In the page | From the command line |
|--------|-------------|-----------------------|
| Import a trace, an mcpsnoop capture or an MCP Inspector export | **Import…** | `library import FILE [--from mcpsnoop\|inspector\|trace] [--transport stdio\|streamable-http]` |
| Tag | **Edit tags** | `library tag ID --tag T…` |
| Expire old recordings | **Retention…** | `library prune --retain-days N [--apply]` |
| Delete | **Delete** | `library delete ID…` |
| Export (tar of traces and sidecars) | **Export** | `library export ID… -o FILE` |
| Redact | **Redact…** | `library redact ID [--header H…] [--pointer P…] [--meta] [--string S…]` |

**Redaction** makes a new recording and leaves the original alone. It replaces with
`"[redacted]"` the values of headers named (by default `Authorization`, `Cookie`, `Set-Cookie`,
`Proxy-Authorization`), of JSON pointers into every message (`/params/arguments/apiKey`; `*`
matches every member, as in `/result/content/*/text`), of every `_meta` member, and every
occurrence of a literal string anywhere. A line that is not JSON stops the redaction rather
than being copied. Member order within a line is not kept. Read the copy before sharing it:
redaction removes what you name, and a recording holds whatever the session sent.

Retention deletes recordings made more than N days ago; one with no date (no `recorded_at`,
no timestamps, no `added_at`, no file time) is never selected.

## Importers

- **mcpsnoop** (its JSON Lines capture, read from captures 0.26.1 wrote): every frame becomes a
  message, a batch one array again; over HTTP, each exchange's first frame is preceded by the
  request (a POST, with the headers mcpsnoop kept) or the response status. It does not record
  `Host` or `Mcp-Session-Id`, response headers but `Content-Type`, transport lifecycle, or the
  method and time of a request without a body (a GET stream, a DELETE), which is placed just
  before the server's first frame on it with its method unstated.
- **MCP Inspector** (the web client's protocol export, `inspector-protocol-….json`, read from
  `@modelcontextprotocol/inspector` 2.10.1): every message becomes an event, an answer folded
  into its request's entry placed at the request's time plus the recorded duration. It does
  not record the transport (say it: `--transport`), anything of HTTP, lifecycle, or messages
  the Inspector's log had dropped (it keeps the latest 1000 by default).

## Security

The library can hold credentials, and a local web server is reachable by every page your
browser opens. So the workbench binds `127.0.0.1` (another address needs
`--dangerously-listen-non-loopback`, and it warns); answers only requests whose `Host` is its
own address — a DNS-rebound name is refused — and whose `Origin`, when present, is its own; needs
the session cookie on every request; accepts a change only as a JSON body carrying its
`Origin`; sends a Content Security Policy that allows no inline script or style and no
connection elsewhere; and puts every string from a recording on the page as text, with
control and bidirectional characters shown as `\u{…}`. The
[security model](../../docs/plan/05-security-model.md) lists the threats and the tests.

The page and its API are for people, not a stable interface
([STABILITY.md](../../STABILITY.md)); `library stats --format json` is the way to read the
numbers from a script.
