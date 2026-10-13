<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

[![CI](https://github.com/tomtom215/mcp-conformance/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/tomtom215/mcp-conformance/actions/workflows/ci.yml?query=branch%3Amain)
[![crates.io](https://img.shields.io/crates/v/mcp-trace-validator.svg)](https://crates.io/crates/mcp-trace-validator)
[![docs.rs](https://img.shields.io/docsrs/mcp-trace-validator)](https://docs.rs/mcp-trace-validator)
[![MSRV](https://img.shields.io/crates/msrv/mcp-trace-validator)](STABILITY.md)
[![License](https://img.shields.io/crates/l/mcp-trace-validator)](LICENSE)

**Clause-by-clause conformance checking for the [Model Context Protocol](https://modelcontextprotocol.io).**
Record any MCP session — any language, any SDK, stdio or streamable HTTP — and find
out which of the specification's requirements it met, which it broke, where, and
which it never exercised.

**Status: `0.6.0`, on [crates.io](https://crates.io/crates/mcp-trace-validator).**
Pre-1.0: the API and the verdicts may change between minor releases, and the
[changelog](CHANGELOG.md) says so when they do.

> **This README describes `main`**, which is ahead of the latest release:
> `cargo install` gets `0.6.0`. Documentation for the release is at
> <https://mcp-conformance.com>, and for `main` at <https://mcp-conformance.com/dev/>.
>
> **Not yet released** (on `main` since `0.6.0`; the
> [changelog's `Unreleased` section](CHANGELOG.md#unreleased) has the full list):
>
> - the `incomplete` verdict for a run that could not judge one of its traces;
> - the cap of 20 findings per clause in the human report;
> - release binaries built with `cargo auditable`, for `cargo audit bin`;
> - the `requirements <ID>` view, its `--area`/`--level`/`--checked`/`--excluded`
>   filters, and the `completions` subcommand;
> - the `2026-07-28` registry rows added since, so the example totals and the
>   coverage table below are `main`'s, not `0.6.0`'s;
> - the wire schema — every message validated against its revision's JSON Schema —
>   and the five clauses it made judgeable;
> - the container image (`ghcr.io/tomtom215/mcp-conformance`, first published by
>   the next release) and the GitHub Action's `capture` input, which records the
>   sessions it then judges.

## Quickstart

```text
cargo install mcp-trace-capture mcp-trace-validator
```

`cargo binstall mcp-trace-capture mcp-trace-validator` skips the compile: each
release attaches prebuilt archives for Linux (static, x86_64 and
aarch64), macOS (arm64 and x86_64) and Windows (x86_64), with SLSA build-provenance
attestations (`gh attestation verify <archive> --repo tomtom215/mcp-conformance`)
and `SHA256SUMS-binaries` beside them. The binaries embed their dependency list
(`cargo auditable`), so `cargo audit bin` checks them against RustSec.

Record a session. For a **stdio** server, configure your client to launch the
capture wrapper instead of the server:

```text
mcp-trace-capture -o session.jsonl stdio -- python my_server.py
```

For a **streamable-HTTP** server, put the proxy in front of it and point your client
at the proxy:

```text
mcp-trace-capture -o session.jsonl http --upstream http://localhost:3000
# client URL: http://127.0.0.1:8080/mcp
```

Then judge it:

```text
mcp-trace-validator validate session.jsonl
```

In CI, the exit code is the verdict (`0` pass, `1` a clause failed — or warned, with
`--strict` — `2` bad invocation or nothing judgeable or output not written, `3`
malformed trace); `validate` takes any number of traces, and a trace it cannot judge
is listed in every format's report, whose verdict then reads `incomplete`.
`--format junit` produces a test report, and `--format sarif` puts each finding on
the trace line it concerns in GitHub code scanning (or any SARIF viewer).
[`mcp-trace-capture`'s README](crates/mcp-trace-capture/README.md) covers what the
recorder guarantees and what it leaves out.

On GitHub Actions, the repository is itself an action: record the traces in an earlier
step, then

```yaml
- uses: tomtom215/mcp-conformance@<commit or tag>   # builds the CLI from that ref
  with:
    traces: traces/*.jsonl                           # writes mcp-conformance.sarif and a JUnit file
- uses: github/codeql-action/upload-sarif@<sha>      # needs security-events: write
  if: always()
  with:
    sarif_file: mcp-conformance.sarif
```

Or let the action record them: with `capture: stdio` (or `http`), it runs your
`client` command against your `server` through `mcp-trace-capture`, then judges
every session recorded:

```yaml
- uses: tomtom215/mcp-conformance@<commit or tag>
  with:
    capture: stdio
    server: python my_server.py
    client: python my_client.py --server-cmd "$MCP_SERVER_COMMAND"
```

[`action.yml`](action.yml) lists the inputs (`revision`, `strict`, a released `version`
to download instead of building, the capture inputs) and outputs; the `action` job in
[CI](.github/workflows/ci.yml) runs it on every push, recording sessions over both
transports. The book's [CI chapter](book/src/ci.md#github-actions) has the details.

Both CLIs also ship as a container image, from the release that follows `0.6.0` on
([`Dockerfile`](Dockerfile); distroless, non-root, no shell; linux/amd64):

```text
docker run --rm -v "$PWD:/work" ghcr.io/tomtom215/mcp-conformance:<version> \
  mcp-trace-validator validate session.jsonl
```

## See it work

A client that retries a multi-round-trip request with the id of the original — the
two are independent requests under `2026-07-28` and must not share one:

```text
$ mcp-trace-validator validate session.jsonl
MCP trace validation — revision 2026-07-28 (declared by the trace)
  FAIL  MRTR-019 (MUST)
        seq 2: the retry reuses id 1 from the request at seq 0; the two are independent requests and must not share one
        spec: "The JSON-RPC `id` MUST be different between the initial request and the retry, as they are independent requests."
        see:  https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr#client-requirements-basic-workflow
  FAIL  TOOL-023 (MUST)
        seq 2: the retry reuses id 1 from the request at seq 0; the two are independent requests and must not share one
        spec: "Note that the JSON-RPC `id` MUST be different between the initial request and the retry."
        see:  https://modelcontextprotocol.io/specification/2026-07-28/server/tools#input-required-tool-results
totals: 39 pass, 2 fail, 0 warn, 165 excluded, 0 unsupported, 0 not applicable, 104 not observed
wire schema: pass (3 messages valid)
verdict: fail
```

- **Every finding cites the clause it breaks** — the event (`seq`), what was wrong,
  the clause verbatim, and a link to it in the published revision; JSON, JUnit and
  SARIF output carry the same. The weekly spec-drift job re-verifies every quote against
  the spec's source and every link's anchor against the published page.
- **The revision is the one the session declares** — in `initialize`, in each
  request's `_meta`, or in the `MCP-Protocol-Version` header. A trace declaring no
  revision is judged against the newest one (and says so); a trace declaring only a
  revision this build does not know is refused rather than judged against the wrong
  rules. `--revision` overrides; naming two judges the session under both, clause by
  clause.
- **Not observed is not a pass.** A clause the session never came near is reported
  *not observed*; one gated on a capability nobody negotiated is *not applicable*;
  one no trace can judge is *excluded*, with the reason. The not-observed rows above are clauses
  this short session never exercised — reporting them as passes would be a score, not
  a verdict.
- **Every message is also held to the revision's own JSON Schema** — the
  specification's `schema.json`, vendored at a pinned commit and re-verified weekly —
  so a missing required member, a wrong type or a value outside an enumeration fails
  the report even where no sentence of the prose names it. It is reported beside the
  clause rows as the wire schema, not counted in their totals
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

| Crate | What it gives you |
|-------|-------------------|
| [`mcp-trace-capture`](https://crates.io/crates/mcp-trace-capture) | **The recorder.** A stdio wrapper and an HTTP reverse proxy (SSE and `https://` included) that forward bytes unchanged and write a validator-ready trace, recording each message before the bytes that complete it are forwarded. CI runs reference-host sessions through it at both revisions and requires them to judge clean, and runs the official suite through the proxy. |
| [`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator) | **The validator and its CLI.** Findings with the clause ID and the offending event `seq`, as human text (what needs attention, or `--all`), JSON (with a published JSON Schema), JUnit, or SARIF, with documented exit codes. Every check is falsified by at least one committed violation trace in [`corpus/`](corpus) — a check that cannot fail is not a check. |
| [`mcp-conformance-core`](https://crates.io/crates/mcp-conformance-core) | **The spec as data.** Requirement registries for `2025-11-25` and `2026-07-28` whose every entry carries a verbatim spec quote, an RFC 2119 level, an optional capability gate, and either a mechanical check or a documented exclusion (the SEP-2484 traceability shape); a weekly job re-verifies every quote against the published text. Plus the JSON Lines trace schema and RFC 8785 canonical JSON. Serde only. |
| [`mcp-everything-server`](https://crates.io/crates/mcp-everything-server) | **The calibration subject**, on [rmcp](https://github.com/modelcontextprotocol/rust-sdk). It passes the pinned official suite's `2025-11-25` server surface — **40/40 checks** — over stdio and policy-gated streamable HTTP, with a default-secure `Host`/`Origin` policy. `--protocol-version 2026-07-28` serves the stateless revision; the suite's pre-release `2026-07-28` scenarios score **42 passing / 0 failing** against it, and five committed captures evidence **119 of the 145 judgeable clauses** between them. Its tap records each suite session for the calibration check. |
| [`mcp-reference-host`](https://crates.io/crates/mcp-reference-host) | **The reference client.** Passes all four of the official suite's `2025-11-25` client scenarios at the pinned version; bounded tool-use loops over stdio and streamable HTTP, scriptable sampling / elicitation / roots with no model-provider network use. |

**Calibration.** On every CI run the official suite (pinned `0.1.16`, `2025-11-25`)
drives the everything server and the reference host, the tapped sessions replay
through the validator, and any MUST-level disagreement not triaged in a committed
ledger fails the build. The `2026-07-28` surface is measured weekly against the
suite's pinned pre-release (`0.2.0-alpha.12`) as a ratchet, not yet as a blocking
agreement check: the suite has no stable `2026-07-28` release to calibrate against.

## Requirement coverage

Generated from the registries by `cargo xtask coverage` and verified in CI:

<!-- coverage:begin (generated by `cargo xtask coverage`; do not edit by hand) -->
**`2026-07-28` — current revision**

| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 59 | 28 | 31 | 0 |
| TRAN | 88 | 44 | 44 | 0 |
| DISC | 4 | 2 | 2 | 0 |
| VERS | 8 | 5 | 3 | 0 |
| MRTR | 25 | 15 | 10 | 0 |
| SUBS | 7 | 4 | 3 | 0 |
| CANC | 17 | 3 | 14 | 0 |
| PROG | 8 | 5 | 3 | 0 |
| CACH | 19 | 4 | 15 | 0 |
| COMP | 6 | 2 | 4 | 0 |
| PAGE | 6 | 1 | 5 | 0 |
| LOG | 8 | 4 | 4 | 0 |
| TOOL | 31 | 15 | 16 | 0 |
| RES | 13 | 6 | 7 | 0 |
| PROM | 11 | 7 | 4 | 0 |
| **Total** | **310** | **145** | **165** | **0** |

310 requirements: 145 judged by 112 distinct trace checks, 165 carrying a documented exclusion that explains why a recorded trace cannot judge them.

**`2025-11-25`**

| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 25 | 13 | 12 | 0 |
| LIFE | 18 | 10 | 8 | 0 |
| TRAN | 49 | 13 | 36 | 0 |
| TOOL | 15 | 9 | 6 | 13 |
| RES | 10 | 3 | 7 | 6 |
| PROM | 10 | 5 | 5 | 7 |
| LOG | 5 | 1 | 4 | 4 |
| COMP | 5 | 1 | 4 | 3 |
| PAGE | 5 | 2 | 3 | 0 |
| **Total** | **142** | **57** | **85** | **33** |

142 requirements: 57 judged by 54 distinct trace checks, 85 carrying a documented exclusion that explains why a recorded trace cannot judge them.

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
        seq 3: request "tools/list" reuses id 1, already used by the same party at seq 0
        spec: "The request ID MUST NOT have been previously used by the requestor within the same session."
        see:  https://modelcontextprotocol.io/specification/2025-11-25/basic#requests
totals: 17 pass, 1 fail, 0 warn, 85 excluded, 0 unsupported, 6 not applicable, 33 not observed
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
