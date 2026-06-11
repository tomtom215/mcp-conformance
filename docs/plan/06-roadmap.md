<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Roadmap

**Status:** Active
**Last reviewed:** 2026-06-09

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
| M2 — Everything server | **In progress** (2026-06-10): server live on rmcp 1.7 over stdio + policy-gated streamable HTTP; **40/40 checks green** against the pinned suite in CI; trace tap, agreement check, and coverage manifest live this series (zero unexplained divergence; first divergence triaged as suite-bug — [#7](https://github.com/tomtom215/mcp-conformance/issues/7)) — remaining: upstream offer (maintainer action) |
| M2.5 — `2026-07-28` migration readiness | Not started — opens when the final text ships (July 28, 2026); re-sequenced ahead of M3 on 2026-06-09; extraction checklist re-scoped 2026-06-11 — the first RC-tracking reconciliation against the draft changelog ([register 1.5a–1.5b](01-ecosystem-context.md)) surfaced four majors the RC announcement never enumerated (`server/discover`, `subscriptions/listen`, tasks-as-extension, MRTR) plus the Roots/Sampling/Logging deprecations |
| M3 — Reference host | Not started |
| M4 — Upstream engagement | Not started (backlog open from day one) |
| M5 — Stewardship artifacts | Not started |

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
- [x] Validator replays the corpus deterministically: 100% pass on known-good traces;
      **every check is killed by at least one injected-violation trace**; byte-identical
      reports across platforms and runs.
- [x] Session state machine for `2025-11-25` with every transition and error edge unit- and
      property-tested.
- [x] Report formats: human, JSON, JUnit; exit codes 0/1/2/3 documented and tested.
- [x] Zero surviving mutants in `mcp-conformance-core` and `mcp-trace-validator`; fuzz
      targets (trace parse, canonicalization, registry deserialization) clean for the CI
      budget with corpora committed.
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
      and checked on every `cargo xtask conformance` run: all seven server-party
      registry capability gates declared and active — the manifest gate caught the
      missing `listChanged` declarations, closed by the `test-list-changed` tool — and
      18 distinct wire methods observed. `BLESS=1` regenerates; drift fails the gate.)*
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
- [ ] Upstream conversation opened: everything-server offered to
      `modelcontextprotocol/rust-sdk` (issue or draft PR), linked from the README whatever
      the outcome.

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

- [ ] `applies` revision ranges in the registry format — the slot ADR-0006 deferred —
      with the embedded loader able to serve more than one revision.
- [ ] `2026-07-28` registry entries extracted from the **final** spec text by the same
      per-requirement method (live fetch → verbatim quote → check or documented
      exclusion), behind the `draft-2026-07-28` feature until the official scenarios
      stabilize; the change inventory in [register 1.5a–1.5b](01-ecosystem-context.md)
      is the extraction checklist (SEP-2575 stateless lifecycle — handshake removal,
      `server/discover`, `subscriptions/listen`, and the
      `ping`/`logging/setLevel`/`notifications/roots/list_changed` removals; SEP-2567
      session removal; SEP-2322 MRTR replacing server-initiated requests; SEP-2663
      tasks extension; SEP-2243 routing headers; SEP-2106 JSON Schema 2020-12;
      SEP-2164 error-code change; SEP-2549 caching metadata; SEP-414 trace context;
      SEP-2468/SEP-837/SEP-2352 authorization deltas; SEP-2577 and SEP-2596
      deprecations plus the RFC 7591 DCR deprecation — reconciled 2026-06-11).
- [ ] Stateless state-machine variant alongside — not replacing — the `2025-11-25`
      machine, every transition and error edge unit- and property-tested.
- [ ] Multi-revision judgment: the same trace validated against both revisions in one
      invocation, applicability differences per clause visible in the report.
- [ ] `corpus/draft/` good and violation pairs green against the final text, with
      provenance-ledger rows.

## M3 — Reference host

**Definition of done**

- [ ] Host completes bounded tool-use loops against the everything server over stdio and
      streamable HTTP: every stop condition (turn limit, completion, cancellation, error
      budget) tested.
- [ ] Official suite **client scenarios** pass with the host as SUT (`--command` wiring,
      pinned version).
- [ ] Sampling, elicitation (including URL mode), and roots handlers scriptable for CI;
      zero model-provider network use.
- [ ] Backoff/retry honoring `Retry-After`, jittered, with budget tests; SSE resumption via
      event-id cursors where applicable.
- [ ] Host-side trace capture emits validator-ready traces with default redaction
      ([05-security-model.md](05-security-model.md)).

## M4 — Upstream engagement (gate, not phase)

Backlog opens at M0; the milestone closes only on merged outcomes.

**Definition of done**

- [ ] ≥ 1 substantive merged PR in `modelcontextprotocol/rust-sdk` or
      `modelcontextprotocol/conformance` (everything-server adoption, conformance scenario,
      MSRV policy, transport hardening — backlog in
      [07-ecosystem-engagement.md](07-ecosystem-engagement.md)).
- [ ] RustSec advisory for CVE-2026-42559 filed in coordination with rmcp maintainers, or
      upstream's documented decision not to ([register 4.3](01-ecosystem-context.md)).
- [ ] A public design note (in-repo) explaining the trace-validation architecture and
      trade-offs, linkable from upstream discussions.

## M5 — Stewardship artifacts

**Definition of done**

- [ ] Published tier-gap report for rmcp: official `tier-check` output + requirement-level
      findings + a concrete close-the-gap checklist; method reproducible from artifacts.
- [ ] Optionally the same report for one community SDK (e.g. `pmcp`) to prove generality.
- [ ] mdBook live (architecture, trace format, corpus guide, conformance results page);
      docs.rs complete for all crates.
- [ ] The `draft-2026-07-28` feature gate dropped (revision becomes default) — only after
      the final text has shipped, M2.5 is complete, and the official scenarios for the
      revision stabilize.

## Standing workstreams

| Workstream | Cadence | Content |
|------------|---------|---------|
| RC tracking | Each upstream RC change | Reconcile draft-revision expectations against the latest text; feeds M2.5, which re-scopes if the rework shifts materially ([08-risk-register.md](08-risk-register.md)) |
| Suite tracking | Scheduled CI | Pinned-stable upgrades as deliberate PRs; `0.2.0-alpha` watched non-blocking |
| Register upkeep | 90-day sweep | Re-verify [01-ecosystem-context.md](01-ecosystem-context.md) rows before external use |
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
