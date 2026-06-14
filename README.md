<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

**Independent conformance testing for the [Model Context Protocol](https://modelcontextprotocol.io).**
Record a trace of any MCP session — in any language, over any transport — and
find out exactly which of the spec's requirements it met, which it broke, and
why.

**Status: `0.2.0` on [crates.io](https://crates.io/crates/mcp-trace-validator)**
(`cargo install mcp-trace-validator`), published with SLSA build-provenance
attestations. Pre-1.0, so the API and the verdicts it produces may still change
between minor releases — the [changelog](CHANGELOG.md) says so explicitly when
they do.

---

## The problem

The Model Context Protocol is a *specification*: a long list of normative
requirements, each a MUST, SHOULD, or MAY that a conforming implementation is
expected to honor. If you build an MCP server or client, how do you actually
*know* it conforms?

Today there is essentially one answer — the official conformance suite, which
drives **live** scenarios written in TypeScript. That suite is the authority,
and it is invaluable. But it leaves a real gap:

- Nothing takes a **recording** of an MCP session — whatever language produced
  it, whatever transport it crossed — and checks it, requirement by requirement,
  against the spec.
- In Rust there is no reference *everything server* or *host* to measure
  against at all.

## What this is

`mcp-conformance` is the missing half — a toolkit built around three verbs:

- **Capture** a trace of an MCP session: a plain [JSON Lines](#the-trace-format)
  file, one event per line.
- **Validate** it offline and get **requirement-level** findings — the exact
  spec clause, the offending message, and a plain-language reason — as human
  text, machine JSON, or JUnit XML for CI.
- **Calibrate** against the authority: the reference server and host bundled
  here are driven by the *official* suite on every CI run, and this toolkit's
  verdicts are diffed against the official runner's. A disagreement fails the
  build.

That last point is the whole game. A conformance verdict is only worth as much
as its credibility, so these verdicts are **continuously checked against the
recognized authority** — deterministic, reproducible from a committed file, and
defensible rather than a "trust us."

## See it work

Install the validator and point it at a recorded session:

```text
$ mcp-trace-validator validate session.jsonl
MCP trace validation — revision 2025-11-25
  PASS  BASE-001 (MUST)
  ...
  FAIL  LIFE-001 (MUST)
        seq 0: first message is a "tools/list" request, expected "initialize"
totals: 36 pass, 1 fail, 1 warn, 89 excluded, 0 unsupported, 13 not applicable
verdict: fail
```

The exit code is documented (`0` pass, `1` findings, `2` bad invocation,
`3` malformed trace), so this drops straight into CI.

## The one idea: capture, then judge

The validator is a **pure function** — a slice of trace events in, a report out
— with no network, no clock, and no I/O of its own. Whoever owns the socket (the
reference server's session tap, the host's capture wrapper, or any external
proxy) records the trace and assigns the ordering; the validator's only job is
to judge it. The judge also never links the SDK it judges, so its verdicts stay
independent of any one implementation's interpretation of the spec.

That separation is what buys determinism, replayability, and
language/transport independence: the same trace yields a byte-identical report
forever, on any platform — a regression is a diff, not a flake. The design and
its trade-offs are written up for an external audience in
[docs/design/trace-validation.md](docs/design/trace-validation.md).

## The toolkit

| Crate | What it gives you |
|-------|-------------------|
| [`mcp-conformance-core`](https://crates.io/crates/mcp-conformance-core) | **The spec as data.** A requirement registry whose every entry carries a verbatim spec quote, an RFC 2119 level, an optional capability gate, and either a mechanical check or a documented reason it cannot be judged from a trace (the SEP-2484 traceability shape) — covering the `2025-11-25` core protocol surface. Plus the JSON Lines trace schema and RFC 8785 canonical JSON. Serde only; it links no protocol SDK. |
| [`mcp-trace-validator`](https://crates.io/crates/mcp-trace-validator) | **The validator and its CLI.** Replay a trace; get findings with the spec clause and the offending event `seq`, as human text, JSON, or JUnit, with documented exit codes. Every check is falsified by at least one committed violation trace in [`corpus/`](corpus) — a check that cannot fail is not a check. |
| [`mcp-everything-server`](https://crates.io/crates/mcp-everything-server) | **The reference server**, on [rmcp](https://github.com/modelcontextprotocol/rust-sdk) (the official Rust SDK). It passes the official suite's full `2025-11-25` server surface — **40/40 checks** — over stdio and policy-gated streamable HTTP, with a default-secure `Host`/`Origin` policy that closes the CVE-2026-42559 DNS-rebinding class by construction. Its session tap records each suite session as a trace for the calibration check. Offered upstream as [rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902). |
| [`mcp-reference-host`](https://crates.io/crates/mcp-reference-host) | **The reference host** (an MCP client). It passes all four of the official suite's `2025-11-25` **client scenarios** at the pinned version — bounded tool-use loops over both real transports (child-process stdio and streamable HTTP), scriptable sampling / elicitation / roots for CI with zero model-provider network use, and host-side trace capture with redaction by construction. |

## Requirement coverage

The table is generated from the registry by `cargo xtask coverage` and verified
in CI — the numbers cannot drift from the data:

<!-- coverage:begin (generated by `cargo xtask coverage`; do not edit by hand) -->
| Area | Requirements | Checked | Excluded | Capability-gated |
|------|-------------:|--------:|---------:|-----------------:|
| BASE | 24 | 12 | 12 | 0 |
| LIFE | 17 | 9 | 8 | 0 |
| TRAN | 49 | 11 | 38 | 0 |
| TOOL | 15 | 9 | 6 | 13 |
| RES | 10 | 3 | 7 | 6 |
| PROM | 10 | 4 | 6 | 7 |
| LOG | 5 | 1 | 4 | 4 |
| COMP | 5 | 1 | 4 | 3 |
| PAGE | 5 | 1 | 4 | 0 |
| **Total** | **140** | **51** | **89** | **33** |

Revision `2025-11-25`: 140 requirements — 51 judged by 47 distinct trace checks (every check falsified by a committed violation trace), 89 carrying documented exclusions explaining why a recorded trace cannot judge them. Capability-gated requirements report *not-applicable* (never a vacuous pass) for sessions that did not negotiate the capability.
<!-- coverage:end -->

A requirement gated on a capability that was never negotiated is reported
**not-applicable**, never as a pass — inflating a score with vacuous checks is
exactly how a conformance tool loses credibility.

## The trace format

A trace is JSON Lines: one event per line, each carrying a capture-assigned `seq`
(the only ordering authority), a `direction`, a `transport`, and a `kind` —
`message` events hold the JSON-RPC payload verbatim; `http` events record
status and conformance-relevant headers; `lifecycle` events mark transport
open/close. This session reuses a request ID:

<!-- The mdBook chapter book/src/trace-format.md embeds the example below via
     this anchor; readme_examples.rs pins it to the validator's real output. -->
<!-- ANCHOR: trace-example -->
```jsonl
{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"my-host","version":"1.0.0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"my-server","version":"1.0.0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}
```

and the validator answers with the violated clause, verbatim from the spec via the
registry, addressed to the offending event:

```text
  FAIL  BASE-003 (MUST NOT)
        seq 3: request "tools/list" reuses id 1, already used by the same party at seq 0
totals: 45 pass, 1 fail, 0 warn, 89 excluded, 0 unsupported, 5 not applicable
verdict: fail
```
<!-- ANCHOR_END: trace-example -->

The five not-applicable rows are the capability-gated requirements this session
never negotiated (the resources and prompts clauses) — reported as such, never
as passes. [`corpus/`](corpus) holds complete annotated sessions for every area.

## Documentation

- **The book** — architecture, the trace format, the corpus guide, and
  conformance results, collected as an mdBook ([`book/`](book)) that builds on
  every push; a GitHub Pages deploy is wired.
- **API docs** for every crate on [docs.rs](https://docs.rs/mcp-trace-validator).
- **The engineered plan** — charter, verified ecosystem register, architecture,
  conformance strategy, engineering standards, security model, roadmap, and
  decision records — in [`docs/plan/`](docs/plan/README.md). Every claim is
  verified and dated.

## Why it exists

Conformance is the load-bearing mechanism of MCP's maturity model: SEP-1730
gates an SDK's tier standing on its conformance pass rate, and SEP-2484 gates
spec finalization on conformance scenarios. The official suite executes live
scenarios from TypeScript; nothing in any language validates *recorded traces*
against the spec's normative requirements, and no Rust everything server exists.
This project builds that missing half — **upstream-first** (anything generically
useful is offered to the official repositories first), calibrated against the
official suite, and engineered to the standard set by
[a2a-rust](https://github.com/tomtom215/a2a-rust) and held by CI: clippy
pedantic + nursery at `-D warnings` on stable and MSRV across three platforms,
property and golden-corpus tests, diff-scoped mutation gates with a
zero-surviving-mutants standard on every shipped crate, fuzzing, a
sanitization pass, and `cargo deny` on every push.

The full reasoning, with every claim verified and dated, is in
[docs/plan/00-charter.md](docs/plan/00-charter.md).

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has the gates — `cargo xtask ci` runs them
all locally — and [SECURITY.md](SECURITY.md) has the vulnerability process.
Anything generically useful to the official MCP SDKs belongs upstream first; the
engagement backlog is [docs/plan/07-ecosystem-engagement.md](docs/plan/07-ecosystem-engagement.md).

## License

[MIT](LICENSE)
