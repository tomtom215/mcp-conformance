<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project are documented in this file.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, minor releases may contain breaking changes; entries say so explicitly.

## [Unreleased]

### Fixed

- The trusted-publishing record was false everywhere it appeared: RELEASING.md,
  ADR-0007's amendment, `release.yml`'s comments and run summary, and the
  v0.2.0 changelog entry all asserted "Trusted Publishing Only" enforced on all
  four crates as of 2026-06-10 — disproven by the v0.2.0 publish itself, whose
  first attempt failed with crates.io's `400: No Trusted Publishing config
  found for repository tomtom215/mcp-conformance`. Every site now states
  exactly what the evidence supports: the config was added 2026-06-11 and is
  proven by the OIDC publish of all four crates; the "Trusted Publishing Only"
  toggle, the bootstrap secret's deletion, and the token's revocation are
  owner-visible only and stand unconfirmed (ADR-0007 §Correction).

## [0.2.0] - 2026-06-11

### Added

- **Registry completeness audit (2026-06-11)**: clause-by-clause re-extraction
  of the `2025-11-25` spec found 68 in-scope normative clauses missing from
  the registry; all are now entries (71 → 139), every quote verified verbatim
  against the published text. Four are mechanically checkable and gained
  checks plus killer traces: `lifecycle.initialize-result-shape` (LIFE-010 —
  the initialize result must carry `capabilities` and `serverInfo`),
  `transport.client-accept-header` (TRAN-025/TRAN-039 — every client request
  must list `text/event-stream` in `Accept`), `transport.success-content-type`
  (TRAN-029/TRAN-040 — HTTP 200s must answer `application/json` or
  `text/event-stream`), and `base.meta-key-format` (BASE-019/BASE-020 — the
  `_meta` key prefix/name grammar, scoped to the `params`/`result` envelope
  positions where user data cannot collide). The other 61 carry documented
  exclusions naming exactly why a recorded trace cannot judge them (stream
  identity, request methods, timing, and server-internal ground truth are
  not in the capture vocabulary). The agreement check over the suite's 30
  tapped sessions runs the new checks at zero unexplained divergence.

- **The agreement check is live** (docs/plan/03-conformance-strategy.md
  §Calibration): `mcp-everything-server` gains a session trace tap (feature
  `tap`, `--tap-dir`, HTTP transport) recording every admitted suite session
  as a validator-ready JSON Lines trace — allowlisted headers only, so
  credential-bearing headers are never captured; the writer assigns `seq`
  per file so the schema's strictly-increasing rule holds even when POST
  exchanges and SSE streams record concurrently. `cargo xtask conformance`
  now replays every tapped session through `mcp-trace-validator` and fails
  on any MUST-level finding not explained in
  `conformance/agreement-divergences.json` (triage class `our-bug` |
  `suite-bug` | `spec-ambiguity` plus an upstream link required; unknown
  fields rejected), writing the full reconciliation to
  `target/conformance/agreement.json`. First run: 30 sessions, zero
  unexplained divergence — and one real catch each way: a MUST divergence
  triaged suite-bug (#7: the runner's dns-rebinding client skips
  `notifications/initialized`) and an informational SHOULD warning on the
  suite's deliberate version-compat probe (TRAN-018).
- **Coverage manifest** (`conformance/coverage-manifest.json`): generated
  from the tapped sessions and checked on every conformance run (`BLESS=1`
  regenerates) — the server's declared capabilities, all eight server-party
  registry capability gates (each must be declared: the gate caught the
  missing `listChanged` declarations on first run), and the 18 wire methods
  the suite exercises.
- `mcp-everything-server`: `test-list-changed` tool emits the three
  `notifications/*/list_changed` messages, and the server now declares
  `listChanged` for tools, resources, and prompts — declared because
  exercisable, per the capability-honesty rule.
- `mcp-conformance-core`: `TraceEvent::new` — the constructor capture
  tooling needs (`TraceEvent` is `#[non_exhaustive]`, so out-of-crate
  literals don't compile).
- `mcp-everything-server`: `tap::RECORDED_HEADERS` is now public — the
  recording allowlist is worth inspecting, and the doc gate (now run with
  `--all-features`) caught a private-intra-doc link that made it so.
- `mcp-everything-server`: streamable HTTP serving (`--transport http`)
  behind the default-secure `Host`/`Origin` policy — 403 before any MCP
  processing, loopback-only by default, `--allowed-host` /
  `--dangerously-allow-any-host` to widen. The full official-suite server
  surface is implemented (suite-defined `test_*` tools incl. sampling and
  the three elicitation scenarios, resources + template + subscriptions,
  four prompts, completion, logging level filtering): **100% pass on
  @modelcontextprotocol/conformance 0.1.16's active `2025-11-25` server
  scenarios** (40 checks), verified against the real runner.
- `mcp-everything-server`: the M2 build-out begins on rmcp 1.7 — the
  `EverythingServer` handler (protocol `2025-11-25`, capabilities advertised
  only once implemented), the tool module (`echo`, `add`, TypeScript
  everything-server phrasing), and a stdio binary
  (`mcp-everything-server --transport stdio`). In-process duplex round-trip
  tests drive a real rmcp client against the server with no sockets.

### Changed

- **MSRV raised from 1.85 to 1.88** — rmcp's measured compilation floor
  (let-chains in its library source; undeclared upstream). Per policy
  (ADR-0004/ADR-0008) this makes the next release **0.2.0**.
- Release pipeline is OIDC-only: the one-time bootstrap conditional is removed
  from `release.yml`; the publish job authenticates exclusively via trusted
  publishing (ADR-0007). *(Corrected 2026-06-11: this entry originally asserted
  "Trusted Publishing Only" enforcement and token revocation as fact — neither
  was verifiable from this repository, and the enforcement claim was false when
  written; see ADR-0007 §Correction. The v0.2.0 GitHub Release body carries the
  original wording.)*

### Fixed

- `mcp-conformance-core`: `to_canonical_string` walks nesting with an explicit
  heap work-stack instead of recursion — a deeply nested hostile value can no
  longer overflow the call stack (an uncatchable abort). Output is
  byte-identical.
- `mcp-conformance-core`: `EventBody::Http` normalizes header field names to
  lowercase on deserialization (HTTP names are case-insensitive, RFC 9110
  §5.1). Previously a trace recording on-the-wire casing (`Mcp-Session-Id`,
  `Mcp-Protocol-Version`) slipped past the case-sensitive transport checks,
  hiding a bad session id or protocol version behind its capitalization.
- `mcp-trace-validator`: BASE-004/BASE-009 now flag a request answered by
  *both* a result and an error (each check formerly tracked only its own
  response flavor and saw a clean one-to-one).
- `mcp-trace-validator`: JUnit XML escaping substitutes the C0 control
  characters XML 1.0 forbids entirely (other than tab/LF/CR), so a report can
  never be an ill-formed document a strict CI parser rejects.
- `mcp-everything-server`: the session tap's SSE splitter now stops
  recording a stream whose un-delimited frame outgrows the recording budget
  (the same 4 MiB bound the JSON path already had) instead of buffering it
  without limit — recording is diagnostics and must never be what takes the
  server down. The stream itself still flows to the client untouched.
- `mcp-everything-server`: the tap records repeated HTTP header field lines as
  their comma-joined value (RFC 9110 §5.3), so a split `Accept` header is
  captured faithfully rather than truncated to its first line.
- Release packaging excludes `xtask` (`publish = false`, but
  `cargo package --workspace` still packaged it; v0.1.0's GitHub Release
  carries the stray — harmless — crate file).

### Security

- `mcp-everything-server`: the `Host`/`Origin` 403 gate now fails closed on
  duplicate `Host` or `Origin` headers (a smuggling shape — it previously
  judged only the first value while a downstream consumer could key off a
  later one). A well-formed request carries exactly one of each.
- `mcp-everything-server`: the per-session `resources/subscribe` set is
  capped, so a hostile client cannot grow its bookkeeping without bound.

## [0.1.0] - 2026-06-10

First release: the `2025-11-25` requirement registry and the offline trace
validator, at the gates documented in [docs/plan/04-engineering-standards.md](docs/plan/04-engineering-standards.md).

### Added

- `mcp-conformance-core`: requirement registry model (RFC 2119 levels, verbatim
  spec quotes, SEP-2484-shaped check-or-exclusion traceability, ADR-0006
  capability gates) covering the `2025-11-25` core protocol surface — base
  protocol, lifecycle, transport security, tools, resources, prompts, logging,
  completion, and pagination, stored as per-area registry files; JSON Lines trace
  event schema; JSON-RPC message classification; canonical JSON serialization with
  RFC 8785 object-key ordering and ECMAScript number formatting validated against
  the RFC's Appendix B vectors.
- `mcp-trace-validator`: deterministic validation engine spanning every registry
  area, with request/response exchange pairing and not-applicable accounting for
  capability-gated requirements; human/JSON reports with
  pass/fail/warn/excluded/unsupported/not-applicable accounting; CLI with
  documented exit codes (0 pass, 1 findings, 2 invocation problem, 3 malformed
  trace); golden-corpus test harness with falsifiability enforcement (every check
  killed by a committed violation trace) and a provenance-ledger invariant;
  criterion benchmarks (unmonitored by CI — see `benches/README.md`).
- `mcp-everything-server`: default-secure HTTP transport policy — loopback-only
  `Host`/`Origin` allowlisting, fail-closed parsing, explicit
  `dangerously_allow_any_host` opt-out.
- `cargo xtask coverage`: generates the README's per-area requirement-coverage
  table from the registry; CI verifies it never drifts.
- CI: informational `-Zminimal-versions` job proving the workspace dependency
  floors build and pass tests.
- Release pipeline (`release.yml`, ADR-0007): tag-triggered, rehearsable via
  `workflow_dispatch`; full gates + cross-OS tests, SLSA build-provenance
  attestation with a byte-identity check between attested and published
  packages, idempotent GitHub Releases, resumable dependency-order publishing,
  and OIDC trusted publishing after a one-time bootstrapped token release.
- `mcp-reference-host`: deterministic retry/backoff policy with caller-supplied
  jitter and capped `Retry-After` honoring.
- Workspace tooling: `cargo xtask ci` (all local gates) and `cargo xtask bless`
  (golden regeneration); CI with format/clippy/test matrices (stable + MSRV 1.85 ×
  Linux/macOS/Windows × three feature modes), docs, `cargo-deny`, package
  validation, diff-scoped mutation gate on PRs, and scheduled RustSec audit + full
  mutation sweep.

[Unreleased]: https://github.com/tomtom215/mcp-conformance/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.2.0
[0.1.0]: https://github.com/tomtom215/mcp-conformance/releases/tag/v0.1.0
