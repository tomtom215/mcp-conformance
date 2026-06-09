<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Contributing

Thank you for considering a contribution. This project holds every line — code, config,
documentation — to the standards in
[docs/plan/04-engineering-standards.md](docs/plan/04-engineering-standards.md). This
file is the practical version of that document.

## Before you build anything substantial

This project is **upstream-first**: anything generically useful to the official MCP
Rust SDK (`modelcontextprotocol/rust-sdk`) or the official conformance suite
(`modelcontextprotocol/conformance`) should be proposed there before it lands here.
Read [docs/plan/07-ecosystem-engagement.md](docs/plan/07-ecosystem-engagement.md), then
open an issue describing your approach before writing a large PR.

## Setup

Stable Rust via [rustup](https://rustup.rs). The workspace pins MSRV 1.85; CI tests
both stable and MSRV, so code must satisfy both.

```bash
git clone https://github.com/tomtom215/mcp-conformance
cd mcp-conformance
cargo xtask ci    # run every local quality gate, in CI order
```

## Quality gates

All of these must pass before merging — `cargo xtask ci` runs them in order:

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings` — also with
   `--no-default-features` and `--all-features`
3. `cargo test --workspace` — also with `--no-default-features` and `--all-features`
4. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`

Additionally enforced in CI: `cargo deny check` (licenses, advisories, sources),
`cargo package --workspace` (publishability), and diff-scoped mutation testing
(`cargo mutants --in-diff`) on PRs. The standard for `mcp-conformance-core` and
`mcp-trace-validator` is **zero surviving mutants**.

## Working on checks and the corpus

The validator's credibility rests on two invariants — both enforced by tests:

1. **Registry ↔ inventory coverage**: every check the registry references exists, and
   every implemented check is referenced.
2. **Falsifiability**: every check is killed by at least one trace in
   `corpus/violations/`. A new check ships with a passing trace, a violating trace,
   and their golden reports.

Workflow: add the requirement to
`crates/mcp-conformance-core/registry/2025-11-25.json` (verbatim spec quote — the
tests check the RFC 2119 keyword is present), implement the check, add corpus traces,
then `cargo xtask bless` and **review the golden diff like code**. Every corpus trace
needs a provenance note in its commit message: what produced it, against which
revision.

## Commits

Conventional Commits with scopes, imperative mood, bodies that explain *why*:

```
feat(validator): add resources/subscribe ordering check
fix(core): reject registry entries with empty check lists
docs(plan): record the 2026-07-28 RC reconciliation outcome
ci(mutants): scope the PR gate to the diff
```

## Pull requests

The PR template checklist is the contract; the short version: SPDX header on every new
file, ≤ 500 lines per file, rustdoc on new public items, tests for new code, ADR for
architectural decisions ([docs/plan/decisions/](docs/plan/decisions/README.md)), plan
documents updated when scope or status changed. No PR merges red — including docs-only
PRs, because docs build with `-D warnings` too.

## Reporting verdict disputes

If the validator's finding disagrees with your reading of the specification, file a
bug with the requirement ID and the spec text. Disputes are triaged into: our bug
(fixed), official-suite divergence (filed upstream), or spec ambiguity (filed
upstream). See [docs/plan/03-conformance-strategy.md](docs/plan/03-conformance-strategy.md).

## Security

Never report vulnerabilities in public issues — see [SECURITY.md](SECURITY.md).
