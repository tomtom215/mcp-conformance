<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Conformance Strategy

**Status:** Active
**Last reviewed:** 2026-06-11

---

## Position relative to the official suite

`@modelcontextprotocol/conformance` is the authority on what "conformant" means
([register 2.1–2.3](01-ecosystem-context.md)). This project does not redefine conformance,
fork the suite, or compete with it. We add the three things it does not provide:

| Official suite provides | This project adds |
|-------------------------|-------------------|
| Live scenario execution against a running SUT, from TypeScript | Offline validation of **recorded traces** — CI-embeddable in any language toolchain without Node |
| Scenario-level pass/fail | **Requirement-level** findings: every verdict tied to a spec clause ID, quote, and offending event |
| Server and client scenario sets, tier-check CLI | Rust reference implementations on both sides of the wire (everything server + reference host) |

Where the official suite and the spec text disagree, the discrepancy is filed upstream;
we implement the spec text in the meantime and record the divergence as a documented
exclusion in the registry. Silent private interpretations are how ecosystems fragment.

## One format, two uses: requirements and SEP traceability

SEP-2484 requires, for every Standards-Track SEP reaching Final: a conformance scenario
tagged with the SEP number, plus a `sep-NNNN.yaml` traceability file "mapping each MUST/MUST
NOT to a check or a documented exclusion" ([register 2.9](01-ecosystem-context.md)).

We adopt that exact shape as the storage format of our requirement registry
([02-architecture.md](02-architecture.md)). Consequences:

- Producing a traceability file for a SEP is a *query* over the registry, not new work.
- Upstream review of any scenario we contribute doubles as review of our registry entries.
- The registry can never drift into a private dialect — its format is pinned to an upstream
  process document.

### What enters the registry

Quote-driven, from the published spec text only (RFC 8174: only UPPERCASE keyword
instances are normative):

1. **Every MUST / MUST NOT** on an in-scope page enters — with checks when a recorded
   trace can judge it, with a documented exclusion naming where it *is* enforced when
   it cannot. No exceptions: that is the SEP-2484 floor.
2. **SHOULD / SHOULD NOT / MAY** enter when the clause constrains observable wire
   behavior (messages, headers, capability declarations). Guidance about UI, internal
   policy, or model interaction stays out of the registry.
