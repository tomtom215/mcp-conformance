<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Roadmap

**Status:** Active
**Last reviewed:** 2026-07-26

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
| M1 — Registry and validator | **Complete** — v0.1.0 published to crates.io via [release run #2](https://github.com/tomtom215/mcp-conformance/actions/runs/27245596142) (attested, byte-verified); every DoD line below carries its evidence |
| M2 — Everything server | **Complete** (2026-06-11): server live on rmcp 1.7 over stdio + policy-gated streamable HTTP; **40/40 checks green** against the pinned suite in CI ([run #27266174013](https://github.com/tomtom215/mcp-conformance/actions/runs/27266174013)); trace tap, agreement check, and coverage manifest live (zero unexplained divergence; first divergence triaged suite-bug, filed upstream as [conformance#338](https://github.com/modelcontextprotocol/conformance/issues/338)); everything-server offered upstream as [rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902) (pre-flight in [#9](https://github.com/tomtom215/mcp-conformance/issues/9)), README-linked — every DoD line below carries its evidence |
| M2.5 — `2026-07-28` migration readiness | **In progress** — every part buildable ahead of the final text is now done: DoD lines 1, 3, and 4 are closed — `applies` revision ranges + a multi-revision loader (`RegistrySet`), the stateless state-machine variant (`context::draft`, behind `draft-2026-07-28`), and multi-revision judgment (`validate_revisions` → `MultiReport`), all tested against the shipped `2025-11-25` plus synthetic/draft data. Only the registry *content* (lines 2, 5) remains, gated on the final text shipping (2026-07-28): the `2026-07-28` registry entries and `corpus/draft/` pairs need the live final text via the extraction method, and the stateless variant must be reconciled against it. Re-sequenced ahead of M3 on 2026-06-09; extraction checklist re-scoped 2026-06-11 — the first RC-tracking reconciliation against the draft changelog ([register 1.5a–1.5b](01-ecosystem-context.md)) surfaced four majors the RC announcement never enumerated (`server/discover`, `subscriptions/listen`, tasks-as-extension, MRTR) plus the Roots/Sampling/Logging deprecations; a 2026-06-27 RC re-reconciliation ([register 1.5d](01-ecosystem-context.md)) extended the checklist by four more — required `resultType` on every result, **removal of SSE stream resumability/`Last-Event-ID`** from Streamable HTTP, removal of `notifications/elicitation/complete` + the URL-mode `elicitationId`, and the error-code allocation/renumbering. **Status at 2026-07-26 (T−2 from the ship date):** the final text has still not shipped ([register 1.5f](01-ecosystem-context.md) — `/specification/2026-07-28` 404, no `docs/specification/2026-07-28/` on the spec repo's `main`), and the draft changelog has been substantively frozen for five weeks (nine Major / twelve Minor / four Deprecated, unchanged since the 2026-06-27 delta apart from the 2026-07-19 Major-#2 wording refinement). Lines 2 and 5 stay gated exactly where they were; nothing further is buildable ahead of the text. **Status at 2026-07-28 (ship day): the final text SHIPPED, and lines 2 and 5 are UNBLOCKED** ([register 1.5h](01-ecosystem-context.md) — `/specification` now resolves to `/specification/2026-07-28`, the site's version dropdown marks it `(latest)` and `default`, upstream's `draft/changelog.mdx` was reset to its post-release stub, and `schema/2026-07-28/schema.ts` is on `main`). The shipped changelog carries **exactly the inventory this milestone already planned for** — nine Major, twelve Minor, four Deprecated, identical to the 1.5d/1.5e counts — so the extraction checklist needs no ship-day re-scoping and the work is the one described in line 2, not a re-plan. What did **not** change: [ADR-0011](decisions/0011-rmcp-pin-holds-at-1-7.md)'s rmcp pin, whose second condition (a stable `3.0.x`) is still unmet at `3.0.0-beta.4` / `max_stable` 2.2.0, and the suite pin, whose `latest` is still 0.1.16. Per the ADR's [2026-07-28 amendment](decisions/0011-rmcp-pin-holds-at-1-7.md), that does not gate this milestone: extraction proceeds against the released text now, and the SDK upgrade joins it when a stable `3.0.x` exists. **Status at 2026-08-06: that stable `3.0.x` exists — the pin is released** ([register 3.14](01-ecosystem-context.md); [ADR-0011 2026-08-06 amendment](decisions/0011-rmcp-pin-holds-at-1-7.md)). `rmcp 3.0.0` published on 2026-07-28 itself and the line is now at `3.1.1` (2026-08-05), MSRV `1.88`, **retaining `2025-11-25` support with `ProtocolVersion::LATEST` still `V_2025_11_25`** — so the 40/40 calibration gate survives the upgrade and the milestone's two halves (SDK upgrade + registry extraction) can finally run as the single measured change the ADR specifies. Both of this milestone's external gates are now clear; the *third* project-wide gate — a stable official suite — remains shut (`latest` still 0.1.16) and holds M5's feature-gate drop, not this milestone. What *has* arrived is a reference implementation — rmcp's `3.0.0-beta` line ships the stateless lifecycle, `server/discover`, tasks-as-extension, MRTR and the TTL cache ([register 3.1](01-ecosystem-context.md)) — usable as a cross-check when extraction starts, never as a quote source. **The rmcp upgrade folded into this milestone is now a three-way decision, not a bump:** `1.8.0` (enumNames + the two behaviour changes already triaged), `2.2.0` (current stable, what Dependabot [#28](https://github.com/tomtom215/mcp-conformance/pull/28) proposes — ~70 compile errors, including `_meta` fields on notification params and the SEP-2577 deprecations), or the `3.0.x` line that will be the `2026-07-28` target. **Decided 2026-07-27 in [ADR-0011](decisions/0011-rmcp-pin-holds-at-1-7.md):** the pin holds at `1.7.0` — no interim `2.2.0`, no `3.0.0-beta` — until *both* the final text ships and rmcp cuts a stable `3.0.x`, at which point the upgrade and this milestone's registry extraction are one piece of work. The `rmcp-macros` ceiling shim makes that mechanical rather than a promise, and the `adopt-rmcp-enumnames-fix` deferral forces a re-decision on 2026-09-01 regardless. **Status at 2026-08-08: both halves are running, and line 2 is under way at 2 of 15 in-scope pages.** The rmcp upgrade landed (`3.1.2`, proven by a before/after wire diff). Extraction has entered `basic/index` (57 clauses) and `basic/transports/streamable-http` (60), giving a `2026-07-28` registry of 117 entries — 51 judged, 0 unsupported, 66 excluded — with every quote verified live (`spec-drift`: 257 quotes across both revisions, 0 drifted) and every judged clause backed by a passing and a violating corpus trace. Nothing in the registry is aspirational: the `PLANNED` ledger that lets entries land ahead of their checks is empty. The page-by-page backlog, the method, and the decisions taken per area live in [the extraction report](../reports/registry-extraction-2026-07-28-inventory-2026-08-06.md), which is the handover document for this line; the remaining thirteen pages were the work. **Status at 2026-08-17: line 2 is COMPLETE — all fifteen in-scope pages are entered.** The `2026-07-28` registry is 272 entries — 124 judged, 0 unsupported, 148 excluded — with `spec-drift` verifying 412 quotes across both revisions, 0 drifted, and every judged clause backed by a conforming and a violating trace. Upstream had not moved: the spec repo's `docs/specification/2026-07-28/` tree is untouched since publication. Six defects were found and fixed on the way, five of them live wrong answers rather than gaps — most notably five capability checks that could only have reported vacuous passes at this revision, because they resolve declarations through the removed `initialize` result. The remaining DoD work is line 5's *captured* corpus, which needs an implementation of the stateless surface (M3) rather than more curation |
| M3 — Reference host | **Complete** (2026-06-13; ADR-0009 + §Amendment): both transports live (child-process stdio, streamable HTTP over reqwest); **all four `2025-11-25` client scenarios pass at pinned 0.1.16 as the standing CI gate**, with the two-real-binaries stdio smoke and the client-side agreement replay (zero unexplained divergence) — [run #27449549660](https://github.com/tomtom215/mcp-conformance/actions/runs/27449549660), "Conformance (official suite, server + client scenarios)"; host trace capture pinned against the validator; SSE resumption honors the server-named `retry` with `Last-Event-ID` (rmcp 1.7's measured gap is register 3.12; the host ships the compliant dance on rmcp's public seam); `auth/*` deferred with an enforced ledger row — every DoD line below carries its evidence |
| M4 — Upstream engagement | In progress (gate, not phase; closes only on merged outcomes) — the public design-note DoD line is **done** ([docs/design/trace-validation.md](../design/trace-validation.md), 2026-06-13: the trace-validation architecture and its trade-offs, written standalone for an upstream audience); the two merged-outcome lines remain open and owner/upstream-gated ([rust-sdk#902](https://github.com/modelcontextprotocol/rust-sdk/issues/902), [rust-sdk#903](https://github.com/modelcontextprotocol/rust-sdk/issues/903), [conformance#338](https://github.com/modelcontextprotocol/conformance/issues/338) filed and tracked — **rust-sdk#903 is now resolved upstream (fixed by merged [rust-sdk#905](https://github.com/modelcontextprotocol/rust-sdk/pull/905), 2026-06-20; maintainer-authored, implementing our report — a successful engagement but not *our* merged PR, so this line stays open)**; a merged floors/MSRV PR, the empirically-verified SSE-resumption fix (register 3.12), or the RustSec advisory for CVE-2026-42559 is the substantive merge the DoD requires — backlog in [07-ecosystem-engagement.md](07-ecosystem-engagement.md)). Outward-facing submissions are drafted, **pending owner authorization; none posted**: the [MSRV-policy issue](../reports/rust-sdk-msrv-policy-issue-draft.md) (register 3.5) is ready to file; the [SSE-resumption filing](../reports/rmcp-sse-resumption-dossier.md) (register 3.12) was **re-decided 2026-06-27 and is no longer the candidate it was** — a standing RC re-reconciliation ([register 1.5d](01-ecosystem-context.md)) found the `2026-07-28` draft *removes* SSE stream resumability and `Last-Event-ID` (SEP-2575), obsoleting the dossier's proposed fix; the `2025-11-25` mechanism stays exact (re-verified first-hand 2026-06-27 at head `eb435c6`) but the filing is deferred to the post-spec window and re-scoped to the durable hang (deferral re-dated to 2026-09-01); so the strongest *buildable-now* candidate is the small rmcp dependency-floors PR + `from_build_env` docs fix ([backlog #9](07-ecosystem-engagement.md), register 3.9) — now **drafted ready-to-file** ([docs/reports/rmcp-build-out-hygiene-draft.md](../reports/rmcp-build-out-hygiene-draft.md), 2026-06-27), owner/authorization-gated (the floors are a decliney ask given `-Z minimal-versions` contentiousness; `from_build_env` is the higher-confidence piece). **Re-verified against rmcp 2.2.0 on 2026-07-19 ([register 1.5e/3.9](01-ecosystem-context.md)): the floors ask shrank** — the `tokio-util` claim is obsolete at 2.2.0 (usage removed in the 2.x refactor), leaving essentially a lone `tokio-stream` bump, so `from_build_env` (still buggy in released 1.8.0/2.2.0) is now clearly the piece to lead with; the [RustSec advisory for CVE-2026-42559](../reports/rmcp-cve-2026-42559-rustsec-advisory-draft.md) is drafted but **blocked** — a 2026-06-14 pre-filing check found the CVE + rmcp's GHSA already aliased onto the dynoxide-rs advisory RUSTSEC-2026-0140 (register 4.3), so an `rmcp`-keyed advisory would collide; the gap (direct `rmcp < 1.4.0` dependents) is real but needs RustSec-maintainer reconciliation, not a drop-in PR (re-checked 2026-07-26 against advisory-db `main`: unchanged, still blocked). **2026-07-26 re-assessment of the candidate set:** the MSRV-policy issue — the backlog's item 3 and one of the two remaining ready-to-file drafts — was **resolved upstream on 2026-07-23 without us**, by [rust-sdk#1034](https://github.com/modelcontextprotocol/rust-sdk/pull/1034) declaring `rust-version = "1.88"` (our measured floor, exactly) plus an MSRV CI job; the draft is obsolete and must not be filed ([register 3.5](01-ecosystem-context.md)). That is the third finding of ours to be fixed or obsoleted upstream before filing (enumNames via #905, SSE-resumption via SEP-2575's removal, now MSRV) — the engagement is working while the DoD's *merged-PR* line stays open. The **new strongest buildable-now candidate** is [register 3.13](01-ecosystem-context.md): rmcp caret-pins its own `rmcp-macros`, so Cargo resolves `rmcp 1.7.0` + `rmcp-macros 1.8.0`, a pair that fails to compile (E0425 at every `#[tool]`) — reproduced first-hand, unfixed at `3.0.0-beta.2`, and fixable with a one-line `=N.M.P` pin (the `serde`/`serde_derive` convention). It is smaller and more obviously correct than the floors ask, which the 2.2.0 re-check had already shrunk to essentially one `tokio-stream` bump; `from_build_env` remains the other solid piece |
| M5 — Stewardship artifacts | In progress — the rmcp tier-gap report is published ([docs/reports/rmcp-tier-gap-2025-11-25.md](../reports/rmcp-tier-gap-2025-11-25.md): 38/40 server scenarios at rmcp head `266f870`, re-measured live 2026-06-13); the mdBook is **built and CI-gated** (2026-06-13; [`book/`](../../book): five chapters across all four required areas, the trace-format and corpus chapters embedded verbatim from canonical sources via `{{#include}}`, with `mdbook build book` run on every push by the `book` CI job) and **live since 2026-06-14** at <https://mcp-conformance.com> (deployed by [Pages run #27493955091](https://github.com/tomtom215/mcp-conformance/actions/runs/27493955091) on the v0.3.0 merge; **custom apex domain since 2026-07-27** — verified live that day: apex `200` over HTTPS with a valid certificate, `http://` and `www.` both `301` to it, and the former `tomtom215.github.io/mcp-conformance/` address now `301`s there too, so no prior link breaks), with docs.rs having rendered all four crates at `0.3.0` (`doc_status: true`); the optional `pmcp` report is now **published** ([docs/reports/pmcp-tier-gap-2025-11-25.md](../reports/pmcp-tier-gap-2025-11-25.md): a measured **16/30** server scenarios for `pmcp` 2.9.0 over its Streamable-HTTP transport, suite 0.1.16 / spec 2025-11-25, with a committed reproducible harness and source-verified per-failure attribution — proving the method generalizes); only the `draft-2026-07-28` feature-gate drop remains |

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
      audit. Now judged by `prompts.arguments-validated` with a killer trace: **140
      entries, 52 judged by 48 checks, 88 exclusions**. The check is deliberately narrow
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
      registry, 124 judged / 0 unsupported / 148 excluded, behind `draft-2026-07-28`,
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
      [`context::draft`](../../crates/mcp-trace-validator/src/context/draft.rs), behind the
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
- [ ] `corpus/draft/` good and violation pairs green against the final text, with
      provenance-ledger rows.

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
- [ ] The `draft-2026-07-28` feature gate dropped (revision becomes default) — only after
      the final text has shipped, M2.5 is complete, and the official scenarios for the
      revision stabilize.

## Standing workstreams

| Workstream | Cadence | Content |
|------------|---------|---------|
| RC tracking | Each upstream RC change | Reconcile draft-revision expectations against the latest text; feeds M2.5, which re-scopes if the rework shifts materially ([08-risk-register.md](08-risk-register.md)). **Closed 2026-07-28** when the final text shipped with the inventory unchanged ([register 1.5h](01-ecosystem-context.md)) — there is no longer a draft to track against, and its successor is the extraction itself (M2.5 line 2). Upstream's `draft/` now accumulates the revision *after* `2026-07-28`; re-open this row if and when that becomes a target |
| Suite tracking | Scheduled CI | Pinned-stable upgrades as deliberate PRs; `0.2.0-alpha` watched non-blocking. The alpha job floats only the *suite binary* and stays at `2025-11-25`, so it says nothing about the next revision — **that gap is now closed by its own gate** (below) rather than by overloading this one |
| Draft readiness | Scheduled CI (ratcheted) | `cargo xtask draft-readiness` drives the official runner's **`2026-07-28` scenarios** against the current server and compares against the committed [`conformance/draft-readiness.json`](../../conformance/draft-readiness.json). Both pins exact. First measurement 2026-07-26: **1 passing / 20 failing / 1 informational across 20 scenarios**, every failure the same HTTP 422 at the removed handshake — write-up in [docs/reports/draft-2026-07-28-readiness-2026-07-26.md](../reports/draft-2026-07-28-readiness-2026-07-26.md). The gate fails on any change in either direction, so M2.5's progress meter cannot drift silently. **2026-07-28:** the revision is no longer a draft, but the *scenarios* still are — the suite's `latest` is 0.1.16 and this ratchet pins the pre-release `0.2.0-alpha.9`, now one behind `0.2.0-alpha.10` (published 2026-07-27). The task keeps its `draft-readiness` name, which is stable across CI, the committed baseline filename, and the report path; renaming is deferred to the `suite-0-2-0-stable-pin-bump` change, where the pins move anyway. **2026-08-17:** two legs, one per revision the server can serve, with scenario keys prefixed by the revision (`2025-11-25/tools-list`). Both score **23 passing / 0 failing / 0 informational across 20 scenarios** — the legacy leg byte-identical to its previous baseline. That the runner cannot tell the two servers apart is itself the finding: its scenarios exercise features, and the registry in this workspace is what separates them (58 pass, 1 fail against the legacy server and 59 pass, 0 fail against the stateless one, with 65 clauses not observed on each, on the two sessions this task now captures into [`corpus/draft/captured/`](../../corpus/draft/captured)). **2026-08-18: pin bumped `alpha.9` → `alpha.11`** (deferral `suite-0-2-0-stable-pin-bump`, re-decided; `alpha.10` 2026-07-27, `alpha.11` 2026-08-07), re-measured with `BLESS=1`. **The "cannot tell them apart" finding above is now superseded:** `alpha.11` adds a `wire-schema-valid` check against the negotiated revision's JSON schema, and the legs separate — **37 passing / 4 failing** for the legacy leg, **41 / 0** for the stateless one. All four failures are that check on `resources-{list,read-text,read-binary,templates-read}`, for missing `cacheScope`/`ttlMs` — CACH-001, the same clause the registry had already flagged and the runner had missed. No pre-existing check changed status; the whole delta is the 36 new checks. The `draft-readiness` name still stands, and its rename is still deferred to the stable-pin change, which is the one that has not happened |
| Register upkeep | 90-day sweep | Re-verify [01-ecosystem-context.md](01-ecosystem-context.md) rows before external use |
| Claims expiry | Weekly scheduled CI (ADR-0010) | `cargo xtask deferrals --check` fails once a [deferral-ledger](deferrals.json) row passes its review-by date; `cargo xtask spec-drift` re-verifies every registry quote against the published spec text |
| Upstream presence | Continuous | Issue triage participation and small fixes in rust-sdk/conformance — the relationship M4 depends on is built before it is needed |

## Sequencing rules

1. M0 strictly precedes everything; no feature code on an unscaffolded repo.
2. M1 strictly precedes M2 (the agreement check needs the validator).
3. M2.5 opens when the `2026-07-28` final text ships and takes precedence over M3
   wherever the two contend for effort; M3 may proceed in parallel where they do not.
4. M2 and M3 may overlap after M2's server passes `core` scenarios.
5. M4 has no ordering constraint — earliest credible moment wins; M5 closes last.
6. Any risk trigger in [08-risk-register.md](08-risk-register.md) firing forces a roadmap
   review before the next milestone proceeds.
