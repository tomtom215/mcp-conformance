<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

[![CI](https://github.com/tomtom215/mcp-conformance/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/tomtom215/mcp-conformance/actions/workflows/ci.yml?query=branch%3Amain)
[![crates.io](https://img.shields.io/crates/v/mcp-trace-validator.svg)](https://crates.io/crates/mcp-trace-validator)
[![docs.rs](https://img.shields.io/docsrs/mcp-trace-validator)](https://docs.rs/mcp-trace-validator)
[![MSRV](https://img.shields.io/crates/msrv/mcp-trace-validator)](STABILITY.md)
[![License](https://img.shields.io/crates/l/mcp-trace-validator)](LICENSE)

**Find out which sentences of the [Model Context Protocol](https://modelcontextprotocol.io)
specification your MCP server keeps, which it breaks, and which it was never asked
about.** One command, no client to write, no SDK, no Node.js:

```text
mcp-conformance check -- python my_server.py          # a stdio server
mcp-conformance check --url http://localhost:3000/mcp  # a Streamable HTTP server
```

`check` talks to the server the way a conforming client does, records every byte
through a tap, and judges the recording against the specification clause by clause,
for the `2025-11-25` and `2026-07-28` revisions. Each finding names the requirement,
the message that broke it, the specification's own sentence, and a link to it.
The exit code is the verdict, so the same command is a CI gate.

**Status: `0.6.0`, on [crates.io](https://crates.io/crates/mcp-trace-validator).**
Pre-1.0: the API and the verdicts may change between minor releases, and the
[changelog](CHANGELOG.md) says so when they do.

> **This README describes `main`, which is ahead of `0.6.0`.** `mcp-conformance`
> (`check`, `test`, `watch`, `serve`), baselines, the party view, the wire schema,
> the container image and the npm and PyPI packages ship with the next release;
> until then, build from this repository
> (`cargo install --path crates/mcp-conformance-cli`). The
> [changelog's `Unreleased` section](CHANGELOG.md#unreleased) lists everything, and
> the book for `main` is at <https://mcp-conformance.com/dev/>.

## Contents

- [Check a server](#check-a-server) · [Use it in CI](#use-it-in-ci) ·
  [Judge traffic you already have](#judge-traffic-you-already-have) ·
  [Install](#install)
- [What a report looks like](#see-it-work) ·
  [How this relates to other tools](#how-this-relates-to-other-tools) ·
  [The toolkit](#the-toolkit)
- [Requirement coverage](#requirement-coverage) · [The trace format](#the-trace-format) ·
  [Documentation](#documentation)

## Check a server

```text
$ mcp-conformance check -- node node_modules/@modelcontextprotocol/server-memory/dist/index.js
mcp-conformance check: node node_modules/@modelcontextprotocol/server-memory/dist/index.js (stdio)
  revision   2025-11-25 — `server/discover` was not answered as a 2026-07-28 server answers it, so the session fell back to `initialize`
  server     memory-server 1.0.0
  tools      9 listed, 3 called
  resources  1 listed, 1 read
  prompts    not declared by the server, so not asked for
  not called:
    create_entities, create_relations, add_observations, delete_entities, delete_observations, delete_relations — not annotated read-only (`readOnlyHint: true`); name it with --call, or use --call-all
  requests   10 sent
  trace      /tmp/mcp-conformance-check-31192-984222902/session.jsonl
MCP trace validation — revision 2025-11-25 (declared by the trace)
  WARN  BASE-086 (SHOULD)
        [server] seq 9: the server sent tool "create_entities"'s inputSchema declaring `$schema` "http://json-schema.org/draft-07/schema#"; JSON Schema 2020-12 (https://json-schema.org/draft/2020-12/schema) is the recommended dialect
        ...
totals: 35 pass, 0 fail, 1 warn, 246 excluded, 0 unsupported, 4 not applicable, 62 not observed
wire schema: pass (21 messages valid)
verdict (server findings only): pass-with-warnings
```

(A run on 2026-11-19 against the official `server-memory` package as npm served it
that day; the `...` stands for seventeen more findings of the same kind, one for each
schema the server's nine tools declare.)

- **It is safe to point at a real server.** `check` lists what the server declares,
  reads the resources it lists, gets the prompts that need no arguments, and calls
  only the tools the server itself annotates `readOnlyHint: true`. Everything else is
  named with the reason it was left alone. `--call <tool>` and `--call-all` call
  more, and the side effects are then yours.
- **It never guesses.** Arguments are built from the tool's own `inputSchema` and
  checked against it; where the schema does not say enough, the tool is skipped and
  said so. Every request is one a conforming client sends: nothing malformed, nothing
  the server did not list. `check` is not a fuzzer or a security scanner.
- **It is bounded.** At most 100 requests, 25 tool calls, 30 seconds an answer and
  five minutes in all by default; each bound is a flag, and the summary says when one
  stopped the run.
- **It says what it did not see.** The lines above the report are the run's own
  account. A clean report over ten requests is a claim about ten requests: the
  clauses the session never reached are counted *not observed*, never passed.
- **The recording is yours.** The trace is kept (`-o` names it); `mcp-conformance
  validate` re-judges it, `--format html` renders it as one page, and
  `mcp-conformance serve` opens it in a local workbench.

`--header "Authorization: Bearer …"` sends a credential to an HTTP server (it is not
written to the trace), `--spec-version` picks a revision instead of negotiating, and
`--format json|junit|sarif|html` changes the report. The book's
[Checking a server](book/src/checking.md) chapter has the rest.

## Use it in CI

The exit status is the verdict: `0` nothing held against the server (warnings pass
unless `--strict`), `1` a clause failed, `2` the run could not be made or judged.

```yaml
- run: mcp-conformance check --format sarif --strict -- python my_server.py > mcp.sarif
- uses: github/codeql-action/upload-sarif@<sha>      # each finding on the trace line it concerns
  if: always()
  with:
    sarif_file: mcp.sarif
```

**Adopting it on a server that already has findings.** Record them once, then fail
only on new ones; the accepted findings stay in every report, marked:

```text
mcp-conformance check --write-baseline mcp-baseline.json -- python my_server.py
mcp-conformance check --baseline mcp-baseline.json --strict -- python my_server.py
```

An entry that no longer matches anything fails the run, so a baseline cannot outlive
the defect it excuses ([ADR-0025](docs/plan/decisions/0025-a-baseline-for-validate.md)).

The repository is also a GitHub Action that judges traces you recorded, or records
them itself from your own client and server; [`action.yml`](action.yml) lists its
inputs and the book's [CI chapter](book/src/ci.md) has the details.

## Judge traffic you already have

`check` exercises a server with a client of its own. To judge what *your* client and
server said to each other, in any language, record the session and validate it:

```text
mcp-trace-capture -o session.jsonl stdio -- python my_server.py     # launch this instead of the server
mcp-trace-capture -o session.jsonl http --upstream http://localhost:3000   # or point the client at this proxy
mcp-conformance validate session.jsonl
```

The recorder forwards bytes unchanged and links no MCP SDK, so the verdict describes
the wire. `mcp-conformance watch` does both at once and prints each finding as the
exchange that shows it completes; `mcp-conformance test` runs the official
conformance suite through the same tap and reports whether the suite and the clause
report agree. [`mcp-trace-capture`'s README](crates/mcp-trace-capture/README.md) says
what the recorder guarantees and what it leaves out.

## Install

Released today (`0.6.0`), the recorder and the validator:

```text
cargo install mcp-trace-capture mcp-trace-validator
cargo binstall mcp-trace-capture mcp-trace-validator   # prebuilt, no compile
```

Each release attaches archives for Linux (static, x86_64 and aarch64), macOS (arm64
and x86_64) and Windows (x86_64), with SLSA build-provenance attestations
(`gh attestation verify <archive> --repo tomtom215/mcp-conformance`) and
`SHA256SUMS-binaries`.

From the next release, `mcp-conformance` is in the same archives and the same
binaries install with no compiler from npm (`npx mcp-conformance`), PyPI
(`pip install mcp-conformance`) and a container image
(`ghcr.io/tomtom215/mcp-conformance`). **None of those three is published yet**;
until it is, those commands fail or find nothing.

## See it work

A client that retries a multi-round-trip request with the id of the original — the
two are independent requests under `2026-07-28` and must not share one:

```text
$ mcp-trace-validator validate session.jsonl
MCP trace validation — revision 2026-07-28 (declared by the trace)
  FAIL  MRTR-019 (MUST)
        [client] seq 2: the retry reuses id 1 from the request at seq 0; the two are independent requests and must not share one
        spec: "The JSON-RPC `id` MUST be different between the initial request and the retry, as they are independent requests."
        see:  https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr#client-requirements-basic-workflow
  FAIL  TOOL-023 (MUST)
        [client] seq 2: the retry reuses id 1 from the request at seq 0; the two are independent requests and must not share one
        spec: "Note that the JSON-RPC `id` MUST be different between the initial request and the retry."
        see:  https://modelcontextprotocol.io/specification/2026-07-28/server/tools#input-required-tool-results
  NOTE  1 request had no response when the trace ended (the recording simply stopped).
        JSON-RPC expects every request answered; no clause of this revision judges it, so the verdict is unchanged.
        client "tools/call" id 1 at seq 2
totals: 46 pass, 2 fail, 0 warn, 320 excluded, 0 unsupported, 0 not applicable, 133 not observed
wire schema: pass (3 messages valid)
verdict: fail
```

- **Every finding cites the clause it breaks** — the event (`seq`), the party it is
  held against (`[client]`, `[server]`), what was wrong, the clause verbatim, and a
  link to it in the published revision; JSON, JUnit and SARIF output carry the same.
  `--party server` counts only the server's findings toward the verdict and exit
  status — for a server the official suite's client drove, whose own slips (no
  `notifications/initialized`, `logging/setLevel` without the capability) are not
  the server author's to fix — and the verdict line says so. The weekly spec-drift
  job re-verifies every quote against the spec's source and every link's anchor
  against the published page.
- **A request left unanswered is noted, never judged.** JSON-RPC expects every
  request answered, but no judgeable clause of either revision says so (`BASE-012`
  is excluded), so a server that exits with a request pending gets a `NOTE` — how
  many, up to ten by method, id and `seq`, and whether the trace ended with a
  transport close, an abort, or simply stopped — carried in every format, and the
  verdict it would otherwise have earned.
- **The revision is the one the session declares** — in `initialize`, in each
  request's `_meta`, or in the `MCP-Protocol-Version` header. A session that
  completed a handshake ran under the revision the server's `initialize` result
  names — that is what negotiation means — so it is judged under that one, with a
  note on stderr when the client had proposed another; a negotiated revision this
  build does not know is refused rather than judged under the proposal. A trace
  declaring no revision is judged against the newest one (and says so); a trace
  declaring only a revision this build does not know is refused rather than judged
  against the wrong rules. `--revision` overrides; naming two judges the session
  under both, clause by clause.
- **Not observed is not a pass.** A clause the session never came near is reported
  *not observed*; one gated on a capability nobody negotiated is *not applicable*;
  one no trace can judge is *excluded*, with the reason. The not-observed rows above are clauses
  this short session never exercised — reporting them as passes would be a score, not
  a verdict.
- **Every message is also held to the revision's own JSON Schema** — the
  specification's `schema.json`, vendored at a pinned commit and re-verified weekly —
  so a missing required member, a wrong type, a value outside an enumeration or image
  `data` that is not base64 (the schema's `byte` format, the one format it asserts)
  fails the report even where no sentence of the prose names it. It is reported beside
  the clause rows as the wire schema, not counted in their totals
  ([ADR-0020](docs/plan/decisions/0020-wire-schema-validation.md)).
- **An empty recording is a bad invocation**, not a pass: the shape a broken capture
  step produces should fail the build.

The report lists what needs attention, with the first 20 findings under each
clause; `--all` lists every clause with its outcome, every finding and, for
exclusions, the reason. `--format json` gives the whole report as data.

## How this relates to other tools

Surveyed 2026-09-24; details and sources in the
[ecosystem register](docs/plan/01-ecosystem-context.md).

| | Judges | Unit of verdict |
|---|---|---|
| [Official conformance suite](https://github.com/modelcontextprotocol/conformance) — the authority | A live server or client, by driving scenarios; also validates every message against the revision's JSON schema | Scenario and check, with per-revision required sets and per-SEP traceability |
| [`mcpsnoop`](https://github.com/kerlenton/mcpsnoop) | Its own recorded captures, offline | A fixed set of MUST rules, reported as text, JUnit or SARIF |
| **This project** | Any recorded session, offline, from any implementation | One row per normative clause of the spec — each with a stable ID, its verbatim quote in the registry, and either a check or a written reason it cannot be judged — continuously calibrated against the official suite |

The official suite remains the authority on what "conformant" means; this project
does not replace it. What it adds is the offline, clause-level reading: *which*
sentence of the specification a session broke, judged from a recording of real
traffic, with every clause it could not see accounted for.

## The one idea: capture, then judge

The validator is a **pure function** — a slice of trace events in, a report out —
with no network, no clock, and no I/O of its own. The capture tool (or the reference
server's tap, or the reference host) records the trace and assigns the ordering; the
validator only judges it. Neither the validator nor the capture tool links an MCP
SDK, so a verdict describes the bytes on the wire rather than one SDK's reading of
them.

That separation buys determinism and replayability: the same trace yields a
byte-identical report on any platform, big-endian and 32-bit included — a regression
is a diff, not a flake. The design and its trade-offs are written up for an external
audience in [docs/design/trace-validation.md](docs/design/trace-validation.md).

## The toolkit

| Crate | What it is |
|-------|-----------|
| [`mcp-conformance-cli`](crates/mcp-conformance-cli) | **The front door** (binary `mcp-conformance`; on `main`, not yet released). `check` exercises a server and reports; `validate` and `requirements` are the validator's; `watch` records and judges live; `test` runs the pinned official suite through the tap and reconciles the two verdicts; `serve` is a local, loopback-only workbench over a library of recordings ([book](book/src/workbench.md)). |
| [`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator) | **The judge**, as a library and a CLI: a pure function from trace events to a report, in human text, JSON (with a published JSON Schema), JUnit, SARIF or HTML. Every check is falsified by at least one committed violation trace in [`corpus/`](corpus). |
| [`mcp-trace-capture`](https://crates.io/crates/mcp-trace-capture) | **The recorder**: a stdio wrapper and an HTTP reverse proxy (SSE and `https://` included) that forward bytes unchanged and write the trace. |
| [`mcp-conformance-core`](https://crates.io/crates/mcp-conformance-core) | **The specification as data**: one registry entry per normative clause, with its verbatim quote, RFC 2119 level and either a check or the written reason no recording can judge it. A weekly job re-verifies every quote against the published text. Serde only. |
| [`mcp-everything-server`](https://crates.io/crates/mcp-everything-server) | **The calibration subject**, on [rmcp](https://github.com/modelcontextprotocol/rust-sdk): passes the pinned official suite's `2025-11-25` server surface (**40/40 checks**) and scores **42 passing / 0 failing** on the suite's pre-release `2026-07-28` scenarios; the eight committed captures at that revision evidence **148 of the 181 judgeable clauses** between them. |
| [`mcp-reference-host`](https://crates.io/crates/mcp-reference-host) | **The reference client**, on rmcp: passes the official suite's four `2025-11-25` client scenarios; scriptable sampling, elicitation and roots. |

**Calibration.** On every CI run the official suite (pinned `0.1.16`, `2025-11-25`)
drives the everything server and the reference host, the tapped sessions replay
through the validator, and any MUST-level disagreement not triaged in a committed
ledger fails the build; so does any message the validator's wire-schema check and the
suite's `wire-schema-valid` disagree about, wherever the suite reports that check. The `2026-07-28` surface is measured weekly against the
suite's pinned pre-release (`0.2.0-alpha.12`) as a ratchet, not yet as a blocking
agreement check: the suite has no stable `2026-07-28` release to calibrate against.

## Requirement coverage

Generated from the registries by `cargo xtask coverage` and verified in CI:

<!-- coverage:begin (generated by `cargo xtask coverage`; do not edit by hand) -->
**`2026-07-28` — current revision**

| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 60 | 31 | 29 | 0 |
| TRAN | 90 | 47 | 43 | 0 |
| DISC | 4 | 3 | 1 | 0 |
| VERS | 9 | 5 | 4 | 0 |
| MRTR | 25 | 15 | 10 | 0 |
| SUBS | 7 | 4 | 3 | 0 |
| CANC | 17 | 3 | 14 | 0 |
| PROG | 8 | 5 | 3 | 0 |
| CACH | 19 | 4 | 15 | 0 |
| COMP | 6 | 2 | 4 | 0 |
| PAGE | 6 | 1 | 5 | 0 |
| LOG | 8 | 4 | 4 | 0 |
| TOOL | 31 | 17 | 14 | 0 |
| RES | 13 | 6 | 7 | 0 |
| PROM | 11 | 7 | 4 | 0 |
| ELIC | 46 | 7 | 39 | 0 |
| SAMP | 23 | 10 | 13 | 0 |
| ROOT | 9 | 3 | 6 | 0 |
| AUTH | 109 | 7 | 102 | 0 |
| **Total** | **501** | **181** | **320** | **0** |

501 requirements: 181 judged by 144 distinct trace checks, 320 carrying a documented exclusion that explains why a recorded trace cannot judge them.

**Extensions attached to `2026-07-28` — reported beside the verdict, never scored ([ADR-0023](docs/plan/decisions/0023-extensions-are-reported-never-scored.md))**

| Extension | Requirements | Checked | Excluded |
|-----------|-------------:|--------:|---------:|
| `io.modelcontextprotocol/tasks` | 49 | 25 | 24 |
| `io.modelcontextprotocol/skills` | 100 | 35 | 65 |
| **Total** | **149** | **60** | **89** |

149 extension requirements: 60 judged by 50 distinct trace checks, 89 carrying a documented exclusion. A report lists an extension's outcomes under its `extensions`, with a verdict of its own that the report's verdict and exit status never read.

**`2025-11-25`**

| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 28 | 14 | 14 | 0 |
| LIFE | 20 | 10 | 10 | 0 |
| TRAN | 52 | 14 | 38 | 0 |
| PING | 7 | 1 | 6 | 0 |
| PROG | 11 | 8 | 3 | 0 |
| CANC | 9 | 4 | 5 | 0 |
| TOOL | 18 | 9 | 9 | 13 |
| RES | 10 | 3 | 7 | 6 |
| PROM | 10 | 5 | 5 | 7 |
| LOG | 5 | 1 | 4 | 4 |
| COMP | 5 | 1 | 4 | 3 |
| PAGE | 5 | 2 | 3 | 0 |
| ELIC | 53 | 11 | 42 | 0 |
| SAMP | 20 | 10 | 10 | 0 |
| ROOT | 10 | 4 | 6 | 0 |
| AUTH | 85 | 5 | 80 | 0 |
| **Total** | **348** | **102** | **246** | **33** |

348 requirements: 102 judged by 96 distinct trace checks, 246 carrying a documented exclusion that explains why a recorded trace cannot judge them.

Every check is falsified by a committed violation trace and examines a real subject on at least one conforming one. A requirement is reported *pass* only where the session carried something it binds to: a capability-gated clause the session never negotiated reports *not-applicable*, and a clause whose subject matter never appeared reports *not-observed*. Neither is a vacuous pass.
<!-- coverage:end -->

## The trace format

A trace is JSON Lines: one event per line, each carrying a capture-assigned `seq`
(the only ordering authority), a `direction`, a `transport`, and a `kind` —
`message` events hold the JSON-RPC payload; `http` events record
conformance-relevant headers, a response's status, and a client request's
`method`; `lifecycle` events mark transport open, close and abort. The format is published as
a JSON Schema, [`trace-event.schema.json`](crates/mcp-conformance-core/schema/trace-event.schema.json)
(also `mcp_conformance_core::trace::EVENT_JSON_SCHEMA`), so a recorder written in
any language can check its output without this toolkit; a test holds the schema
and the validator's reader to the same answer on every corpus record. This
`2025-11-25` session reuses a request ID:

<!-- The mdBook chapter book/src/trace-format.md embeds the example below via
     this anchor; readme_examples.rs pins it to the validator's real output. -->
<!-- ANCHOR: trace-example -->
```jsonl
{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"my-host","version":"1.0.0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"my-server","version":"1.0.0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}
```

and the validator answers with the violated clause, addressed to the offending
event:

```text
  FAIL  BASE-003 (MUST NOT)
        [client] seq 3: request "tools/list" reuses id 1, already used by the same party at seq 0
        spec: "The request ID MUST NOT have been previously used by the requestor within the same session."
        see:  https://modelcontextprotocol.io/specification/2025-11-25/basic#requests
totals: 17 pass, 1 fail, 0 warn, 246 excluded, 0 unsupported, 6 not applicable, 78 not observed
wire schema: pass (4 messages valid)
verdict: fail
```
<!-- ANCHOR_END: trace-example -->

The six not-applicable rows are the capability-gated requirements this session
never negotiated (the resources and prompts clauses), and the
not-observed rows are the clauses whose subject matter never appeared —
nothing was paginated, no binary content was sent, no error was returned.
Neither is reported as a pass. [`corpus/`](corpus) holds complete annotated
sessions for every area.

## Documentation

- **[The book](https://mcp-conformance.com)** — architecture, the trace format, the
  two revisions, the corpus, and conformance results, for the latest release;
  [`/dev/`](https://mcp-conformance.com/dev/) is the same book for `main`
  ([`book/`](book), built on every push), and the version selector switches
  between them and every earlier release.
- **API docs** for every crate on [docs.rs](https://docs.rs/mcp-trace-validator).
- **[STABILITY.md](STABILITY.md)** — what each release keeps stable: verdicts,
  requirement IDs, exit codes, report and trace formats, the MSRV.
- **The plan** — charter, ecosystem register, architecture, conformance strategy,
  engineering standards, security model, roadmap, and decision records — in
  [`docs/plan/`](docs/plan/README.md). The ecosystem register dates every external
  fact it records, and a weekly CI job fails when a row is older than ninety days.
- **The [project review of 2026-09-24](docs/reports/project-review-2026-09-24.md)** —
  a dated review: what was wrong on that date, and the plan it proposed. It is kept as
  written; [the roadmap](docs/plan/06-roadmap.md) records what has been done since.

## Engineering

Held by CI on every push: clippy pedantic + nursery at `-D warnings` on the pinned
stable toolchain and on the MSRV (1.88) across Linux, macOS and Windows; property,
golden-corpus and falsifiability tests; diff-scoped mutation testing with zero
surviving mutants required in shipped crates; `cargo deny`; and a 60-second run of
every fuzz target. Weekly: longer fuzzing, byte-identical reports on big-endian and 32-bit
targets, a build at the oldest dependency versions the manifests claim, quote drift
against the published spec, and the expiry of every dated claim.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has the gates — `cargo xtask ci` runs them all
locally — and [SECURITY.md](SECURITY.md) has the vulnerability process.

## License

[MIT](LICENSE)
