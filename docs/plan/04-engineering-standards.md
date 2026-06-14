<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Engineering Standards

**Status:** Active
**Last reviewed:** 2026-06-11

---

The bar is [a2a-rust](https://github.com/tomtom215/a2a-rust): every standard below is either
lifted from it verbatim or strengthened. Nothing here is aspirational — each item is a CI
gate or a PR-review check from the first commit of code. "We'll add it later" is how
later never arrives.

## Source standards

| Standard | Rule |
|----------|------|
| License headers | SPDX header (`MIT`) + copyright line on **every** file whose format carries comments: Rust, TOML, YAML, Markdown, shell, proptest regressions. Comment-incapable formats (JSON, lockfiles, `.cff`, binary fuzz seeds) are exempt — they cannot carry one. Third-party texts keep their own license header (`CODE_OF_CONDUCT.md` is CC-BY-4.0 Contributor Covenant). |
| Unsafe code | `#![forbid(unsafe_code)]` at every library crate root — a compile-time guarantee, not a lint. |
| File size | ≤ 500 lines per non-test source file (`src/` trees) and per embedded registry document, enforced by `cargo xtask file-sizes` in CI. Integration tests and benches are exempt (they live outside `src/`) but should stay navigable. Thin `mod.rs` files (re-exports and docs only). |
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
| Sanitization | `cargo-careful` | The engine crates (`mcp-conformance-core`, `mcp-trace-validator`) run their suites against a std built with debug assertions and extra const-UB checks; a UB or integer-overflow regression a release build folds is a failure. Scheduled (nightly toolchain) |
| Cross-architecture | `s390x` + `powerpc` (qemu) + `i686` (native) | The engine crates' suites run on every corner of the (endianness × pointer-width) square CI's hosts leave untested — `s390x` (64-bit big-endian) and `powerpc` (32-bit big-endian) under qemu, `i686` (32-bit little-endian) native — via `cargo xtask cross-arch`: the canonical form, the JSON/`JUnit` reports, and the golden corpus must be byte-identical where every CI host is 64-bit little-endian (`x86-64`/`aarch64`). A byte-for-byte divergence is a failure. Scheduled (`cross-arch` matrix) |
| Conformance | official runner via `xtask` | Agreement check green; M2 onward: 100% server-scenario pass as a hard gate |
| Benchmarks | `criterion` | Validator throughput (events/sec), canonicalization, state-machine stepping — measured, not gated: no baseline history exists yet, and an invented threshold would be folklore (decision recorded in `crates/mcp-trace-validator/benches/README.md`; revisit with M2's production-shaped workload) |

## CI gates (every PR, in order of cost)

1. `cargo fmt --all -- --check`
2. `cargo clippy --workspace --all-targets -- -D warnings` — repeated per feature
   combination: default, `--no-default-features`, and `--all-features` (the
   `draft-2026-07-28` feature joins the matrix when M2.5 introduces it)
3. `cargo test` matrix: {stable, MSRV} × {Linux, macOS, Windows} × feature combinations
4. `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` — twice: default
   features and `--all-features` (feature-gated modules carry rustdoc the default
   build never sees)
5. Structural gates: README coverage table in sync with the registry
   (`cargo xtask coverage --check`), the ≤ 500-line file cap
   (`cargo xtask file-sizes`), and every relative documentation link resolving
   (`cargo xtask docs-links`)
6. `cargo deny check` (license allowlist, advisories, sources); `cargo audit` runs
   in the weekly scheduled job
7. `cargo package --workspace --exclude xtask --locked` validation (publishability
   of every release crate; xtask is `publish = false` yet `--workspace` would
   package it)
8. Mutation gate (diff-scoped), golden-corpus run, agreement check; fuzzing runs
   in the weekly scheduled job (five minutes per target)
9. Nightly toolchain (per-push, informational) + official-suite `0.2.0-alpha`
   tracking as a scheduled, non-blocking job

Workflow hygiene: GitHub Actions pinned by commit SHA; concurrency cancellation for
superseded pushes (never for `main`); least-privilege workflow permissions.

## Dependency policy

- Every dependency is a liability; the burden of proof is on adding it.
  `mcp-conformance-core` carries serde-family only ([02-architecture.md](02-architecture.md)).
- Workspace-level version ranges with upper bounds (`>=x.y, <next-major`), the a2a-rust
  convention; `deny.toml` enforces the license allowlist, denies wildcards, and warns on
  duplicate-major drift (warn, not deny: transitive churn happens — the config carries
  the justification).
- `Cargo.lock` committed (workspace contains binaries and a CLI; reproducible CI outweighs
  library-lockfile purism).

Per-dependency justification (direct runtime dependencies only; dev/bench
dependencies are justified where they are declared):

| Dependency | Crates | Why it clears the bar |
|------------|--------|----------------------|
| `serde`, `serde_json` | all | The protocol is JSON; `float_roundtrip` is correctness (canonical fixpoint) |
| `clap` | binaries, behind `cli` features | [ADR-0005](decisions/0005-cli-argument-parsing.md): library consumers never pay for it |
| `rmcp` | everything-server | The point of M2: parity proven *on the official SDK*, not beside it. Feature-minimal (`server`, `macros`, `transport-io`, `transport-streamable-http-server`); MSRV consequence in [ADR-0008](decisions/0008-msrv-1.88.md) |
| `tokio` | everything-server (`cli`), reference-host (M3) | rmcp's runtime, not re-litigated; no default features, per-crate feature grants |
| `schemars` | everything-server | Tool/prompt parameter schemas for rmcp's `#[tool]`; already in rmcp's `server` feature tree |
| `tokio-util` | reference-host; everything-server (floor shim) | Direct in the host: `CancellationToken` is the bounded loop's cooperative stop condition (ADR-0009), already in the tree as rmcp's own dependency. In the everything-server only as a documented minimal-versions floor repair |
| `tokio-stream` | everything-server | Direct under the `tap` feature (`StreamExt::then` drives the SSE recording pass-through) *and* a documented floor repair for rmcp's under-specified requirement — the manifest comment carries both facts |
| `http-body-util`, `tracing` | everything-server (floor shims) | Not used directly: documented minimal-versions floor repairs for under-specified third-party requirements (each names its culprit in the manifest; removable when upstream fixes) |
| `reqwest`, `futures`, `sse-stream` | reference-host (`http` feature) | The SSE-resumption dance drives rmcp's **public** `StreamableHttpClient` seam (ADR-0009 §Amendment): `reqwest::Client` is the trait's only shipped implementation, and the seam's vocabulary is `futures` streams of `sse_stream::Sse` frames. All three version-mirror rmcp's own requirements and were already in the tree as its dependencies |

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
