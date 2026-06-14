<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Changelog

All notable changes to this project are documented in this file.

The format is [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Pre-1.0, minor releases may contain breaking changes; entries say so explicitly.

## [Unreleased]

### Added

- **A dependency-floor honesty gate** (`cargo xtask minimal-versions`; scheduled
  `minimal-versions` CI job): the workspace's declared dependency floors
  (`Cargo.toml` `>=x.y.z`) are now the oldest versions it actually resolves to and
  builds/tests against, not assertions. `-Z direct-minimal-versions` pins every
  direct dependency to its floor, builds the whole workspace, and runs the engine
  suites there. Building this surfaced six floors sitting *below* the workspace's
  resolvable minimum — `serde` (→ 1.0.220), `serde_json` (→ 1.0.127), `tower`
  (→ 0.5.2), `tokio-util` (→ 0.7.9), and, in the host crate, `http` (→ 1.1) and
  `futures` (→ 0.3.30, whose old floor 0.3.0 is yanked) — each forced higher by the
  M2 server stack's transitive requirements and raised to the minimum the tree
  resolves to. Nightly-only (the flag is unstable), so a loud skip without it, and
  scheduled rather than per-PR since upstream churn can make a floor newly
  dishonest with no local change. Recorded as a new lens in the testing pyramid
  (`docs/plan/04-engineering-standards.md`).
- **A cross-architecture byte-identity check** (`cargo xtask cross-arch`;
  scheduled `cross-arch` CI matrix): the two engine crates (`mcp-conformance-core`,
  `mcp-trace-validator`) build and run their suites on every corner of the
  **(endianness × pointer-width)** square CI's own hosts leave untested, proving
  M1's "byte-identical reports across platforms" guarantee. Every CI host is
  64-bit little-endian (`x86-64`/`aarch64` Linux/macOS/Windows), so the canonical
  JSON form, the JSON/JUnit reports, and the golden corpus had only ever been
  pinned 64-bit little-endian. The three added corners (`cargo xtask cross-arch`:
  "3 architectures pass"):
  - `s390x` (64-bit **big-endian**) and `powerpc` (32-bit **big-endian**) under
    `qemu-user` — core 58 + validator lib 88 + golden 5 + readme 2 + pathological
    3, byte-identical, with the native frame-budget proof and the subprocess `cli`
    suite out of scope (an emulated stack / a cross-built child cannot exec
    without `binfmt`);
  - `i686` (32-bit **little-endian**) run **natively** via multilib — the *whole*
    suite, `cli` and the deep-stack proof included, byte-identical on 32-bit.

  Each arch runs on its own CI runner (the 32-bit `gcc-multilib` and the
  big-endian cross-gccs hard-conflict at the dpkg level); a target whose toolchain
  is absent skips loudly. Recorded as a new lens in the testing pyramid
  (`docs/plan/04-engineering-standards.md`).

## [0.3.0] - 2026-06-14

> **Version-class call** (RELEASING.md: pre-1.0 minors may break, and the
> changelog says so explicitly): the next release is **0.3.0**, not 0.2.1.
> Two changes below are breaking — `TraceContext::new` (and therefore
> `engine::validate`) now panics on hand-built event slices whose `seq` is
> not strictly increasing, where 0.2.0 judged them silently wrong; and the
> newly judged TRAN-026 changes verdicts, so a trace containing a
> client-POSTed batch body that previously failed only generic message
> checks now also fails `transport.http-post-single-message`.

### Added

- **A project mdBook** (`book/`): a curated reader's guide — Introduction,
  Architecture, the trace format, the trace corpus, and conformance results —
  built and gated on every push by a `book` CI job (`mdbook build book`). The
  trace-format and corpus chapters embed the README's worked example and
  `corpus/README.md` verbatim via `{{#include}}`, so the book cannot drift from
  its sources; docs.rs completeness stays enforced by `missing_docs = "deny"`
  and the `--all-features` rustdoc gate. "Live" GitHub Pages deployment is the
  one owner-gated piece left.
- **Two more standing gates, and the release pipeline grows a third.**
  `cargo xtask version-sync` ties the README's stated crates.io version to
  `[workspace.package].version` (the README update the release checklist used
  to forget — the stale "0.1.0" a prior audit found, now a CI failure); a
  weekly `cargo-careful` job runs the engine crates' suites against a std with
  debug assertions and extra const-UB checks (a UB / integer-overflow
  regression a release build folds is now caught); and the release `verify`
  job runs `cargo xtask semver` (cargo-semver-checks vs the crates.io
  baseline), so an undeclared API break cannot ride a release alongside the
  declared behavioral ones.

- **Claims expire** (ADR-0010): three rounds of auditing found every
  falsehood in claims that were true once and never re-checked — so the
  repository now re-checks them itself. The deferral ledger
  (`docs/plan/deferrals.json`) gives every consciously deferred piece of
  work a review-by date; `cargo xtask deferrals --check` (weekly scheduled
  CI) fails once a row expires un-re-decided. First rows: the suite's
  `auth/*` client scenarios, the rmcp SSE-resumption upstream filing, the
  rust-sdk#902 offer clock, the register's 90-day sweep, and the suite
  0.2.0 pin bump. And the registry's verbatim quotes — verified in round
  two by a `/tmp` script that died with its session — are now re-verified
  weekly by `cargo xtask spec-drift` against the published spec text, under
  the normalization `SourceRef::quote` documents (italics/links/escapes
  unwrapped; list fragments verified verbatim per the `"; "` convention);
  first live run: 140/140 quotes verified. The registry's in-scope page set
  is finally explicit data (`registry/2025-11-25/sources.json`: the nine
  in-scope pages mapped to their published sources, plus every out-of-scope
  page of the revision with a verified reason — the gate keeps the list and
  the registry's citations identical in both directions).
- `cargo xtask ci` now runs the MSRV clippy leg CI runs (loud skip when the
  1.88 toolchain is absent), and `cargo xtask mutants` is the exact
  diff-scoped mutation gate from the PR workflow, computed against
  `origin/main` — the local-vs-CI gate skew that bit round two, mechanized
  away.
- Scheduled CI now accumulates evidence weekly instead of discarding it:
  grown fuzz corpora and criterion bench results upload as 90-day artifacts
  (the round-two "corpora seed-only" / "no bench history" deferrals,
  liquidated — benches/README.md records the posture). The tap's `loom`
  question is re-decided and recorded at the code it judges: nothing
  lock-free to model, uniqueness-only ordinal, real-parallelism stress test
  as the standing evidence.
- **The client gate is standing** (`cargo xtask conformance`, same CI job as
  the server leg): a child-process stdio smoke — the host binary spawning
  the everything-server binary over a real pipe, the one place two sibling
  executables can meet — then the four `2025-11-25` client scenarios run
  sequentially (client runs fail on WARNINGs and the `sse-retry` timing
  window is load-bearing, so parallel suite mode is deliberately not used),
  then the client-side agreement replay: every host-captured trace through
  `mcp-trace-validator` against `conformance/client-agreement-divergences.json`
  (same triage contract and both-directions staleness discipline as the
  server baseline; empty and live on first run — 4 sessions, zero
  unexplained divergence).
- **`mcp-reference-host`: the suite's client scenarios pass — all four, at
  the pin** (`initialize`; `tools_call` 1/1;
  `elicitation-sep1034-client-defaults` 5/5; `sse-retry` 3/3, inside the
  −50/+200 ms retry window with `Last-Event-ID` offered). What landed: the
  two real transports from rmcp's official client features (`proc` =
  child-process stdio, `http` = streamable HTTP over reqwest); the binary
  (`cli`) honoring the runner's contract (URL as final argument,
  `MCP_CONFORMANCE_SCENARIO` dispatch through the one `scenario.rs` table)
  with a hard `--deadline-secs` watchdog (the runner's 30 s kill reaches
  only its `sh -c` wrapper — an orphaned host would wedge the runner
  forever, measured); host-side trace capture (`capture`) as a `Transport`
  wrapper — redaction by construction, the message seam never sees headers
  — whose output is pinned against the validator's real reader and engine;
  and the spec's SSE-resumption dance (`resume`) on rmcp's public
  `StreamableHttpClient` seam, honoring the server-named `retry` through
  `RetryPolicy::delay_honoring_retry_after` (the load-bearing use ADR-0009
  predicted). rmcp 1.7's own transport cannot pass `sse-retry` — POST
  response streams reconnect-never and the in-flight call is lost; measured
  at source and on the wire (−53 ms "too early", no `Last-Event-ID`) —
  recorded as register row 3.12 and ADR-0009 §Amendment, upstream filing in
  the M4 backlog. `reqwest`/`futures`/`sse-stream` enter as direct
  dependencies of the `http` feature, version-mirroring rmcp's own tree.
