<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Roadmap

**Status:** Active
**Last reviewed:** 2026-10-12

---

Milestones are defined by **definitions of done, not dates**. A milestone closes when every
DoD line is demonstrably true (CI link or artifact), and never before. Sequencing is strict
where stated; standing workstreams run across milestones. Status lives here and only here —
other documents describe intent, this one tracks reality.

External anchors (context, not commitments): the `2026-07-28` spec release
([register 1.2](01-ecosystem-context.md), change inventory in
[1.5a](01-ecosystem-context.md)) and the official suite's `0.2.0` line
([register 2.4](01-ecosystem-context.md)).

## Milestone status

| Milestone | Status |
|-----------|--------|
| M0 — Foundation | **Complete** — every gate green in [CI run #3](https://github.com/tomtom215/mcp-conformance/actions/runs/27233613023) |
| M1 — Registry and validator | **Complete** — v0.1.0 published via [release run #2](https://github.com/tomtom215/mcp-conformance/actions/runs/27245596142) (attested, byte-verified) |
| M2 — Everything server | **Complete** (2026-06-11) — 40/40 checks against the pinned suite in CI ([run #27266174013](https://github.com/tomtom215/mcp-conformance/actions/runs/27266174013)); agreement check live. The upstream adoption offer ([rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902)) closed unadopted on 2026-10-12 ([07-ecosystem-engagement.md](07-ecosystem-engagement.md), item 1) |
| M2.5 — `2026-07-28` migration readiness | **Complete** (shipped in 0.6.0) — every build judges `2026-07-28` ([ADR-0018](decisions/0018-judge-the-declared-revision.md)); its corpus pairs run in the golden suite on every `cargo test`, each with a provenance row the corpus invariant test requires |
| M3 — Reference host | **Complete** (2026-06-13) — all four `2025-11-25` client scenarios pass at the pinned suite as a standing CI gate ([run #27449549660](https://github.com/tomtom215/mcp-conformance/actions/runs/27449549660)) |
| M4 — Upstream engagement | **In progress** (gate, not phase) — the public design note is done; the two merged-outcome lines are open and owner/upstream-gated (below) |
| M5 — Stewardship artifacts | **Complete** — [rmcp](../reports/rmcp-tier-gap-2025-11-25.md) and [pmcp](../reports/pmcp-tier-gap-2025-11-25.md) tier-gap reports published; the book is live at <https://mcp-conformance.com>, and `pages.yml` builds it as a versioned site, one book per release (`cargo xtask book-site`); the `draft-2026-07-28` feature gate is dropped |

## M0 — Foundation

Repository scaffolding at the full standards bar before any feature code.

**Definition of done**

- [x] Cargo workspace with the four crates + `xtask` skeletons compiling on
      {stable, MSRV} × {Linux, macOS, Windows}; MSRV and edition selected and recorded in
      the workspace manifest with rationale.
- [x] CI live with every gate from [04-engineering-standards.md](04-engineering-standards.md)
      §CI (format, clippy matrix, test matrix, docs, deny+audit, package validation), actions
      SHA-pinned, all green.
- [x] SPDX headers on every file; `clippy.toml`, `deny.toml`, `mutants.toml` in place with
      justified values.
- [x] Governance files: `CONTRIBUTING.md`, `SECURITY.md`, `GOVERNANCE.md`, `RELEASING.md`,
      `CITATION.cff`, issue/PR templates.
- [x] Root `README.md` states scope honestly (pre-release, no unearned badges or claims).
- [x] Crate names registered on crates.io as minimal-but-real `0.1.0` releases **or** a
      recorded decision to defer ([ADR-0003](decisions/0003-crate-naming.md) notes the
      squatting trade-off; `mcp-host` went from free to 33 releases in five months).

## M1 — Registry and validator (first public release)

The spec as data, and the engine that judges traces against it.

**Definition of done**

- [x] Requirement registry for `2025-11-25` complete for the core protocol surface
      (lifecycle, tools, resources, prompts, logging, completion, pagination, transport
      security), each entry carrying level, actor, source quote, applicability, and
      check-or-exclusion ([02-architecture.md](02-architecture.md)); coverage table
      generated into the README by `cargo xtask coverage` and verified in CI.
      *(Erratum, found and closed by the 2026-06-11 repo audit: this line was not true
      as originally closed — a clause-by-clause re-extraction against the live spec
      text found 68 in-scope normative clauses with no entry, among them lifecycle's
      "The server MUST respond with its own capabilities and information" and twelve
      streamable-HTTP MUSTs. All 68 are now in the registry (71 → 139 entries: 7 entries
      judged by 4 new checks with killer traces, 61 documented exclusions), every quote re-verified
      verbatim against the published text in the same audit, and the agreement check
      over the suite's 30 tapped sessions stayed at zero unexplained divergence with
      the new checks active.)*
      *(Second adversarial audit, 2026-06-11, post-v0.2.0: one further clause — transports' numbered restatement
      of the POST obligation, now TRAN-049 — and TRAN-026 converted from a falsely
      excluded entry ("a multi-message body cannot be represented in a trace" — untrue:
      the payload is an arbitrary JSON value) to a judged one with a killer trace.
      140 entries, 51 judged by 47 checks, 89 exclusions, each exclusion's
      enforcement-pointer now verified against a named test.)*
      *(Third adversarial audit, 2026-07-26 — this one over the **exclusions** rather
      than the spec text: all 89 documented exclusions re-read against what a trace
      actually carries. **88 held**, several emphatically so (TRAN-001: a parseable
      trace *is* the evidence framing held; TRAN-003: absence is not evidence because
      capture drops undecodable bytes; BASE-011: the violation is caught, attributed to
      BASE-004/BASE-009). **One did not.** PROM-008 ("Servers SHOULD validate prompt
      arguments before processing") was excluded as "validation thoroughness is
      implementation-internal" — true of thoroughness, false of *failure*: a server that
      answers `prompts/get` with a result, when the request omits an argument that same
      server published as `required` in `prompts/list`, demonstrably processed input it
      had declared invalid. Both halves of that comparison are recorded messages, so the
      exclusion's premise was simply untrue — the same shape as TRAN-026's in the second
      audit. Now judged by `prompts.arguments-validated` with a killer trace, which took
      the registry to **140 entries, 52 judged by 48 checks, 88 exclusions**. The check is deliberately narrow
      — it fires only on prompts whose required arguments were observed being declared,
      and only when the server returned a *result*, since an error means the server did
      reject the call. PROM-007 stays excluded on purpose: two of its three enumerated
      failure cases (invalid name, internal error) really are server-side ground truth.)*
- [x] Validator replays the corpus deterministically: 100% pass on known-good traces;
      **every check is killed by at least one injected-violation trace**; byte-identical
      reports across platforms and runs.
      *(2026-06-14: "across platforms" is now verified on every corner of the
      (endianness × pointer-width) square CI's own hosts leave untested, not only the
      64-bit little-endian ones (`x86-64`/`aarch64`). The engine crates' suites —
      canonicalization, the JSON/JUnit renderers, and the golden replay whose reports are
      pinned byte-for-byte — pass on `s390x` (64-bit big-endian) and `powerpc` (32-bit
      big-endian) under qemu, and `i686` (32-bit little-endian) native, via `cargo xtask
      cross-arch` and the scheduled `cross-arch` matrix. This turns an asserted
      cross-platform guarantee into a tested one.)*
- [x] Session state machine for `2025-11-25` with every transition and error edge unit- and
      property-tested.
- [x] Report formats: human, JSON, JUnit; exit codes 0/1/2/3 documented and tested.
- [x] Zero surviving mutants in `mcp-conformance-core` and `mcp-trace-validator`; fuzz
      targets (trace parse, canonicalization, registry deserialization) clean for the CI
      budget with corpora committed.
      *(Erratum, third audit 2026-06-13: "clean for the CI budget" was unverified for the
      weekly fuzz job, which had never actually run in CI — the repo's first dispatch of it
      failed. The `canonical_json` target asserted `parse(canonical(v)) == v`
      (representational identity), a claim false by design for any value JCS folds
      (`-0.0` → `0`, `2.0` → `2`) and one that **contradicted its own unit test**, which
      correctly asserts string-level idempotence. It survived only until the fuzzer first
      generated a `-0.0`. Fixed: the target now asserts the same idempotence
      (`canonical(parse(canonical(v))) == canonical(v)`), the exact input is pinned by
      `seed-negative-zero-fold` and a `cargo test` regression, and all three targets now
      run clean (canonical_json 3.5M execs, registry_parse 3.9M, trace_parse 12.8M).)*
- [x] Published to crates.io ([v0.1.0](https://github.com/tomtom215/mcp-conformance/releases/tag/v0.1.0),
      [release run #2](https://github.com/tomtom215/mcp-conformance/actions/runs/27245596142)) —
      bootstrapped per ADR-0007, OIDC trusted publishing from the next release; rustdoc
      complete (docs.rs all-features); README documents the trace format with a worked
      example.

## M2 — Everything server at the Tier-1 bar

**Definition of done**

- [x] `mcp-everything-server` exercises every capability in scope for `2025-11-25`
      (coverage manifest generated from the registry; parity with the TypeScript everything
      server's surface — [register 2.10](01-ecosystem-context.md)) over stdio and
      streamable HTTP. *(2026-06-10: every suite-defined tool/resource/prompt implemented
      — [register 2.15](01-ecosystem-context.md). The committed
      `conformance/coverage-manifest.json` is generated from the tapped suite sessions
      and checked on every `cargo xtask conformance` run: all eight server-party
      registry capability gates declared and active — the manifest gate caught the
      missing `listChanged` declarations, closed by the `test-list-changed` tool — and
      18 distinct wire methods observed. `BLESS=1` regenerates; drift fails the gate.)*
      *(Second audit, 2026-06-11: the "parity with register 2.10" phrase overclaimed —
      2.10's TypeScript surface includes structured output, URL-mode elicitation, and
      async sampling, none of which the suite exercises and one of which the server
      lacked. Structured output is now real: `get-structured-content` mirrors the
      TypeScript tool exactly (derived `outputSchema`, `structuredContent`, the
      backward-compatible text block), pinned by a roundtrip test. URL-mode
      elicitation and async sampling remain deliberate deltas, documented in the
      crate README: URL mode needs a URL-capable client and lands with M3's host;
      async sampling is the tasks pattern, which `2025-11-25` does not define
      (SEP-2663 moves tasks to an extension in `2026-07-28` — register 1.5a).)*
- [x] **100% pass on the official suite's server scenarios** (pinned version) in CI via
      `cargo xtask conformance` — the hard gate from here forward. 40/40 checks, suite
      0.1.16, spec `2025-11-25`:
      [CI run #27266174013](https://github.com/tomtom215/mcp-conformance/actions/runs/27266174013)
      ("Conformance (official suite, server scenarios)" job, 2026-06-10).
- [x] Agreement check live: official-runner verdicts vs validator verdicts diffed in CI;
      zero unexplained divergence (explained ones filed upstream and linked).
      *(2026-06-10: the everything-server's session tap (`--tap-dir`, feature `tap`)
      records every suite session as a validator-ready trace; `cargo xtask conformance`
      replays all of them through `mcp-trace-validator` and enforces the policy against
      `conformance/agreement-divergences.json` (triage class + upstream link required,
      unknown fields rejected). First run: 30 sessions, 1,288 pass / 840 excluded /
      0 not-applicable, one MUST divergence triaged suite-bug
      ([#7](https://github.com/tomtom215/mcp-conformance/issues/7) — the runner's
      dns-rebinding client skips `notifications/initialized`; filed upstream 2026-06-11
      as [conformance#338](https://github.com/modelcontextprotocol/conformance/issues/338)
      after source-level verification against suite 0.1.16 and main) and one SHOULD warn
      (TRAN-018: the suite's version-compat probe sends a 2025-03-26 header after
      negotiating 2025-11-25 — informational by design). Reconciliation artifact:
      `target/conformance/agreement.json`.)*
- [x] `Host`/`Origin` validation on by default with tests proving 403 behavior
      ([05-security-model.md](05-security-model.md)) — middleware + rmcp transport check
      kept in sync from one policy; in-process 403 matrix, real-process loopback test,
      and the suite's `dns-rebinding-protection` scenario all green (2026-06-10).
- [x] Upstream conversation opened: everything-server offered to
      `modelcontextprotocol/rust-sdk` (issue or draft PR), linked from the README whatever
      the outcome. *(2026-06-11: offered as
      [rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902) —
      pre-flight record and posted text in
      [#9](https://github.com/tomtom215/mcp-conformance/issues/9); README links the
      conversation from the everything-server section. Outcome (adopt / fixtures /
      external) tracked in #9; risk R9's 60-day offer clock runs from today.)*

## M2.5 — `2026-07-28` migration readiness (time-boxed)

Re-sequenced ahead of M3 (2026-06-09): multi-revision trace validation is the
deliverable whose value peaks across the migration window and SEP-2596's ≥ 12-month
dual-revision tail ([register 1.4, 1.5b](01-ecosystem-context.md)), and its registry work
cannot start in earnest before the final text ships. The standing RC-tracking
workstream feeds this milestone until then.

First reconciliation (2026-06-11): the draft changelog inventories materially more
than the RC announcement did — `server/discover` is a new server MUST,
`subscriptions/listen` replaces the GET stream and resource subscriptions, tasks move
to an official extension (SEP-2663), the MRTR pattern replaces server-initiated
requests (SEP-2322), three authorization deltas entered mid-window (PR #2862), and
Roots/Sampling/Logging are deprecated (SEP-2577). The stateless state-machine variant
below is therefore a larger build than first scoped; the extraction checklist in the
second DoD line reflects the full inventory.

**Definition of done**

- [x] `applies` revision ranges in the registry format — the slot ADR-0006 deferred —
      with the embedded loader able to serve more than one revision.
      *(2026-06-14: [`AppliesRange`](../../crates/mcp-conformance-core/src/applies.rs) models
      the half-open `[introduced, removed)` interval the architecture named
      ([02-architecture.md](02-architecture.md) §Requirement registry);
      `Requirement::applies_to(revision)` decides force-at-revision, an absent range meaning
      every revision (so every existing `2025-11-25` entry is unchanged).
      [`RegistrySet`](../../crates/mcp-conformance-core/src/requirement/set.rs) carries the
      union of requirements across revisions and projects to a single-revision `Registry`
      via `registry(revision)`, sharing one definition of well-formed with the
      single-revision loader. `RegistrySet::builtin()` describes the sole shipped revision
      and projects byte-for-byte to `Registry::builtin_2025_11_25()`; the multi-revision
      behaviour — applicability filtering, unknown-revision → `None`, and the dead-data
      (`applies` matches no described revision) rejection — is pinned with synthetic
      ≥2-revision data. Local `cargo xtask ci` green; MSRV-1.88 clippy green.)*
- [x] `2026-07-28` registry entries extracted from the **final** spec text by the same
      per-requirement method (live fetch → verbatim quote → check or documented
      exclusion), behind the `draft-2026-07-28` feature until the official scenarios
      stabilize; the change inventory in [register 1.5a–1.5d](01-ecosystem-context.md)
      is the extraction checklist (SEP-2575 stateless lifecycle — handshake removal,
      `server/discover`, `subscriptions/listen`, and the
      `ping`/`logging/setLevel`/`notifications/roots/list_changed` removals; SEP-2567
      session removal; SEP-2322 MRTR replacing server-initiated requests; SEP-2663
      tasks extension; SEP-2243 routing headers; SEP-2106 JSON Schema 2020-12;
      SEP-2164 error-code change; SEP-2549 caching metadata; SEP-414 trace context;
      SEP-2468/SEP-837/SEP-2352 authorization deltas; SEP-2577 and SEP-2596
      deprecations plus the RFC 7591 DCR deprecation; and the four 2026-06-27
      additions ([register 1.5d](01-ecosystem-context.md)) — SEP-2322's required
      `resultType` on every result, SEP-2575's removal of SSE stream resumability and
      `Last-Event-ID` from Streamable HTTP, the removal of
      `notifications/elicitation/complete` + the URL-mode `elicitationId`, and the
      error-code allocation policy with its renumbering
      (`-32001→-32020`, `-32003→-32021`, `-32004→-32022`) and added `HeaderMismatchError`
      — reconciled 2026-06-11, extended 2026-06-27; Major #2 `_meta` fields refined
      2026-07-19 per [register 1.5e](01-ecosystem-context.md): `clientInfo` demoted
      required→SHOULD, a new `serverInfo`-in-each-result SHOULD added).
      *(Completed 2026-08-17. All fifteen in-scope pages are entered: a 272-entry
      registry, 125 judged / 0 unsupported / 147 excluded, behind `draft-2026-07-28`,
      with `spec-drift` verifying all 412 quotes across both revisions and a passing
      plus a violating corpus trace behind every judged clause. Method and per-area
      decisions:
      [the extraction report](../reports/registry-extraction-2026-07-28-inventory-2026-08-06.md),
      whose addendums are the running log and whose Addendum 7 is the finished state.
      `out_of_scope` in
      [`registry/2026-07-28/sources.json`](../../crates/mcp-conformance-core/registry/2026-07-28/sources.json)
      means deliberately excluded, never "not extracted yet" — a page that is simply
      not reached appears in neither bucket.)*
- [x] Stateless state-machine variant alongside — not replacing — the `2025-11-25`
      machine, every transition and error edge unit- and property-tested.
      *(2026-06-14:
      [`context::stateless`](../../crates/mcp-trace-validator/src/context/stateless.rs) (named `context::draft` until 0.6.0), behind the
      `draft-2026-07-28` feature. The `2026-07-28` rework removes the
      `initialize`/`initialized` handshake (register 1.3, 1.5a; SEP-2575), so the variant's
      defining property is that a session is **operational from its first message** — no
      `BeforeInitialize`/`Ready` gate — with the only remaining handshake-like exchange the
      optional one-shot `server/discover` probe (`Active` ⇄ `AwaitingDiscoverResult`, plus
      the error edge). Every transition and the error edge are unit-tested, and a proptest
      pins the invariants over arbitrary interleavings (starts `Active`; awaiting iff a
      discover request is outstanding; a transition into awaiting is never spurious). Built
      *alongside* the stateful machine (not wired into judgment yet — that needs the line-2
      registry content), and scoped to the lifecycle: per-request `_meta` validation and
      the removed-method prohibitions are checks that land with the final text. **Draft-tracking:**
      the shape follows the SEPs in [register 1.5a–1.5b](01-ecosystem-context.md) and must
      be reconciled against the final `2026-07-28` text. Local `cargo xtask ci` green,
      including the MSRV-1.88 clippy leg over `--all-features`.)*
- [x] Multi-revision judgment: the same trace validated against both revisions in one
      invocation, applicability differences per clause visible in the report.
      *(2026-06-14: [`multi::validate_revisions`](../../crates/mcp-trace-validator/src/multi.rs)
      projects the set to each requested revision, runs the ordinary engine against each,
      and aligns the results into a `MultiReport` — one row per clause carrying its outcome
      under every revision, with a clause that does not exist at a revision reported
      *absent* (`None`) and kept distinct from ADR-0006's capability `not-applicable`.
      Exposed in one invocation via `validate --revision <YYYY-MM-DD>` (repeatable),
      optionally over an external `--registry-set`, in human and JSON form; the
      [`cli`](../../crates/mcp-trace-validator/tests/cli.rs) integration test drives the real
      binary against a two-revision set and reads the per-clause `["pass", null]` /
      `[null, "pass"]` columns. Built and tested against the shipped `2025-11-25` as the
      sole revision plus synthetic ≥2-revision data, so it is ready to receive the
      `2026-07-28` registry content (line 2) the day the final text ships.)*
- [x] `corpus/draft/` good and violation pairs green against the final text, with
      provenance-ledger rows. *(Holds on every `cargo test`: the golden suite replays every
      `corpus/draft/` trace through the `2026-07-28` registry, and the corpus invariant test
      fails a trace with no row in `corpus/README.md`'s provenance ledger. Ticked 2026-10-12.)*

## M3 — Reference host

*(Opened 2026-06-11; ADR-0009 records the design, the pinned suite's client-SUT
contract, and — §Amendment 2026-06-12 — the decoded client verdict rules and the
measured rmcp SSE-resumption gap (register 3.12). Landed: the scriptable
interaction layer; the `rmcp::ClientHandler` with URL-mode elicitation handling,
now exercised end to end against the server's `test_url_elicitation`; the bounded
loop with every stop condition tested; both real transports from rmcp's official
client features (child-process stdio, streamable HTTP over reqwest); the binary
honoring the runner's contract with its own `--deadline-secs` watchdog; host-side
trace capture pinned against the validator's reader and engine; and the compliant
SSE-resumption dance — `retry` honored through
`RetryPolicy::delay_honoring_retry_after`, `Last-Event-ID` offered — on rmcp's
public `StreamableHttpClient` seam. All four `2025-11-25` client scenarios pass
at pinned 0.1.16 in local runs (`initialize`; `tools_call` 1/1;
`elicitation-sep1034-client-defaults` 5/5; `sse-retry` 3/3). Still open below:
the xtask/CI wiring that turns those runs into the standing gate (with the
client-side agreement replay), which is also where the child-process spawn gets
its real-binary proof. The suite's `auth/*` client scenarios are deferred,
matching TRAN-009's registry record.)*

**Definition of done**

- [x] Host completes bounded tool-use loops against the everything server over stdio and
      streamable HTTP: every stop condition (turn limit, completion, cancellation, error
      budget) tested. *(Stop-condition lattice in-process (`agent_loop`); streamable HTTP
      over a real socket (`transports.rs`); stdio between the two real binaries in the
      conformance gate's smoke — [run #27449549660](https://github.com/tomtom215/mcp-conformance/actions/runs/27449549660).)*
- [x] Official suite **client scenarios** pass with the host as SUT (`--command` wiring,
      pinned version). *(The standing gate: `initialize`; `tools_call` 1/1;
      `elicitation-sep1034-client-defaults` 5/5; `sse-retry` 3/3 — sequential by design,
      client runs fail on WARNINGs (ADR-0009 §Amendment). `auth/*` deferred:
      deferral-ledger row `auth-client-scenarios`, registry TRAN-009.)*
- [x] Sampling, elicitation (including URL mode), and roots handlers scriptable for CI;
      zero model-provider network use. *(`script` is data; no code path can dial a
      provider. URL mode round-trips end to end against `test_url_elicitation`.)*
- [x] Backoff/retry honoring `Retry-After`, jittered, with budget tests; SSE resumption via
      event-id cursors where applicable. *(`retry.rs` property-tested since v0.1.0; the
      `resume` dance honors the server-named `retry` through
      `delay_honoring_retry_after` and offers `Last-Event-ID` — measured by the suite's
      own clock, 3/3.)*
- [x] Host-side trace capture emits validator-ready traces with default redaction
      ([05-security-model.md](05-security-model.md)). *(`capture` records at the message
      seam — headers are unobservable there, so credentials cannot leak by construction;
      output pinned against the validator's reader and engine, and replayed in the
      client agreement.)*

## M4 — Upstream engagement (gate, not phase)

Backlog opens at M0; the milestone closes only on merged outcomes.

**Definition of done**

- [ ] ≥ 1 substantive merged PR in `modelcontextprotocol/rust-sdk` or
      `modelcontextprotocol/conformance` (everything-server adoption, conformance scenario,
      MSRV policy, transport hardening — backlog in
      [07-ecosystem-engagement.md](07-ecosystem-engagement.md)).
- [ ] RustSec advisory for CVE-2026-42559 filed in coordination with rmcp maintainers, or
      upstream's documented decision not to ([register 4.3](01-ecosystem-context.md)).
- [x] A public design note (in-repo) explaining the trace-validation architecture and
      trade-offs, linkable from upstream discussions. *(2026-06-13:
      [docs/design/trace-validation.md](../design/trace-validation.md) — written for an
      external/upstream audience, standalone from the planning docs; ten decisions and their
      costs, with the agreement check (continuous calibration against the official suite) as
      the credibility mechanism. The SemVer §9 records the verdict-as-contract posture the
      `cargo-semver-checks` dimension verified this round.)*

## M5 — Stewardship artifacts

**Definition of done**

- [x] Published tier-gap report for rmcp: official `tier-check` output + requirement-level
      findings + a concrete close-the-gap checklist; method reproducible from artifacts.
      *(2026-06-13: [docs/reports/rmcp-tier-gap-2025-11-25.md](../reports/rmcp-tier-gap-2025-11-25.md).
      Server scenarios re-measured live against the pinned suite 0.1.16 at rmcp head
      `266f870`: 38/40, failing `prompts-get-with-args` and `elicitation-sep1330-enums`
      (the latter is [rust-sdk#903](https://github.com/modelcontextprotocol/rust-sdk/issues/903)
      / register 3.8 — fixed upstream by merged [rust-sdk#905](https://github.com/modelcontextprotocol/rust-sdk/pull/905)
      2026-06-20, not yet released). Requirement-level reading: neither is a `2025-11-25` normative-clause
      violation — one sits below the registry's MUST floor (arg substitution is schema
      prose), the other is a SEP-1330 serialization bug — plus a close-the-gap checklist and
      reproducible commands. Honest scope: the authoritative figure is the suite's `server`
      subcommand; `tier-check` itself is GitHub-token-gated and its conformance counter
      carries conformance#182's "0/30" bug, both documented in the report rather than
      reported as a misleading number.)*
- [x] Optionally the same report for one community SDK (e.g. `pmcp`) to prove generality.
      *(2026-06-14: [docs/reports/pmcp-tier-gap-2025-11-25.md](../reports/pmcp-tier-gap-2025-11-25.md).
      The community Rust SDK `pmcp` 2.9.0 ships no suite-wired server, so the same method
      was applied by building a standalone pmcp SUT ([docs/reports/pmcp-harness/](../reports/pmcp-harness/),
      a non-workspace project — pmcp's MSRV 1.91 never enters our 1.88 workspace) and
      running the pinned suite 0.1.16 against it: **16/30 server scenarios** (17/32 checks),
      recomputed directly from the 30 committed `checks.json` files. The 14 failures were
      iterated down to only pmcp-attributable causes (one harness wiring gap fixed first)
      and each was root-caused twice — over the wire and against pmcp source — into six
      limitations of pmcp's Streamable-HTTP surface (no server→client back-channel for
      logging/progress/sampling/elicitation — transport-scoped; stringified tool output;
      flat `Content::Resource`; no reachable resource `blob`; stubbed `completion/complete`;
      strict protocol-version exact-match). The self-built-SUT caveat is stated plainly in
      the report. Method fully reproducible from the committed harness + commands.)*
- [x] mdBook live (architecture, trace format, corpus guide, conformance results page);
      docs.rs complete for all crates.
      *(Live 2026-06-14, on the custom apex domain <https://mcp-conformance.com> since 2026-07-27 (the GitHub Pages address redirects there).
      [`book/`](../../book) carries five chapters — Introduction,
      Architecture, The trace format, The trace corpus, Conformance results —
      covering all four required areas. The trace-format and corpus chapters embed
      the README's worked example and `corpus/README.md` verbatim via `{{#include}}`
      (the README example anchored and already pinned to the validator's real output
      by `readme_examples.rs`), so the book cannot drift from its sources; the `book`
      CI job runs `mdbook build book` — green in [CI run #27481899846](https://github.com/tomtom215/mcp-conformance/actions/runs/27481899846) — which fails on a
      missing include file or anchor. **docs.rs completeness is enforced, not assumed:** `missing_docs =
      "deny"` in the workspace lints plus the `--all-features` rustdoc gate under
      `-D warnings` (the `doc` CI job). The deploy ran: `pages.yml` (all actions
      SHA-pinned) built and published the book on the v0.3.0 merge to `main` —
      [Pages run #27493955091](https://github.com/tomtom215/mcp-conformance/actions/runs/27493955091),
      success — and the live site returns `200`. docs.rs rendered all four crates
      at `0.3.0` (`doc_status: true` on each `status.json`), so both clauses of
      this line now hold.)*
- [x] The `draft-2026-07-28` feature gate dropped (revision becomes default).
      *(2026-09-24, [ADR-0018](decisions/0018-judge-the-declared-revision.md): the text has
      shipped and every in-scope page is entered; the suite-stability condition now gates
      only the blocking `2026-07-28` agreement check.)*

## Standing workstreams

| Workstream | Cadence | Content |
|------------|---------|---------|
| RC tracking | Each upstream RC change | Reconcile draft-revision expectations against the latest text; feeds M2.5, which re-scopes if the rework shifts materially ([08-risk-register.md](08-risk-register.md)). **Closed 2026-07-28** when the final text shipped with the inventory unchanged ([register 1.5h](01-ecosystem-context.md)) — there is no longer a draft to track against, and its successor is the extraction itself (M2.5 line 2). Upstream's `draft/` now accumulates the revision *after* `2026-07-28`; re-open this row if and when that becomes a target |
| Suite tracking | Scheduled CI | Pinned-stable upgrades as deliberate PRs; `0.2.0-alpha` watched non-blocking. The alpha job floats only the *suite binary* and stays at `2025-11-25`, so it says nothing about the next revision — **that gap is now closed by its own gate** (below) rather than by overloading this one |
| Draft readiness | Scheduled CI (ratcheted) | `cargo xtask draft-readiness` drives the official runner's **`2026-07-28` scenarios** (pinned `0.2.0-alpha.12`) against the everything server serving each revision, and fails on any change from the committed [`conformance/draft-readiness.json`](../../conformance/draft-readiness.json); a change is recorded deliberately with `BLESS=1`. Current (2026-10-12, rmcp 3.5.1): **42 passing / 0 failing** on both legs. How it got there — the runner's `wire-schema-valid` check independently confirming CACH-001, and rmcp 3.5.1 closing it — is in [corpus/README.md](../../corpus/README.md) and git history |
| Register upkeep | 90-day sweep | Re-verify [01-ecosystem-context.md](01-ecosystem-context.md) rows before external use |
| Claims expiry | Weekly scheduled CI (ADR-0010) | `cargo xtask deferrals --check` fails once a [deferral-ledger](deferrals.json) row passes its review-by date; `cargo xtask spec-drift` re-verifies every registry quote against the published spec text |
| Upstream presence | Continuous | Issue triage participation and small fixes in rust-sdk/conformance — the relationship M4 depends on is built before it is needed |

## The 2026-09-24 review's plan

[The review](../reports/project-review-2026-09-24.md) is a dated report and stays as
written; this is where its plan's status lives. Audited item by item on 2026-10-13
against the tree, git history and CI. Items not listed are done: C1, C2, H1, H3, H5,
D1, D3, D4, D6, 0.1, 0.3–0.7, 1.1–1.3, 1.5, 2.1, 4.4, M6, M8, M10.

| Item | Status | What remains |
|------|--------|--------------|
| D2 / 2.4 / H2 — wire-schema validation | **Done** ([ADR-0020](decisions/0020-wire-schema-validation.md)) | — The suite's `wire-schema-valid` is compared message by message with every tapped session's `wire_schema` where the runner reports it (the alpha-tracking run, both draft-readiness legs), against `conformance/wire-schema-divergences.json`: 0 divergences on 2026-10-13, on data where neither side flagged a message. The stable pin `0.1.16` predates the check, so the pinned run compares against its green verdict only |
| D5 — prose-claim gates | Decided: kept (owner, 2026-10-13) | — |
| 0.2 / H4 — scheduled workflow green | Partial | Every cause fixed in the tree; issue #50 closes on the next green scheduled run |
| 1.4 — distribution | Partial (publication pending) | Container image (Dockerfile; built and run in CI; pushed to GHCR with provenance by `release.yml` on the next tag; linux/amd64 only) and the Action's `capture` input (stdio and HTTP, exercised in CI). Not yet observed: a first GHCR publish and the package's visibility. The image job runs beside the crates.io publish, not after it, so a release that fails later can leave an image published |
| 1.6 / M7 — everything-server tap | **Done** | — `seq` is observation order; requests naming a session the server never assigned share one `unassigned` trace per run; per-session state is kept for the run by design (releasing it on `DELETE` would split a session whose stream records after it) |
| 1.7 / 3.2 — other SDKs | **Done** (pending the first scheduled run) | TypeScript, Python, Go and Rust conformance servers recorded by `interop/matrix.sh`, committed with provenance and generated into `book/src/sdk-matrix.md`; the weekly `sdk-matrix` job re-records and compares; worked TypeScript and Python guides in `book/src/sdk-servers.md`, pinned to the job's script. Remaining: TypeScript at `2026-07-28` awaits an SDK line that serves it; the session script is small (none of these servers lists a tool the client calls) |
| 2.2 / M2 — census | **Done** | — The census counts MUST-, SHOULD- and MAY-family keywords, table rows included; every MUST- and SHOULD-family instance must sit inside an entry's quote; 0 unentered on both revisions (2026-10-13). MAY-family keywords are counted, not required to be entered (a permission has nothing a trace can violate) |
| 2.3 / M1 — disputed exclusions | **Done** | — `TRAN-020`, `TRAN-083`/`TRAN-084` (with their restatements `TOOL-046`/`TOOL-035`), `BASE-033`/`BASE-037` and `DISC-003` are judged, each with a passing and a violating corpus trace (2026-10-13); the rest of M1 had already landed. `BASE-037` now warns on every rmcp-served capture (rmcp puts `serverInfo` only on `server/discover` and `subscriptions/listen` results), which leaves the manual `cargo xtask draft-capture` red until the server or rmcp sets it on every result |
| 2.5 / M3 — server MUSTs on client-feature pages | **Done** | — `client/elicitation`, `client/sampling` and `client/roots` are in scope at both revisions, every MUST- and SHOULD-family keyword entered (161 entries; 45 judged by 26 checks with corpus pairs, the rest excluded with reasons), URL-mode `url` validity (RFC 3986) and `sampling.tools` judged at both. Found: the Go SDK fails `ELIC-059`; this project's reference host warns `ELIC-095` (its `AcceptWithDefaults` answers a form whose schema requires fields with empty content) |
| 2.6 / M4 — extensions (tasks, skills) | Not started | — |
| 2.7 — everything-server surface | **Done** | — Pagination (all four listings, stateless cursors, `-32602` for one never issued), icons, `resource_link`, annotations, an audio prompt and `x-mcp-header` designations, served on second pages so the suite's catalogue is untouched; the captures evidence 137 of the 147 judgeable `2026-07-28` clauses (was 127). The ten not observed are listed by clause in `corpus/README.md`. Tasks are left to 2.6 (an extension at `2026-07-28`) |
| 2.8 — authorization, observable subset | Done 2026-10-13 | [ADR-0022](decisions/0022-judge-the-challenge-not-the-flow.md); `AUTH` entries at both revisions (12 judged by 7 challenge checks, the rest excluded with reasons); deferral row `auth-client-scenarios` narrowed to the host-as-SUT half |
| 2.9 — draft tracking | **Done** | — `registry/draft/` holds a tracking skeleton (page set and census) and `post-release.json` (`TRAN-081`'s post-release rewording, bound unchanged); `cargo xtask spec-drift --draft` runs in the weekly `draft-drift` job, informational. On 2026-10-13 the draft kept the page set and census and 313 of 314 released quotes; the 314th is `TRAN-081` |
| 3.1 / M5 — calibration at `2026-07-28` | Blocked | Needs a stable official suite `0.2.0` (deferral row `suite-0-2-0-stable-pin-bump`) |
| 3.3 — second calibration source | **Done** ([book/src/mcpsnoop.md](../../book/src/mcpsnoop.md)) | Five upstream `mcpsnoop` issues drafted for the owner to file; one open spec-ambiguity item (a legacy `initialize` inside a `2026-07-28` trace: is it held to that revision's request-envelope clauses?) |
| 3.4 — a2a-rust ports | **Done** | Per-PR semver checks, per-crate SBOMs, PR fuzzing, generated requirement pages, a coverage floor (95% of lines; 96.40% measured) and `cargo xtask falsify-gates` (every offline gate and the golden corpus) all run in CI |
| 4.1–4.3 — upstream | Owner | Close #9; file the clause-level-rows proposal and the two small upstream PRs |
| Phase 5 — proportionality | Partial | Roadmap condensed; CHANGELOG still carries audit narrative; merged branches not deleted |
| Low — `#[allow]` attributes | **Done** | — `#[allow(deprecated)]` sits on the item or statement using SEP-2577's Roots, Sampling or Logging; no module-wide allow remains |

## The workbench

[ADR-0021](decisions/0021-a-workbench-people-keep-open.md), decided 2026-10-13 by the
owner: make the clause-level reading something people use daily, alongside the official
suite and `mcpsnoop`. One phase at a time; each closes only against its definition of done.

| Phase | Work | Done when | Status |
|-------|------|-----------|--------|
| W1 | `mcp-conformance` crate; `test` runs the pinned suite through capture against a server or client and reports both verdicts | A server and a client run end to end in CI through `test`; its report agrees with `cargo xtask conformance` on the same subject | Done in code, pending CI: `cargo xtask conformance`'s product leg runs `test` against the everything server (HTTP, stdio) and the reference host and holds it to the gate's results (same suite statuses; the gate's failures a subset of `test`'s; the rest listed in `conformance/wire-findings.json`: 4 triaged, 2 suite bugs fixed in `0.2.0-alpha.12` and 2 open revision-attribution findings; `BASE-009` was a validator false positive, fixed). The crate is `mcp-conformance-cli` (owner, 2026-10-13; ADR-0021), `publish = false` until its first release's bootstrap |
| W2 | HTML report (`--format html`) | Golden HTML for the corpus; screenshot baselines for a passing, a failing and a two-revision report; no network requests when opened | Done, pending CI's first `visual` run: nine pages pinned as golden HTML (every corpus trace renders and is checked; pinning all would be 18.8 MB); passing, failing and two-revision pages at 1280 and 360 px in light and dark under screenshot baselines; no request when opened, asserted by request interception. Not checked in Firefox, Safari or a screen reader |
| W3 | npm and PyPI packages carrying the release binaries; container image (1.4) | `npx mcp-conformance --version` and `pip install` + run work on Linux, macOS and Windows runners from locally built packages | Done in code, pending CI's first macOS and Windows runs and the owner's registry setup: `cargo xtask packages` builds `mcp-conformance` plus five `@mcp-conformance/<os>-<cpu>` npm packages and five wheels from the release archives; installed and run on Linux x86_64. Publishing jobs (npm provenance, PyPI trusted publishing) are in `release.yml`, untested until a tag. Wheels are stored uncompressed (19.7 MB for Linux x86_64) |
| W4 | `watch`: live judging over capture, `--otlp` export under the MCP semantic conventions | Findings print within one exchange of the violating message on a corpus replay; exported spans accepted by an OpenTelemetry Collector in CI | Done in code, pending CI's first `otlp` run: `watch stdio`/`http` record as capture does and judge live; over every corpus trace nothing printed live is withdrawn or missed, and a violation replayed through `watch stdio` prints before the client's next message. Spans follow semconv-genai at `5293effb`; accepted by the pinned Collector 0.162.0 locally. Not covered by a test: `watch http`, the terminal tally, Windows |
| W5 | `serve`: library, search, timeline, inspector, diff, live tail, dashboards, administration (tags, retention, delete, export, redaction), importers (`mcpsnoop`, MCP Inspector) | Every view under screenshot baselines and end-to-end interaction tests; the security model's workbench section implemented and tested (token, Host/Origin, CSP, text-only rendering) | Done in code, pending CI: every view has screenshot specs and end-to-end interaction tests, and the security section is implemented and tested; the twenty workbench baselines are a local draft until the CI runner's rendering replaces them. Not run: the diff-scoped mutation gate on the new code; macOS and Windows. Known limit: browsers do not separate cookies by port (security model) |

## Sequencing rules

1. M0 strictly precedes everything; no feature code on an unscaffolded repo.
2. M1 strictly precedes M2 (the agreement check needs the validator).
3. M2.5 opens when the `2026-07-28` final text ships and takes precedence over M3
   wherever the two contend for effort; M3 may proceed in parallel where they do not.
4. M2 and M3 may overlap after M2's server passes `core` scenarios.
5. M4 has no ordering constraint — earliest credible moment wins; M5 closes last.
6. Any risk trigger in [08-risk-register.md](08-risk-register.md) firing forces a roadmap
   review before the next milestone proceeds.
