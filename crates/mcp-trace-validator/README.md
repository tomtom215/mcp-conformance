<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-trace-validator

Deterministic offline validation of recorded MCP protocol traces: replay a JSON Lines
trace against the requirement registry and get requirement-level findings — the clause
verbatim with its link, the offending event `seq`, actionable detail — as human text,
JSON, JUnit XML, SARIF, or one self-contained HTML page.

> **This README is `main`'s.** `cargo install` gets the latest release, `0.6.0`,
> whose own README is the one on crates.io. Documentation for the release is at
> <https://mcp-conformance.com>, and for `main` at <https://mcp-conformance.com/dev/>.
>
> **Not yet released** (on `main` since `0.6.0`):
>
> - `requirements <ID>…` (one requirement in full) and the `--area`, `--level`,
>   `--checked` and `--excluded` filters;
> - the `completions` subcommand;
> - `--version`'s second line, naming the revisions the build judges;
> - the cap of 20 findings per clause in the human report;
> - the `incomplete` verdict, and unjudged traces listed in every format;
> - `judge::judge_with`;
> - the wire schema: every message validated against its revision's JSON Schema
>   (the report's `wire_schema`), and five clauses that needed a schema engine
>   to judge — `BASE-017`/`BASE-073`, `TOOL-045`/`TRAN-137`/`TRAN-082` — plus
>   `TOOL-011`/`TOOL-040` judging conformance to the `outputSchema`, not presence;
> - `--format html` (`html::render`): one self-contained page per run.

`cargo install mcp-trace-validator` (or `cargo binstall mcp-trace-validator` for a
prebuilt binary), then:

```text
mcp-trace-validator validate session.jsonl                 # what needs attention, at the revision it declares
mcp-trace-validator validate --all session.jsonl           # every clause and finding, with exclusion reasons
mcp-trace-validator validate - --format json < session.jsonl
mcp-trace-validator validate session.jsonl --format sarif > results.sarif
mcp-trace-validator validate session.jsonl --format html > report.html     # one file, for a browser or a bug report
mcp-trace-validator validate traces/*.jsonl --format junit > junit.xml   # one document for many traces
mcp-trace-validator validate session.jsonl --revision 2025-11-25 --revision 2026-07-28
mcp-trace-validator requirements --revision 2025-11-25
mcp-trace-validator requirements MRTR-019 BASE-052          # one requirement in full: clause, link, checks
mcp-trace-validator requirements --area TOOL --excluded     # filters: --area, --level, --checked, --excluded
mcp-trace-validator completions bash > ~/.local/share/bash-completion/completions/mcp-trace-validator
```

`--format sarif` is SARIF 2.1.0 for code scanning (GitHub, GitLab, IDE viewers): one
rule per violated clause, one result per finding at the trace line holding its event.
`--format json` is described by the JSON Schema at
[`schema/report.schema.json`](schema/report.schema.json)
(`report::JSON_SCHEMA`).

`--format html` writes one page to attach to a bug report against an SDK: the verdict and
every outcome's count per revision, the wire-schema outcome, each failing or warning clause
with its quote, link and findings, the message timeline — direction, kind, method and id,
which request each response answers, HTTP exchanges — with every finding at its event and
linked both ways, and the passed, not-observed, not-applicable and excluded clauses with
their reasons. It loads nothing: no script, no font, no image; its only style is inline and
admitted by a Content Security Policy that names its SHA-256 digest, and everything taken
from the trace is escaped. Its layout is not a stable surface ([STABILITY.md](../../STABILITY.md)).

The human report lists the first 20 findings under each clause and counts the rest;
`--all` and the machine formats carry every one. Given several traces, `validate`
writes one document per format; a trace it cannot judge (unreadable, malformed, of an
unsupported revision, or judging nothing) is named in its error, listed with that
error in every format — `unjudged_traces` in JSON, an `<error>` test case in JUnit, a
failed-invocation notification in SARIF, a line with its reason in HTML — and makes the
overall verdict `incomplete`.

A trace is JSON Lines — one event per line with a capture-assigned `seq`,
`direction`, `transport`, and a `kind`-discriminated body (`message` carries the
JSON-RPC payload), specified by `mcp-conformance-core`'s trace-event JSON Schema:

```jsonl
{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"my-host","version":"1.0.0"}}}}
```

Findings name the requirement, quote-backed by the registry, and the offending
event: `seq 3: request "tools/list" reuses id 1, already used by the same party
at seq 0`.

Exit codes: `0` pass (warnings allowed unless `--strict`), `1` a clause failed, `2`
bad invocation or nothing judgeable or output not written, `3` malformed trace.
`--version` prints the protocol revisions the build judges.

The built-in registries are the default. To judge against your own requirement
data — a registry you are drafting, or a subset of the clauses — `validate
--registry <file>` takes a single-revision registry document (the shape
`requirements --format json` prints), and `validate --registry-set <file>` a
multi-revision set; the two exclude each other, and `--registry` excludes
`--revision`. `requirements --registry <file>` prints such a document.

Shell completions come from `completions <bash|zsh|fish|powershell|elvish>`, generated
from the same definition that parses the command line. For zsh, write the script as
`_mcp-trace-validator` in a directory on `$fpath`; for fish, to
`~/.config/fish/completions/mcp-trace-validator.fish`.

The library engine (`default-features = false`) has no CLI dependencies and performs no
I/O. `judge::judge_with` judges a document in one call, by the rules the CLI applies,
under the limits and at the revisions you choose:

```rust
use mcp_trace_validator::judge::{self, Options};
use mcp_trace_validator::report::{Outcome, Verdict};

let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"1"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;

let judgment = judge::judge_with(trace, &Options::default())?;
assert_eq!(judgment.verdict(), Verdict::Fail);
let report = &judgment.reports[0];
// `BASE-003`: the request id 1 was used before, by the same party.
let row = report.requirements.iter().find(|row| row.id == "BASE-003").unwrap();
assert_eq!(row.outcome, Outcome::Fail);
assert_eq!(row.findings[0].seq, Some(2));
// Every message is also valid against the revision's JSON Schema.
assert_eq!(report.wire_schema.as_ref().unwrap().outcome, Outcome::Pass);
println!("{}", report.render_findings());
# Ok::<(), Box<dyn core::error::Error>>(())
```

The same as a program that exits non-zero unless the session passed, from a
checkout of the repository:
`cargo run -p mcp-trace-validator --example judge_file -- <trace.jsonl>`
([`examples/judge_file.rs`](examples/judge_file.rs)).

Part of
[mcp-conformance](https://github.com/tomtom215/mcp-conformance); [the
book](https://mcp-conformance.com) covers recording a session, the trace format,
both revisions and every requirement.

License: MIT