- `mcp-everything-server`: `test_url_elicitation` — the URL-mode elicitation
  round trip (register 2.10 parity), closing the last interactive
  TypeScript-surface delta: a `mode: "url"` `elicitation/create` and, on
  consent, `notifications/elicitation/complete` for the issued id. The
  host↔server loop — consent recorded, id spent exactly once, by name;
  decline produces no completion — is pinned end to end in the host's
  `agent_loop` tests. The README's "needs a URL-capable client" deferral is
  closed, not restated.
- `mcp-trace-validator`: `transport.http-post-single-message` — TRAN-026
  ("The body of the POST request MUST be a single JSON-RPC request,
  notification, or response.") is now judged, with a killer trace
  (`tran-026-http-post-batch.jsonl`). Its previous exclusion claimed a
  multi-message body "cannot be represented in a trace" — untrue (the payload
  is an arbitrary JSON value, and a batch was only caught generically under
  BASE-008, never attributed to TRAN-026). Registry: 140 entries, 51 judged
  by 47 checks, 89 documented exclusions.
- Registry `TRAN-049`: the transports page states the client POST obligation
  twice (an intro sentence and a numbered step three lines apart); only one
  sentence was an entry. "Every MUST on an in-scope page enters — no
  exceptions" now holds for both, the restatement excluded with prose naming
  its twin.
- `mcp-everything-server`: two tests the registry's exclusions claimed
  existed but did not — `unsupported_protocol_version_is_rejected_with_400`
  (TRAN-020: pins rmcp 1.7's in-session 400; the initialize exchange itself
  never consults the header, measured) and `default_bind_is_loopback`
  (TRAN-008: every other test passes `--bind` explicitly and would never
  notice a widened default).
- **`mcp-reference-host`: the host exists** (M3 opens; ADR-0009). Three
  transport-agnostic pieces, tested in-process against the real
  `mcp-everything-server`: `script` (every model/user behavior as data —
  sampling reply, SEP-1034-defaults/fixed/decline/cancel elicitation
  policies, URL-mode consent, roots; zero model-provider network use by
  construction), `handler` (the `rmcp::ClientHandler` answering from a
  script, with an event log and a pending-id set enforcing the URL-mode
  client MUST — unknown or already-completed `elicitationId` completions
  are observably ignored), and `run` (the bounded loop: scripted calls or
  discover-and-call-once with schema-derived arguments — local `$ref`s
  resolved, enum-as-`oneOf`/`const` sampled — under the stop-condition
  lattice cancellation > turn limit > error budget > completion, every
  variant a tested stop reason, in-band `isError` results counting against
  the budget like protocol errors). The SEP-1034 path round-trips against
  the same `test_elicitation_sep1034_defaults` tool the server-side suite
  run exercises, with the wire content pinned byte-for-byte. The client-SUT
  contract was decoded from the pinned suite 0.1.16 bundle (URL appended as
  the command's final argument, `MCP_CONFORMANCE_SCENARIO`/`_CONTEXT` env,
  30 s budget; four protocol scenarios + fourteen deferred `auth/*` ones)
  and recorded in ADR-0009. Binary, transports, and suite wiring are the
  next slice; the crate README states exactly what is and is not here.
- `mcp-everything-server`: `get-structured-content` — the TypeScript
  everything server's structured-output tool, mirrored exactly (the zod city
  enum, the weather fixtures, derived `outputSchema`, `structuredContent`
  plus the backward-compatible JSON text block). The M2 line claimed "parity
  with the TypeScript everything server's surface" while the server had no
  `outputSchema` tool at all — the suite never exercises one, so nothing
  noticed. The roundtrip test pins the TOOL-010/TOOL-011 pairing the spec
  requires of any server declaring an output schema. The two remaining
  TypeScript-surface deltas (URL-mode elicitation, async sampling) are now
  documented decisions with reasons in the crate README, not silences.
