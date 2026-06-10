<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Engineering Standards

**Status:** Active
**Last reviewed:** 2026-06-09

---

The bar is [a2a-rust](https://github.com/tomtom215/a2a-rust): every standard below is either
lifted from it verbatim or strengthened. Nothing here is aspirational — each item is a CI
gate or a PR-review check from the first commit of code. "We'll add it later" is how
later never arrives.

## Source standards

| Standard | Rule |
|----------|------|
| License headers | SPDX header (`MIT`) + copyright line on **every** file: Rust, TOML, YAML, Markdown, shell. |
| Unsafe code | `#![forbid(unsafe_code)]` at every library crate root — a compile-time guarantee, not a lint. |
| File size | ≤ 500 lines per file. Thin `mod.rs` files (re-exports and docs only). |
| Panics | No `unwrap()`/`expect()`/`panic!()` reachable from untrusted input. Malformed traces, hostile JSON, and broken transports produce typed errors and documented exit codes. |
| Public API docs | rustdoc on every public item, with runnable examples on entry points. `RUSTDOCFLAGS="-D warnings"`. |
| Comments | Explain *why* and constraints — never narrate the next line. Config values carry their justification (the a2a-rust `mutants.toml` discipline). |

## Lints

- `clippy::pedantic` + `clippy::nursery` (plus `unwrap_used` / `expect_used`), configured
  once in `[workspace.lints]` so every crate inherits the policy from one table; analyzer
  thresholds and the MSRV hint live in `clippy.toml` ([ADR-0004](decisions/0004-toolchain-and-msrv.md)).
- `RUSTFLAGS="-D warnings"` and `cargo fmt --check` in CI; no warning ever merges. Test
  modules may `#[allow(clippy::unwrap_used)]` — locally and visibly, never crate-wide.
- Thresholds carried from a2a-rust: cognitive-complexity 25, function length 60,
  max 7 arguments — deviations require a comment at the override site.

## Toolchain policy

- **MSRV pinned** in `[workspace.package] rust-version`: 1.85 at M0, raised to **1.88** at
  M2 when rmcp's measured floor forced it ([ADR-0008](decisions/0008-msrv-1.88.md),
  [register 3.5](01-ecosystem-context.md)),
  tested in CI on every platform, bumped only in minor releases with a changelog entry.
- **Edition:** latest stable edition compatible with the chosen MSRV, fixed at M0.
- Stable toolchain for all gates; nightly runs as a non-blocking informational job.

## Testing pyramid

Zero network to model providers or external services in any test; zero API credits ever.
Package registries are reachable only at dependency-install time, under lockfiles.

| Layer | Tool | Minimum |
|-------|------|---------|
| Unit | `#[test]` / `#[tokio::test]` | Every public function; every state-machine transition, including every error edge |
| Property | `proptest` | Round-trips for all serialized types (trace events, registry records, reports); state-machine invariants under generated event sequences |
| Golden corpus | trace fixtures + golden reports | 100% on known-good traces; every check killed by at least one injected-violation trace ([03-conformance-strategy.md](03-conformance-strategy.md)) |
| Integration | `tests/`, real processes over stdio/HTTP on loopback | Host ↔ everything-server round trips per transport |
| Mutation | `cargo-mutants` | Zero surviving mutants in every shipped crate (xtask excluded); diff-scoped gate (`--in-diff`) on PRs, full workspace sweep scheduled |
| Fuzz | `cargo-fuzz` | Targets: trace parsing, JSON canonicalization, registry deserialization. Clean for the CI fuzz budget; corpora checked in |
| Conformance | official runner via `xtask` | Agreement check green; M2 onward: 100% server-scenario pass as a hard gate |
| Benchmarks | `criterion` | Validator throughput (events/sec), canonicalization, state-machine stepping — measured, not gated: no baseline history exists yet, and an invented threshold would be folklore (decision recorded in `crates/mcp-trace-validator/benches/README.md`; revisit with M2's production-shaped workload) |

## CI gates (every PR, in order of cost)

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings` — repeated per feature
   combination, including `--no-default-features`, `--all-features`, and
   `draft-2026-07-28`
3. `cargo test` matrix: {stable, MSRV} × {Linux, macOS, Windows} × feature combinations
4. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
5. `cargo deny check` (license allowlist, advisories, sources) + `cargo audit`
6. `cargo package --workspace` validation (publishability of every release crate)
7. Mutation gate (diff-scoped), fuzz smoke, golden-corpus run, agreement check
8. Nightly + official-suite `0.2.0-alpha` tracking as scheduled, non-blocking jobs

Workflow hygiene: GitHub Actions pinned by commit SHA; concurrency cancellation for
superseded pushes (never for `main`); least-privilege workflow permissions.

## Dependency policy

- Every dependency is a liability; the burden of proof is on adding it.
  `mcp-conformance-core` carries serde-family only ([02-architecture.md](02-architecture.md)).
- Workspace-level version ranges with upper bounds (`>=x.y, <next-major`), the a2a-rust
  convention; `deny.toml` enforces the license allowlist and bans duplicate-major drift.
- `Cargo.lock` committed (workspace contains binaries and a CLI; reproducible CI outweighs
  library-lockfile purism).

## Releases

- SemVer 2.0.0. All publishable crates share one version and release together
  (a2a-rust model). `#[non_exhaustive]` on protocol-facing types.
- Tag-triggered release workflow: version/CHANGELOG validation → full CI → packaging with
  SLSA build-provenance attestation → publish dry-run → GitHub Release with notes →
  crates.io publish in dependency order with index-propagation waits.
- **Trusted publishing (OIDC)** to crates.io — no long-lived registry tokens. This is the
  one deliberate upgrade over a2a-rust's token-in-environment approach.
- `CHANGELOG.md` per Keep-a-Changelog for *code* releases. Plan documents carry no
  changelogs ([ADR-0001](decisions/0001-plan-documentation-model.md)).
- Deprecations in our public API follow the spirit of MCP's own lifecycle policy
  ([register 1.4](01-ecosystem-context.md)): a deprecated item documents its replacement and
  survives at least one minor release before removal; post-1.0, at least twelve months.

## Commits and PRs

- Conventional Commits with scopes (`feat(validator): …`, `ci(mutants): …`,
  `docs(plan): …`); imperative mood; bodies explain *why*.
- Every PR: standards checklist (headers, file caps, docs, tests, mutation gate), and an ADR
  added or updated when an architectural decision was made or revised.
- No PR merges red. No exceptions, including "it's just docs" — docs build with
  `-D warnings` too.

## Repository governance files

Created at M0, modeled on a2a-rust: `CONTRIBUTING.md` (gates, checklist, commit format),
`SECURITY.md` ([05-security-model.md](05-security-model.md)), `GOVERNANCE.md` (roles),
`RELEASING.md` (the process above), `CITATION.cff`, issue/PR templates.