3. Constraints stated only in schema field documentation without RFC 2119 keywords
   (e.g. completion's 100-value cap) are not registry entries; the agreement check
   (M2) covers any divergence that matters in practice.

### Check semantics

| Registry level | Finding on violation | Report effect |
|----------------|---------------------|---------------|
| MUST / MUST NOT | Error | Fails the run (exit 1) |
| SHOULD / SHOULD NOT | Warning | Reported, never fails the run by default; `--strict` promotes to error |
| MAY | Informational | Coverage signal only |

Two hard rules:

1. **Not-applicable ≠ passed.** A requirement gated on an undeclared capability reports as
   not-applicable and is excluded from pass-rate denominators. Vacuous passes inflate scores
   and destroy trust in the tool.
2. **Every check is falsifiable.** For each check, the corpus contains at least one trace
   that passes it and one injected-violation trace that fails it. A check that has never
   failed anything is untested code.

## Scenario taxonomy

Our scenarios and corpora mirror the official suite's structure
([register 2.3](01-ecosystem-context.md)) so results are directly comparable; the
committed corpus currently ships `good/`, `violations/`, and `golden/`
(`corpus/README.md`), with the suite-aligned directories below adopted as their
content lands:

| Official suite | Our corpus directory | Notes |
|----------------|---------------------|-------|
| `core` | `corpus/core/` | Lifecycle, tools, resources, prompts, logging, completion, pagination |
| `auth` | `corpus/auth/` | OAuth flows, metadata discovery — traces include the HTTP layer |
| `backcompat` | `corpus/backcompat/` | Older-revision behavior against `applies` ranges |
| `extensions` | `corpus/extensions/` | Reverse-DNS extension negotiation ([register 1.5](01-ecosystem-context.md)) |
| `draft` | `corpus/draft/` | `2026-07-28` RC behavior, behind the `draft-2026-07-28` feature |
| `metadata`, `sep-NNN` | per-SEP directories | Tagged for SEP-2484 traceability |

Each corpus entry is: the trace (`.jsonl`), the expected report, and a provenance note
(what produced the trace, against which implementation and revision). The expected report
is pinned across two files — the trace's golden holds every row the trace decided, and
`corpus/golden/exclusions/<revision>.json` holds the rows its registry decided, once per
revision rather than once per trace ([ADR-0013](decisions/0013-golden-report-format.md));
splicing them reproduces the whole report, and the golden suite asserts that it does.

## Calibration: the agreement check

Defined in [02-architecture.md](02-architecture.md) (`xtask`): every CI run executes the
pinned official runner and our validator against the same session and diffs verdicts.
Divergences are triaged into exactly three buckets — our bug (fix), suite bug (file
upstream), spec ambiguity (file upstream) — and the triage outcome is recorded in the
corpus provenance note. This is the project's standing answer to "why should anyone trust a
second opinion?": because it is continuously reconciled with the first one.

Mechanics as of 2026-06-10 (both sides live): `cargo xtask conformance` runs the pinned
runner against the everything server (its per-scenario verdicts land in
`target/conformance/*/checks.json`, gated by the committed
`conformance/expected-failures.yaml` — empty; 40/40 pass) **and** replays the same
sessions through our validator: the server's tap (feature `tap`, `--tap-dir`) records
every admitted session as a validator-ready JSON Lines trace, and the agreement step
fails on any MUST-level validator finding not explained in
`conformance/agreement-divergences.json` (every entry requires a triage class and an
upstream link; unknown fields are rejected). The baseline gates in both directions
(2026-06-11): an entry that explains nothing in the current run is *stale* — the
divergence it described no longer occurs — and fails the run until removed, so an
explanation leaves the baseline in the same change that resolves it (typically the
suite pin bump), and a lingering pattern can never silently absorb the next
same-requirement failure. The reconciliation is written to
`target/conformance/agreement.json` with full pass/fail/warn/excluded/not-applicable
accounting. The same tapped sessions generate the committed
`conformance/coverage-manifest.json` (server capabilities, registry capability gates,
methods observed); drift or an undeclared server-party gate fails the run. The check
earned its keep immediately: its first run surfaced one MUST divergence (triaged
suite-bug, [#7](https://github.com/tomtom215/mcp-conformance/issues/7); filed upstream
as [conformance#338](https://github.com/modelcontextprotocol/conformance/issues/338),
2026-06-11) and one
informational SHOULD warning on the suite's version-compat probe.

## Official-suite version policy

- **Pin** the stable line (`0.1.16` as of 2026-06-11 — [register 2.4](01-ecosystem-context.md))
  in `xtask` via an exact version (`SUITE_VERSION` in `xtask/src/conformance.rs`);
  upgrades are deliberate PRs with a diff of scenario changes.
- **Track** the `0.2.0-alpha` line in a scheduled, non-blocking CI job so breakage arrives as
  an early warning, not a release-day surprise. That job floats the `alpha` dist-tag *by
  design* — it exists to hear upstream churn coming — and stays at the registry's revision,
  so it is signal to read, never a gate.
- **Measure** the next revision separately and *gate* it: `DRAFT_SUITE_VERSION` in
  `xtask/src/draft_readiness.rs` pins an exact alpha (`0.2.0-alpha.11`) and runs the
  runner's `2026-07-28` scenarios against the server once per revision the server can
  serve, ratcheting every check's status against `conformance/draft-readiness.json`. Floating and pinned are not
  interchangeable here: a tracking job wants the newest thing upstream has, a ratchet
  wants an input that cannot move underneath it, so the two are different jobs on
  different pins rather than one job with a compromise.
- **The two pins have different triggers**, and conflating them was a mistake this
  document made until 2026-08-18. `SUITE_VERSION` gates the *released* revision, so it
  waits for a stable release to exist — `0.1.16` is still the only one. `DRAFT_SUITE_VERSION`
  measures readiness against a scenario set that is *itself* pre-release and moving, so
  holding it back does not keep the measurement stable, it makes it describe an
  increasingly old question. It therefore advances to the newest alpha carrying the
  `2026-07-28` scenarios, deliberately, with a `BLESS=1` re-measurement and a scenario
  diff review in the same commit. Holding at `alpha.9` for six weeks cost exactly what
  that argument predicts: `alpha.11` added a `wire-schema-valid` check that the ratchet
  had no way to see.
- **Neither pin may fall behind unnoticed.** `cargo xtask suite-currency` asks npm what
  the `latest` and `alpha` dist-tags currently point at and fails when either differs
  from its pin. It runs in the weekly `claims-expire` job, so a red run files a tracking
  issue (ADR-0010 §Amendment) rather than blocking an unrelated pull request. This
  automates the *noticing* only; every sentence above about how a bump is made still
  holds, and holding a pin deliberately remains a legitimate outcome — recorded where
  the pin lives, not by leaving the gate red. Before this existed the re-check was a
  dated line in the deferral ledger, and it had already missed two alpha releases.
- The `draft` suite runs only under the `draft-2026-07-28` feature. The readiness ratchet
  above is *not* that: it records the runner's verdicts and nothing else. The registry's
  verdict on the same traffic is taken separately — each leg's session is captured to
  `corpus/draft/captured/`, where the golden suite replays it through the `2026-07-28`
  registry on every `cargo test`. Keeping the two apart matters because they have
  disagreed, usefully. Through `0.2.0-alpha.9` the runner scored 23/23 against a
  `2025-11-25` server *and* a `2026-07-28` one — it could not tell them apart — while the
  registry separated them — 59 pass, 1 fail against the first and 60 pass, 0 fail
  against the second, with 64 clauses not observed on each — the single failure
  being CACH-001, no `ttlMs` on cacheable results.
  `alpha.11` closes that gap from the other side: its new `wire-schema-valid` check
  validates every message against the negotiated revision's JSON schema and fails the
  `2025-11-25` leg's four resource scenarios for missing `cacheScope`/`ttlMs` — the same
  clause, found independently, six weeks later. The registry's verdict is now
  corroborated rather than merely unrefuted, and the argument for taking both readings
  is stronger, not weaker: one of them saw it first. Folding the alpha's verdicts into
  the agreement gate would still make a released pin's outcome hostage to a pre-release
  check set.

## Supporting rmcp's path to Tier 1

The Rust SDK is officially Tier 2 ([register 2.8](01-ecosystem-context.md)). Tier 1 requires
([register 2.5](01-ecosystem-context.md)): 100% conformance pass, new protocol features
inside the two-week RC→release window, two-business-day triage, seven-day P0 resolution, and
documented stable releasing. Of these, the first two are where outside engineering effort
genuinely helps; the rest are maintainer-process commitments we can support but not supply.

Our concrete contributions to that path (tracked in
[07-ecosystem-engagement.md](07-ecosystem-engagement.md)):

1. **The everything server** — the SEP-1730 appendix artifact rust-sdk verifiably lacks
   ([register 3.4](01-ecosystem-context.md)) — built rmcp-idiomatic and offered upstream.
2. **Conformance scenarios and fixtures** for gaps the suite's Rust runs expose, contributed
   to `modelcontextprotocol/conformance`.
3. **A published tier-gap report**: the official suite's measurement plus requirement-level
   detail from our validator, refreshed per spec revision — turning "reach Tier 1" from a
   slogan into a checklist. First edition published 2026-06-13:
   [docs/reports/rmcp-tier-gap-2025-11-25.md](../reports/rmcp-tier-gap-2025-11-25.md)
   (rmcp 38/40 at head `266f870`; the two failures read back to clause level; close-the-gap
   checklist). It reports the `server`-subcommand figure rather than a `tier-check` aggregate,
   which is GitHub-token-gated and under-counts ([register 2.13](01-ecosystem-context.md),
   conformance#182).
4. **2026-07-28 readiness**: validator and corpus support for the stateless rework *before*
   the two-week window opens, so the Rust ecosystem has a test target early.

## What this strategy refuses to do

- No conformance claims about third-party implementations without published traces and
  registry versions — every public verdict must be reproducible from artifacts.
- No "blessed by" language. The official suite is the authority; we are infrastructure that
  serves it until and unless upstream chooses to adopt any part of this work.
- No score theater: pass rates are always reported with denominators, not-applicable counts,
  and registry version.