- `mcp-trace-validator`: pathological-input boundedness tests — 100k-event
  sessions validate with correct verdicts, 20k-fold request-id reuse stays
  linear and is flagged, and hostile deep nesting is rejected at parse with
  the offending line named (never a stack overflow, never judged anyway).
  benches/README.md records the re-affirmed no-timing-gate decision: still
  no measurement history, but complexity is now gated by these tests.
- Concurrency and crash-durability proofs for the session tap, replacing
  reasoning with evidence: 16 sessions recording through one writer at real
  parallelism (per-file `seq` contiguous from 0, every file parses through
  the real reader, zero cross-session bleed), and a SIGKILL mid-burst
  integration test pinning the documented durability shape — every persisted
  line parses, at most the final line may be torn.
- `TraceContext::new` (and so `engine::validate`) now *enforces* the
  strictly-increasing-`seq` contract with a documented panic instead of
  judging a contract-violating hand-built slice silently wrong; the
  session-id mutants exclusion's "one event owns one seq" justification now
  names this enforcement rather than assuming the reader is the only path.

### Changed

- Third-audit census closures: the readiness line (`listening on `) is
  single-sourced as `mcp_everything_server::READINESS_LINE_PREFIX` — the
  cross-process contract orchestration waits on — with the binary tests
  pinning the literal independently and xtask's copy carrying the pointer;
  the corpus README states the violation-trace naming contract the golden
  harness enforces (`area-nnn-…` must falsify `AREA-NNN` by name); the
  pathological-input tests document their honest limit (a quadratic-but-
  correct mutant passes unless it blows the mutation timeout — verdicts and
  hangs are the caught classes, by design); and the core README's "every
  in-scope normative clause" claim now names its universe
  (`sources.json` + the spec-drift gate) instead of leaving "in-scope" to
  judgment.
