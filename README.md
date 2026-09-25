<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

**Clause-by-clause conformance checking for the [Model Context Protocol](https://modelcontextprotocol.io).**
Record any MCP session — any language, any SDK, stdio or streamable HTTP — and find
out which of the specification's requirements it met, which it broke, where, and
which it never exercised.

**Status: `0.5.1` on [crates.io](https://crates.io/crates/mcp-trace-validator)**
(`cargo install mcp-trace-validator`), published with SLSA build-provenance
attestations. Pre-1.0: the API and the verdicts may change between minor releases,
and the [changelog](CHANGELOG.md) says so when they do. Everything below describes
`main`, including `mcp-trace-capture`, which ships with the next release.

## Quickstart

```text
cargo install mcp-trace-capture mcp-trace-validator
```

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
mcp-trace-validator validate --quiet session.jsonl
```

In CI, `--format junit` produces a test report and the exit code is the verdict
(`0` pass, `1` findings, `2` bad invocation, `3` malformed trace).
[`mcp-trace-capture`'s README](crates/mcp-trace-capture/README.md) covers what the
recorder guarantees and what it leaves out.

## See it work

A client that retries a multi-round-trip request with the id of the original — the
two are independent requests under `2026-07-28` and must not share one:

```text
$ mcp-trace-validator validate --quiet session.jsonl
MCP trace validation — revision 2026-07-28 (declared by the trace)
  FAIL  MRTR-019 (MUST)
        seq 2: the retry reuses id 1 from the request at seq 0; the two are independent requests and must not share one
        spec: "The JSON-RPC `id` MUST be different between the initial request and the retry, as they are independent requests."
        see:  https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr#client-requirements-basic-workflow
  FAIL  TOOL-023 (MUST)
        seq 2: the retry reuses id 1 from the request at seq 0; the two are independent requests and must not share one
        spec: "Note that the JSON-RPC `id` MUST be different between the initial request and the retry."
        see:  https://modelcontextprotocol.io/specification/2026-07-28/server/tools#input-required-tool-results
totals: 37 pass, 2 fail, 0 warn, 147 excluded, 0 unsupported, 0 not applicable, 86 not observed
verdict: fail
```

- **Every finding cites the clause it breaks** — the event (`seq`), what was wrong,
  the clause verbatim, and a link to it in the published revision; JSON and JUnit
  output carry the same. The weekly spec-drift job re-verifies every quote against
  the spec's source and every link's anchor against the published page.
- **The revision is the one the session declares** — in `initialize`, in each
  request's `_meta`, or in the `MCP-Protocol-Version` header. A trace declaring no
  revision is judged against the newest one (and says so); a trace declaring only a
  revision this build does not know is refused rather than judged against the wrong
  rules. `--revision` overrides; naming two judges the session under both, clause by
  clause.
- **Not observed is not a pass.** A clause the session never came near is reported
  *not observed*; one gated on a capability nobody negotiated is *not applicable*;
  one no trace can judge is *excluded*, with the reason. The 86 above are clauses
  this short session never exercised — reporting them as passes would be a score, not
  a verdict.
- **An empty recording is a bad invocation**, not a pass: the shape a broken capture
  step produces should fail the build.

Without `--quiet` every clause is listed with its outcome and, for exclusions, the
reason. `--format json` gives the whole report as data.

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
| [`mcp-trace-capture`](crates/mcp-trace-capture) | **The recorder.** A stdio wrapper and an HTTP reverse proxy (SSE and `https://` included) that forward bytes unchanged and write a validator-ready trace, recording each message before the bytes that complete it are forwarded. CI runs reference-host sessions through it at both revisions and requires them to judge clean, and runs the official suite through the proxy. New; ships with the next release. |
| [`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator) | **The validator and its CLI.** Findings with the clause ID and the offending event `seq`, as human text (full or `--quiet`), JSON, or JUnit, with documented exit codes. Every check is falsified by at least one committed violation trace in [`corpus/`](corpus) — a check that cannot fail is not a check. |
| [`mcp-conformance-core`](https://crates.io/crates/mcp-conformance-core) | **The spec as data.** Requirement registries for `2025-11-25` and `2026-07-28` whose every entry carries a verbatim spec quote, an RFC 2119 level, an optional capability gate, and either a mechanical check or a documented exclusion (the SEP-2484 traceability shape); a weekly job re-verifies every quote against the published text. Plus the JSON Lines trace schema and RFC 8785 canonical JSON. Serde only. |
| [`mcp-everything-server`](https://crates.io/crates/mcp-everything-server) | **The calibration subject**, on [rmcp](https://github.com/modelcontextprotocol/rust-sdk). It passes the pinned official suite's `2025-11-25` server surface — **40/40 checks** — over stdio and policy-gated streamable HTTP, with a default-secure `Host`/`Origin` policy. `--protocol-version 2026-07-28` serves the stateless revision; the suite's pre-release `2026-07-28` scenarios score **41 passing / 0 failing** against it, and five committed captures evidence **114 of the 125 judgeable clauses** between them. Its tap records each suite session for the calibration check. |
| [`mcp-reference-host`](https://crates.io/crates/mcp-reference-host) | **The reference client.** Passes all four of the official suite's `2025-11-25` client scenarios at the pinned version; bounded tool-use loops over stdio and streamable HTTP, scriptable sampling / elicitation / roots with no model-provider network use. |

**Calibration.** On every CI run the official suite (pinned `0.1.16`, `2025-11-25`)
drives the everything server and the reference host, the tapped sessions replay
through the validator, and any MUST-level disagreement not triaged in a committed
ledger fails the build. The `2026-07-28` surface is measured weekly against the
suite's pinned pre-release (`0.2.0-alpha.11`) as a ratchet, not yet as a blocking
agreement check: the suite has no stable `2026-07-28` release to calibrate against.

## Requirement coverage

Generated from the registries by `cargo xtask coverage` and verified in CI:

<!-- coverage:begin (generated by `cargo xtask coverage`; do not edit by hand) -->
**`2026-07-28` — current revision**

| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 57 | 26 | 31 | 0 |
| TRAN | 80 | 33 | 47 | 0 |
| DISC | 4 | 2 | 2 | 0 |
| VERS | 8 | 5 | 3 | 0 |
| MRTR | 25 | 15 | 10 | 0 |
| SUBS | 7 | 4 | 3 | 0 |
| CACH | 18 | 4 | 14 | 0 |
| COMP | 6 | 2 | 4 | 0 |
| PAGE | 6 | 3 | 3 | 0 |
| LOG | 8 | 4 | 4 | 0 |
| TOOL | 29 | 14 | 15 | 0 |
| RES | 13 | 6 | 7 | 0 |
| PROM | 11 | 7 | 4 | 0 |
| **Total** | **272** | **125** | **147** | **0** |

272 requirements: 125 judged by 100 distinct trace checks, 147 carrying a documented exclusion that explains why a recorded trace cannot judge them.

**`2025-11-25`**

| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 25 | 12 | 13 | 0 |
| LIFE | 18 | 10 | 8 | 0 |
| TRAN | 49 | 12 | 37 | 0 |
| TOOL | 15 | 9 | 6 | 13 |
| RES | 10 | 3 | 7 | 6 |
| PROM | 10 | 5 | 5 | 7 |
| LOG | 5 | 1 | 4 | 4 |
| COMP | 5 | 1 | 4 | 3 |
| PAGE | 5 | 2 | 3 | 0 |
| **Total** | **142** | **55** | **87** | **33** |

142 requirements: 55 judged by 51 distinct trace checks, 87 carrying a documented exclusion that explains why a recorded trace cannot judge them.

Every check is falsified by a committed violation trace and examines a real subject on at least one conforming one. A requirement is reported *pass* only where the session carried something it binds to: a capability-gated clause the session never negotiated reports *not-applicable*, and a clause whose subject matter never appeared reports *not-observed*. Neither is a vacuous pass.
<!-- coverage:end -->

## The trace format

A trace is JSON Lines: one event per line, each carrying a capture-assigned `seq`
(the only ordering authority), a `direction`, a `transport`, and a `kind` —
`message` events hold the JSON-RPC payload verbatim; `http` events record
conformance-relevant headers, a response's status, and a client request's
`method`; `lifecycle` events mark transport open/close. This `2025-11-25` session
reuses a request ID:

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
totals: 17 pass, 1 fail, 0 warn, 87 excluded, 0 unsupported, 6 not applicable, 31 not observed
verdict: fail
```
<!-- ANCHOR_END: trace-example -->

The six not-applicable rows are the capability-gated requirements this session
never negotiated (the resources and prompts clauses), and the thirty-one
not-observed rows are the clauses whose subject matter never appeared —
nothing was paginated, no binary content was sent, no error was returned.
Neither is reported as a pass. [`corpus/`](corpus) holds complete annotated
sessions for every area.

## Documentation

- **[The book](https://mcp-conformance.com)** — architecture, the trace format, the
  two revisions, the corpus, and conformance results ([`book/`](book), built on every
  push).
- **API docs** for every crate on [docs.rs](https://docs.rs/mcp-trace-validator).
- **The plan** — charter, ecosystem register, architecture, conformance strategy,
  engineering standards, security model, roadmap, and decision records — in
  [`docs/plan/`](docs/plan/README.md). The ecosystem register dates every external
  fact it records, and a weekly CI job fails when a row is older than ninety days.
- **The [project review of 2026-09-24](docs/reports/project-review-2026-09-24.md)** —
  what was wrong, and the plan being executed to fix it.

## Engineering

Held by CI on every push: clippy pedantic + nursery at `-D warnings` on the pinned
stable toolchain and on the MSRV (1.88) across Linux, macOS and Windows; property,
golden-corpus and falsifiability tests; diff-scoped mutation testing with zero
surviving mutants required in shipped crates; `cargo deny`; and a gate that every
fuzz target builds. Weekly: fuzzing, byte-identical reports on big-endian and 32-bit
targets, a build at the oldest dependency versions the manifests claim, quote drift
against the published spec, and the expiry of every dated claim.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has the gates — `cargo xtask ci` runs them all
locally — and [SECURITY.md](SECURITY.md) has the vulnerability process.

## License

[MIT](LICENSE)
