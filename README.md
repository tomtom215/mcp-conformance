<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# mcp-conformance

A Rust-native conformance toolkit for the [Model Context Protocol](https://modelcontextprotocol.io)
(MCP): a machine-readable requirement registry, a transport-agnostic protocol trace
validator, a Rust "everything server" reference implementation, and a reference host
runtime.

**Status: pre-release (unpublished).** What exists today, with nothing claimed beyond it:

- **`mcp-conformance-core`** — the spec as data: a requirement registry whose entries
  carry verbatim spec quotes, RFC 2119 levels, and SEP-2484-shaped
  check-or-documented-exclusion traceability (seeded with 17 requirements for revision
  `2025-11-25`); the JSON Lines trace schema; JSON-RPC message classification;
  canonical JSON with RFC 8785 key ordering.
- **`mcp-trace-validator`** — a deterministic offline validator: replay a recorded
  trace, get requirement-level findings (spec clause, offending event `seq`,
  actionable detail) as human text, machine JSON, or JUnit XML, with documented exit codes for CI.
  15 checks, each falsified by a committed violation trace in [`corpus/`](corpus).
- **`mcp-everything-server`** / **`mcp-reference-host`** — the M2/M3 artifacts, currently
  shipping only their foundations: a default-secure `Host`/`Origin` policy (the
  CVE-2026-42559 DNS-rebinding class, closed by construction) and a deterministic
  retry/backoff policy.

```text
$ cargo run -p mcp-trace-validator -- validate session.jsonl
MCP trace validation — revision 2025-11-25
  PASS  BASE-001 (MUST)
  ...
  FAIL  LIFE-001 (MUST)
        seq 0: first message is a "tools/list" request, expected "initialize"
totals: 13 pass, 1 fail, 1 warn, 2 excluded, 0 unsupported
verdict: fail
```

The complete project plan — charter, verified ecosystem context, architecture,
conformance strategy, engineering standards, security model, roadmap, and decision
records — lives in [`docs/plan/`](docs/plan/README.md).

## Why this exists

Conformance is the load-bearing mechanism of MCP's maturity model: SEP-1730 gates SDK
tier standing on conformance pass rates, and SEP-2484 gates spec finalization on
conformance scenarios. The official suite executes live scenarios from TypeScript;
nothing in any language validates *recorded traces* of MCP traffic against the spec's
normative requirements, and no Rust everything server exists. This project builds that
missing half — upstream-first, calibrated against the official suite, engineered to
the standard set by [a2a-rust](https://github.com/tomtom215/a2a-rust) and held by CI:
clippy pedantic+nursery at `-D warnings` on stable and MSRV across three platforms,
property tests, golden-corpus tests, diff-scoped mutation gates with a
zero-surviving-mutants standard on the judgment crates, and `cargo deny` on every push.

The reasoning, with every claim verified and dated:
[docs/plan/00-charter.md](docs/plan/00-charter.md).

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has the gates (`cargo xtask ci` runs them all);
[SECURITY.md](SECURITY.md) has the vulnerability process. Anything generically useful
to the official MCP SDKs belongs upstream first —
[docs/plan/07-ecosystem-engagement.md](docs/plan/07-ecosystem-engagement.md).

## License

[MIT](LICENSE)