- Two gates can no longer be fooled the way this audit's own tooling was:
  `docs-links` now also checks reference-style definitions (`[label]:
  target` — previously the gate's one false-negative path; today's are all
  external, but a relative one would have passed unchecked), and
  `file-sizes` fails when its scan finds implausibly few files instead of
  reporting a vacuous green over an empty walk.
- `mcp-everything-server`: session-id entropy is pinned, not assumed —
  `session_ids_are_version_4_uuids_and_distinct` asserts the v4-UUID
  version/variant nibbles and distinctness on real initialize responses,
  and TRAN-010's exclusion now cites it (TRAN-011's visible-ASCII check
  would never notice a regression to sequential ids).
- The tap tells the truth about its failure modes, loudly: a non-UTF-8 SSE
  chunk now stops recording that stream (the doc always said "abort"; the
  code cleared the buffer and kept parsing — resuming after a dropped chunk
  can mis-frame everything that follows), and a non-empty request body that
  is not JSON is reported to stderr instead of leaving a silent hole a trace
  reader would misread as "no body". Module docs now state the real
  durability contract: flushed records survive a kill, queued records die
  with the process, the final line may tear.

### Fixed

- **A fuzz harness that contradicted its own unit test** (third audit, found by the
  first real CI run of the weekly fuzz job — dispatched precisely because a
  never-run gate is not a gate). The `canonical_json` fuzz target asserted
  `parse(canonical(v)) == v` over `serde_json::Value` and called it "round-trip
  exact" — but canonicalization deliberately folds representations (RFC 8785 maps
  `-0.0` → `0`, `2.0` → `2`), so that claim is false by design, and the
  `canonical_form_is_a_parse_fixpoint` unit test had always (correctly) asserted
  the *idempotence* property instead. The two disagreed; only the fuzzer, on its
  first generated `-0.0`, could expose it. The canonicalizer was always correct
  (its `-0.0 → 0` fold is RFC 8785 Appendix B, already unit-tested). Fixed: the
  fuzz target now asserts the same idempotence
  (`canonical(parse(canonical(v))) == canonical(v)`); the crashing input is pinned
  as the corpus seed `seed-negative-zero-fold` and as a `cargo test` regression
  (`negative_zero_fold_is_idempotent_not_representation_preserving`); and all three
  fuzz targets were re-run clean (canonical_json 3.5M execs, registry_parse 3.9M,
  trace_parse 12.8M). The census this round was scoped to read `fuzz_targets/*.rs`
  and missed the contradiction — recorded so the next round's census cross-checks
  paired tests of one function, not each in isolation.
