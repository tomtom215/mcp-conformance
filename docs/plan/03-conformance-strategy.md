<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Conformance Strategy

**Status:** Active
**Last reviewed:** 2026-08-24

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
   it cannot. No exceptions: that is the SEP-2484 floor. MUST-family means every
   RFC 2119 spelling of it — MUST, MUST NOT, REQUIRED, SHALL, SHALL NOT — including
   keywords in table rows. Gated since 2026-08-20 by the `must_census` in each
   revision's `sources.json`: `cargo xtask spec-drift` recounts every page's
   MUST-family keyword instances against the published text, so a clause the registry
   never had can no longer arrive unnoticed. Before that gate existed the rule was
   hand-verified, and two clauses had been missed — BASE-082 and LIFE-018, both
   restating an obligation the registry already held under another page's id. Since
   2026-10-13 the gate also requires every instance to sit inside the quote of an
   entry citing its page: a count matched while `REQUIRED` went uncounted and
   unentered (TRAN-141).
2. **SHOULD / SHOULD NOT / RECOMMENDED / NOT RECOMMENDED** enter too, every instance:
   judged when the clause constrains observable wire behavior (messages, headers,
   capability declarations), and otherwise entered with an exclusion saying why —
   guidance about UI, internal policy, or model interaction is recorded as decided
   rather than left unread. Gated the same way since 2026-10-13 (`should_census`, and
   the same every-instance-entered rule). **MAY / OPTIONAL** grant latitude, so a
   trace has nothing in them to violate: they are counted (`may_census`, so an edit
   still asks for a re-read) but enter only where a clause pairs the permission with
   something checkable.
