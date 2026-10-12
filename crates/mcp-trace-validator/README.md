<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-trace-validator

Deterministic offline validation of recorded MCP protocol traces: replay a JSON Lines
trace against the requirement registry and get requirement-level findings — the clause
verbatim with its link, the offending event `seq`, actionable detail — as human text,
JSON, JUnit XML, or SARIF.

`cargo install mcp-trace-validator` (or `cargo binstall mcp-trace-validator` for a
prebuilt binary), then:

```text
mcp-trace-validator validate session.jsonl                 # what needs attention, at the revision it declares
mcp-trace-validator validate --all session.jsonl           # every clause and finding, with exclusion reasons
mcp-trace-validator validate - --format json < session.jsonl
mcp-trace-validator validate session.jsonl --format sarif > results.sarif
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

The human report lists the first 20 findings under each clause and counts the rest;
`--all` and the machine formats carry every one. Given several traces, `validate`
writes one document per format; a trace it cannot judge (unreadable, malformed, of an
unsupported revision, or judging nothing) is named in its error, listed with that
error in every format — `unjudged_traces` in JSON, an `<error>` test case in JUnit, a
failed-invocation notification in SARIF — and makes the overall verdict `incomplete`.

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

Shell completions come from `completions <bash|zsh|fish|powershell|elvish>`, generated
from the same definition that parses the command line. For zsh, write the script as
`_mcp-trace-validator` in a directory on `$fpath`; for fish, to
`~/.config/fish/completions/mcp-trace-validator.fish`.

The library engine (`default-features = false`) has no CLI dependencies and performs no
I/O. `judge::judge_with` judges a document in one call, by the rules the CLI applies,
under the limits and at the revisions you choose. Part of
[mcp-conformance](https://github.com/tomtom215/mcp-conformance); [the
book](https://mcp-conformance.com) covers recording a session, the trace format,
both revisions and every requirement.

License: MIT