- The round's closing verification ran as its floor and its new dimension:
  the full `--all-features` mutation sweep — now **857 mutants** (the round
  added ~109 mutable sites): 741 caught, 116 unviable, **0 missed**, 42
  minutes — and, for the first time, **miri over `mcp-conformance-core`**
  (63 tests, 0 findings; isolation disabled for proptest's cwd persistence,
  and the 50k-deep canonicalization proof runs at depth 500 under
  `cfg(miri)` — the interpreter checks the walker for UB there, not for
  native frame budget, which stays a native-only proof). `cargo audit`:
  233 dependencies, no advisories. `cargo package --workspace --exclude
  xtask --locked`: green. Both conformance legs re-confirmed on the final
  tree: server 40/40 with 30-session agreement, client smoke + 4 scenarios
  with 4-session agreement — zero unexplained divergence everywhere.
- `conformance/expected-failures.yaml` used a `failures:` key the pinned
  runner has never read: the 0.1.16 loader consumes exactly `server:` and
  `client:` keys and silently ignores everything else, so the committed
  baseline was a no-op that happened to coincide with reality (zero expected
  failures). The file now uses the real schema, documents the silent-ignore
  hazard, and carries the (empty) `client:` section the client gate reads.
- The full `--all-features` mutation sweep (748 mutants, 31 minutes) ran as
  the audit's closing verification: 641 caught, 105 unviable, 0 timeouts,
  and exactly 2 missed — both in the tap's non-JSON-body note, code this
  same audit had added hours earlier (its guard had no observer). The note
  is now session-scoped (it can never claim a recording that did not
  happen) and counted against the real binary's stderr; both mutants were
  re-applied by hand and die against the counting test.
- Error-path tests now pin *which* error, not just that one occurred —
  six sites asserted only `is_err()`, and one of them proved able to hide a
  deleted security gate: with the sampling capability gate removed, the old
  assertion stayed green (the doomed `sampling/createMessage` failed
  downstream as `-32603` — after an illegal request had already gone out on
  the wire) while the strengthened test fails (demonstrated by neutering the
  gate). Pinned: the gate's `-32600` and message, resource-not-found's
  `-32002` (the repo's only deliberate use of it), and `-32602` at the four
  parameter-boundary sites whose comments claimed the code the assertions
  never checked.
- The golden corpus now enforces attribution by name: a violation trace
  `area-nnn-…` must produce a Fail/Warn row with findings for exactly
  requirement `AREA-NNN` — previously a defect re-routed to the wrong
  requirement could re-bless silently, guarded only by global check-ID
  set-equality and human diff review. Also: every golden must belong to a
  living trace (orphan sweep), and blessing requires `BLESS=1` exactly,
  matching the coverage manifest's convention (`BLESS=0` no longer blesses).
- The tap's every-platform validator round-trip now fails on *any*
  MUST-level finding, not only `LIFE-*` — a tap serialization regression
  that manufactures transport or base findings (wrong header recording,
  broken `seq`, mangled payloads: precisely what the tap exists to get
  right) was previously visible only in the npx-gated conformance job.
- Five registry exclusions said things the code disproves, found by tracing
  every "enforced instead" pointer to its target: TOOL-012 cited "policy
  tests" for four duties of which two (rate limiting, output sanitization)
  are implemented nowhere; RES-005 cited the wrong test file; TRAN-003
  claimed non-UTF-8 bytes "surface as capture-time read failures" while the
  tap silently skips them; TRAN-008 named the wrong enforcement site;
  LOG-002 called heuristic verdicts "non-deterministic" when the defect is
  unsoundness. Each now states what is actually enforced, where, and by
  which named test.

- The trusted-publishing record was false everywhere it appeared: RELEASING.md,
  ADR-0007's amendment, `release.yml`'s comments and run summary, and the
  v0.2.0 changelog entry all asserted "Trusted Publishing Only" enforced on all
  four crates as of 2026-06-10 — disproven by the v0.2.0 publish itself, whose
  first attempt failed with crates.io's `400: No Trusted Publishing config
  found for repository tomtom215/mcp-conformance`. Every site now states
  exactly what the evidence supports: the config was added 2026-06-11 and is
  proven by the OIDC publish of all four crates; the "Trusted Publishing Only"
  toggle, the bootstrap secret's deletion, and the token's revocation are
  owner-visible only — the owner confirmed on 2026-06-11, after the
  correction landed, that trusted publishing is working as intended
  (ADR-0007 §Correction records the confirmation and its weight).

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