3. Constraints stated only in schema field documentation without RFC 2119 keywords
   (e.g. completion's 100-value cap) are not registry entries; the agreement check
   (M2) covers any divergence that matters in practice.
4. **An official extension's specification enters by the same rules, beside the
   revision it attaches to, and is never scored**
   ([ADR-0023](decisions/0023-extensions-are-reported-never-scored.md)). Extensions are
   published in their own repositories, not under the revision's pages, so they have
   their own page set and census (`registry/2026-07-28/extensions/sources.json`), held
   to the site's `extensions/` navigation; every keyword is entered and decided as rules
   1 and 2 say. Their entries name their extension, and a report judges them into an
   `extensions` block whose verdict the report's never reads — the official suite's
   `not_scored: extension`. Tasks and skills are entered; MCP Apps is deferred
   (`mcp-apps-extension`) and the authorization extensions are reasoned out.

### How the next revision starts

From data, not from a report. The specification publishes its unreleased next revision
under `docs/specification/draft/`, and the registry tracks it with a skeleton in
`crates/mcp-conformance-core/registry/draft/`:

- `sources.json` has a dated revision's shape — every page the draft publishes in exactly
  one of `in_scope` / `out_of_scope`, each exclusion with its reason, and the three
  censuses as last read — and `cargo xtask page-set` holds it to that shape offline.
- `post-release.json` records each released clause the draft words differently: the
  entry, its released quote (checked against the registry by `cargo test`), the draft's
  wording, and what the change means for the entry. The first row is TRAN-081's
  safe-integer sentence, which the draft now bounds "for integers represented using
  IEEE754 double-precision floating point numbers" rather than "for JavaScript" — the
  interval and the check unchanged.

The skeleton is data, never a judged revision: the core crate embeds registry documents
by an explicit per-revision list that names nothing in `draft/`, `draft` is not a protocol
revision `--revision` accepts, the directory is left out of the published package, and it
carries no requirement entries. `cargo xtask spec-drift --draft` compares it with the live
draft — the page set, each page's census, every MUST-family or SHOULD-family keyword no
released quote covers, and for every `2026-07-28` entry whether its sentence is still on
the draft page (present, changed with the draft wording shown, removed, or on a page the
draft dropped). It is informational: it exits 0 with a Markdown report whatever it finds,
failing only when the draft cannot be read, and runs weekly as the scheduled workflow's
`draft-drift` job, where the report is the job summary.

When the draft is cut as a dated revision, the skeleton becomes that revision's
directory: its `sources.json` the new `sources.json`, each `post-release.json` row applied
to the new revision's entry, each report finding entered or retired, the area files
added to the embedded list, and the revision added to `spec-drift`'s `REVISIONS` — at
which point the release gate, not this report, holds it to its text.

### Check semantics

| Registry level | Finding on violation | Report effect |
|----------------|---------------------|---------------|
| MUST / MUST NOT | Error | Fails the run (exit 1) |
| SHOULD / SHOULD NOT | Warning | Reported, never fails the run by default; `--strict` promotes to error |
| MAY | Informational | Coverage signal only |
| An extension's clause, any level | In the extension's own block, with its own verdict | Never fails the run, `--strict` or not: the extension is optional by definition ([ADR-0023](decisions/0023-extensions-are-reported-never-scored.md)) |

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
| `auth` | `corpus/{,draft/}{good,violations}/` (`http-authorization`, `auth-*`) | The HTTP-observable subset only (review item 2.8): a protected server's `401`/`403` `WWW-Authenticate` challenge. OAuth flows and metadata discovery run off the MCP endpoint and are excluded clause by clause (`AUTH-*`); the suite's `auth/*` client scenarios stay deferred (`auth-client-scenarios`) |
| `backcompat` | `corpus/backcompat/` | Older-revision behavior against `applies` ranges |
| `extensions` | `corpus/draft/{good,violations}/` (`tasks-*`, `task-*`, `skills-*`, `skill-*`) | The tasks and skills extensions' clauses, judged beside `2026-07-28` and never scored, as the suite runs its `tasks-*` scenarios under `not_scored: extension` ([ADR-0023](decisions/0023-extensions-are-reported-never-scored.md)); reverse-DNS extension negotiation itself is a core clause ([register 1.5](01-ecosystem-context.md)) |
| `draft` | `corpus/draft/` | `2026-07-28` traces: `good/`, `violations/`, and `captured/` (recordings of real sessions). The directory keeps its pre-release name; every build judges it |
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

**The wire schema is reconciled the same way (2026-10-13; review item 2.4,
[ADR-0020](decisions/0020-wire-schema-validation.md)).** From `0.2.0-alpha.11` the
runner appends a `wire-schema-valid` check to each scenario's `checks.json`, listing
every message the implementation under test sent that fails the negotiated revision's
JSON Schema — the message itself, not a session — and a `wire-schema-harness-error`
check of the same shape for messages the runner sent. The agreement step joins those
messages to the tapped sessions by sender and canonical JSON and compares them with
the validator's report `wire_schema` member (`xtask/src/agreement/schema.rs`): a
message one side flags and the other does not, or a flagged message no tapped session
carries, is a divergence. Each is explained in
`conformance/wire-schema-divergences.json` — leg, kind, a required `message_contains`
filter, a triage class, an upstream link and a reason — or fails the run, and an entry
that explains nothing in a compared run is stale and fails it too. The ledger admits a
fourth class, `schema-pin-skew`, because the runner bundles its own copy of each
revision's schema and this validator vendors `wire_schema::UPSTREAM_COMMIT` with its
own errata, so two correct readings can differ. The pinned `0.1.16` predates the
check: on the pinned run the runner's green verdict is its whole side, and any message
the validator flags is a divergence, exactly as a failing requirement row is. The
message-level comparison proper runs wherever the runner reports the check — the
scheduled alpha-tracking job (`MCP_SUITE_VERSION`), and both legs of
`cargo xtask draft-readiness`, whose sessions are judged at `2026-07-28` because that
is the revision the runner validates against whatever the server serves. The
reconciliation is the `wire_schema` member of `agreement.json` (for the draft legs,
`target/draft-readiness/<revision>/wire-schema-agreement.json`). Known sources of
divergence that no run has yet exercised: the runner asserts JSON Schema `format`
(through `ajv-formats`) and this validator does not; the runner applies none of this
validator's errata (`ElicitResult.content`, `JSONValue`), only its own `NumberSchema`
one, which our vendored copy already carries upstream; and its bundled `2026-07-28`
copy predates ours, naming `SubscriptionsListenResultMeta` where ours (equal to
upstream `main`) names `SubscriptionsListenResultMetaObject` and adds
`SubscriptionsListenResultResponse` — a rename that validates the same messages.

### A second calibration source: mcpsnoop

Since 2026-10-13 the corpus is also read by `mcpsnoop check`, the one other tool
that judges recorded sessions ([register 5.10](01-ecosystem-context.md)). Our
traces are converted into its capture format without inventing anything a
verdict reads, and the conversion is held against mcpsnoop's own recordings of
the same sessions on every measurement. Its rules are mapped to registry clauses
by reading both texts, and every divergence is triaged much as above — our
defect (fixed, with a regression trace), its defect (an upstream
issue drafted for the owner), or a reading the text does not settle (an open
item). `cargo xtask mcpsnoop` measures with the pinned binary — informational and
scheduled, never a gate; `cargo xtask mcpsnoop --check` gates the committed table
offline. The table and its triage: [book/src/mcpsnoop.md](../../book/src/mcpsnoop.md).

## Official-suite version policy

- **Pin** the stable line (`0.1.16` as of 2026-06-11 — [register 2.4](01-ecosystem-context.md))
  via an exact version (`VERSION` in `crates/mcp-conformance-cli/src/suite.rs`, which
  `xtask` and `mcp-conformance test` both run);
  upgrades are deliberate PRs with a diff of scenario changes.
- **Track** the `0.2.0-alpha` line in a scheduled, non-blocking CI job so breakage arrives as
  an early warning, not a release-day surprise. That job floats the `alpha` dist-tag *by
  design* — it exists to hear upstream churn coming — and stays at the registry's revision,
  so it is signal to read, never a gate.
- **Measure** the next revision separately and *gate* it: `DRAFT_SUITE_VERSION` in
  `xtask/src/draft_readiness.rs` pins an exact alpha (`0.2.0-alpha.12`) and runs the
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
- The `draft` corpus is judged in every build (ADR-0018 retired the feature that once
  gated it). The readiness ratchet
  above is *not* that: it records the runner's verdicts and nothing else. The registry's
  verdict on the same traffic is taken separately — each leg's session is captured to
  `corpus/draft/captured/`, where the golden suite replays it through the `2026-07-28`
  registry on every `cargo test`. Keeping the two apart matters because they have
  disagreed, usefully. Through `0.2.0-alpha.9` the runner scored 23/23 against a
  `2025-11-25` server *and* a `2026-07-28` one — it could not tell them apart — while the
  registry separated them — one MUST failing against the first and none against the
  second, the same clauses not observed on each (the per-capture counts live in
  `corpus/README.md`, checked against the goldens) — the single failure being
  CACH-001, no `ttlMs` on cacheable results.
  `alpha.11` closes that gap from the other side: its new `wire-schema-valid` check
  validates every message against the negotiated revision's JSON schema and fails the
  `2025-11-25` leg's four resource scenarios for missing `cacheScope`/`ttlMs` — the same
  clause, found independently, six weeks later. The registry's verdict is now
  corroborated rather than merely unrefuted, and the argument for taking both readings
  is stronger, not weaker: one of them saw it first. Folding the alpha's verdicts into
  the agreement gate would still make a released pin's outcome hostage to a pre-release
  check set.

## Supporting rmcp at Tier 1

**rmcp reached Tier 1.** The published table places Rust in Tier 1 alongside TypeScript,
Python, C# and Go, and rust-sdk's `ROADMAP.md` reports every SEP-1730 requirement met, with
100% server and client conformance on both dated suites
([register 2.8](01-ecosystem-context.md), refuted and rewritten in the 2026-08-24 sweep; this
section read "is officially Tier 2" until then). Tier 1 requires
([register 2.5](01-ecosystem-context.md)): 100% conformance pass, protocol features on the
timeline agreed per release, two-business-day triage, seven-day P0 resolution, and documented
stable releasing.

**That changes what help is worth offering, and the change is not cosmetic.** Tier 1 is not a
finish line but a *standing* condition — the sdk-tiers page defines demotion as "Tier 1 → Tier
2: **Any** conformance test fails" — so the useful contribution shifts from closing a gap to
keeping a 100% bar from slipping across two live revisions and a growing scenario set. Of the
four gaps this project was founded to help close, three closed without us
([ADR-0015](decisions/0015-the-tier-2-premise-is-gone.md)); the everything server is the one
that remains, and the roadmap published 2026-08-22 independently names SDK "conformance with
the specification" a priority area ([register 1.9](01-ecosystem-context.md)).

Our concrete contributions (tracked in
[07-ecosystem-engagement.md](07-ecosystem-engagement.md)):

1. **The everything server** — the SEP-1730 appendix artifact rust-sdk verifiably lacks
   ([register 3.4](01-ecosystem-context.md)) — built rmcp-idiomatic and offered upstream.
2. **Conformance scenarios and fixtures** for gaps the suite's Rust runs expose, contributed
   to `modelcontextprotocol/conformance`.
3. **A published tier-gap report**: the official suite's measurement plus requirement-level
   detail from our validator, refreshed per spec revision. Its frame is now *holding* Tier 1
   rather than reaching it, and each edition must say which tier and which revision it
   measured against — a report that says "gap" without naming the bar is the score theater
   this strategy refuses below. First edition published 2026-06-13:
   [docs/reports/rmcp-tier-gap-2025-11-25.md](../reports/rmcp-tier-gap-2025-11-25.md)
   (rmcp 38/40 at head `266f870`; the two failures read back to clause level; close-the-gap
   checklist). It reports the `server`-subcommand figure rather than a `tier-check` aggregate,
   which is GitHub-token-gated and under-counts ([register 2.13](01-ecosystem-context.md),
   conformance#182).
4. **`2026-07-28` support**: validator and corpus support for the stateless rework. Written
   as *readiness ahead of the window*; the revision shipped 2026-07-28 and is now current
   ([register 1.1](01-ecosystem-context.md)), so this is ongoing support for a live revision,
   measured two-legged by `cargo xtask draft-readiness`
   ([register 1.5i](01-ecosystem-context.md)).

## What this strategy refuses to do

- No conformance claims about third-party implementations without published traces and
  registry versions — every public verdict must be reproducible from artifacts.
- No "blessed by" language. The official suite is the authority; we are infrastructure that
  serves it until and unless upstream chooses to adopt any part of this work.
- No score theater: pass rates are always reported with denominators, not-applicable counts,
  and registry version.
