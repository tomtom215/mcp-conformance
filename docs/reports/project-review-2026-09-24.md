<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Project review and recovery plan — 2026-09-24

**Reviewed:** tag `v0.5.1` / `main` at `e6a1136` (last commit 2026-08-29) ·
**Spec:** `modelcontextprotocol/modelcontextprotocol` at `ab3a39c` ·
**Official suite:** `modelcontextprotocol/conformance` at `7169291` (npm `latest` 0.1.16,
newest alpha 0.2.0-alpha.11) · **rmcp:** `rust-sdk` at `6677eee` (crates.io 3.4.1)

This is a dated report: it records what was true on the day and is exempt from the
living-document gates. Labels: **VALIDATED** (reproduced by a command during this review),
**VERIFIED** (read in a primary source — file, registry entry, upstream page), **INFERRED**
(reasoned, not checked).

## Verdict

The engineering core is real and the verdict machinery is carefully built. The product as
shipped does not do what its README says for the revision people now implement, has no
way for an outside user to obtain a trace, and has been operationally red for three
weeks. The ecosystem moved underneath two of its four founding premises. Most of the
effort since June went into self-verification of documents rather than into the
user-facing gaps, and the documentation now costs more to maintain than it returns.

Everything needed to fix this is buildable and mostly small. The plan below orders it so
that the tool stops producing wrong verdicts first, becomes usable by an outsider second,
and becomes spec-complete third.

## How this review was done

- Ran `cargo test --workspace --all-features --locked` (848 pass, 1 fail), `cargo clippy
  --workspace --all-targets --all-features -- -D warnings` (clean), `cargo xtask gates`
  (clean), and each claims gate (`deferrals`, `register-currency`, `toolchain`,
  `coverage`, `draft-coverage`, `docs-links`, `version-sync`, `spec-drift`,
  `suite-currency`).
- Built the validator with **default features** — what `cargo install
  mcp-trace-validator` gives a user — and ran it on every committed trace.
- Cloned the spec, the official suite, rmcp, `a2a-rust` and `mcpsnoop` and read them
  directly; counted normative keywords per spec page with a script and compared against
  both registries; re-verified every registry quote offline.
- Read GitHub Actions history, issues, PRs, releases and branches for this repository.
- Surveyed the adjacent tools (npm, crates.io, GitHub) and checked upstream issue state.

The scripts and raw outputs are not committed; the commands above reproduce every
VALIDATED figure below.

## Findings

Severity is about consequence for a user or for the project's credibility, not effort.

### Critical — the tool gives wrong verdicts on the current revision

**C1. A default install fails every conforming `2026-07-28` session, silently.**
VALIDATED. A default-feature build run on the five captures in `corpus/draft/captured/`
(three conforming sessions, plus a deliberately malformed probe and a legacy-mode server
session with one genuine CACH-001 finding) returns `verdict: fail`, exit 1, on all five — a
false `LIFE-001` ("first message is a `server/discover` request, expected `initialize`") on
each, plus
`BASE-003` on the two official-suite captures and `PAGE-002` on the probe. No note is
printed. README lines 74–77 promise the report "says so" when a trace is of another
revision; that path (`crates/mcp-trace-validator/src/declared.rs`, `is_known`) only
recognises revisions the build ships, so a default build has nothing to say. `--revision
2026-07-28` on the same build exits 2 with "registry set does not describe revision
2026-07-28" and no hint that a feature exists. With `--all-features` the same stdio capture
passes (81 pass, 0 fail). `2026-07-28` has been the published revision since 2026-07-28;
the only normative change on `draft/` since then is one wording edit (TRAN-081's
safe-integer sentence).

This violates the project's own first principle — a wrong verdict is worse than none —
on the exact input new users will bring.

**C2. At `--revision 2026-07-28` the CLI discards every finding's detail.** VALIDATED.
The multi-revision path prints `MRTR-019 (MUST) 2026-07-28=fail` with no `seq`, no reason
and no quote; the JSON form contains no `seq` field at all; JUnit is refused. The library
produces the details — only the CLI drops them. So even a user who finds the feature gets
"fail" with nothing to act on.

### High — usefulness, spec completeness, operations

**H1. No capture tool ships.** VERIFIED. The headline promises "record a trace of any MCP
session — in any language, over any transport". The validator has two subcommands
(`validate`, `requirements`). The only recorders are the everything-server's `--tap-dir`
(non-default `tap` feature, HTTP only, and only for traffic to *this* server) and the
reference host's capture (only sessions it drives, at the rmcp message seam rather than the
wire). There is no published trace JSON Schema. The charter lists "not a gateway/proxy" as
a non-goal (`docs/plan/00-charter.md`), which is the likely reason — but a recording tap is
not a gateway, and without one the product has no path from "my Python server" to "a
verdict". An undocumented working path exists: `mcp-reference-host --server-cmd <server>
--trace-dir out/` then `mcp-trace-validator validate out/*.jsonl` (29 pass, 0 fail
against the everything server) — for stdio servers only.

**H2. No JSON-schema validation of messages.** VERIFIED. The validator depends on serde,
serde_json and clap only; nothing reads `schema/<rev>/schema.json`. The official suite now
validates every wire message against the negotiated revision's schema
(`src/validation/wire-schema.ts`, check `wire-schema-valid`). Undetected here (INFERRED
from the checks' opportunistic field reads, e.g. `checks/tools.rs`): missing required
result fields, wrong JSON types, bad content-block discriminators, enum violations
(`role`, `LoggingLevel`, `cacheScope`), malformed capability objects and notification
params. Several registry exclusions (BASE-017/073, TRAN-081/082) exist only because the
architecture forbids a schema engine.

**H3. Three `2026-07-28` spec pages are neither in scope nor excluded.** VALIDATED.
`basic/patterns/cancellation`, `basic/patterns/progress` and `basic/patterns/index` appear
nowhere in `registry/2026-07-28/sources.json`. They carry wire-checkable MUSTs (cancellation
only for subscription teardown; progress-token type; monotonic progress). No gate can catch
this: the drift gate never checks that `in_scope ∪ out_of_scope` equals the page set.

**H4. The project is operationally red.** VERIFIED (GitHub Actions) and VALIDATED
(locally).
- The weekly Scheduled workflow's `claims-expire` job failed on 2026-09-07, 09-14 and
  09-21: two expired deferral rows (review-by 09-01 and 09-07), the toolchain pin (1.98.0
  vs stable 1.98.1), and four register rows past 90 days. Its auto-filed tracking issue
  (#50) has no human response.
- `cargo test` itself fails today: `xtask`'s
  `register_currency::tests::the_real_register_parses_and_is_current_today` asserts the
  register is under 90 days old against the wall clock, so **every PR's CI goes red as the
  calendar advances**, regardless of what the PR changes. It has failed in PR CI since
  about 2026-09-10.
- Dependabot #55 (sse-stream 0.3.0) fails to compile the reference host; #51 and #53 are
  green and unmerged for 16 days.
- No commits since 2026-08-29.

**H5. Two founding premises no longer hold.** VERIFIED.
- *"No Rust everything server exists" / "rmcp still has none."* rust-sdk carries an
  in-repo `conformance` package (server and client) and its `conformance.yml` runs the
  official suite on every push for both `2025-11-25` and `2026-07-28`; its ROADMAP reports
  100% (30/30 server) on both. The offer rust-sdk#902 has had no maintainer engagement in
  105 days and its body cites a stale 38/40 figure.
- *"Nothing takes a recording of an MCP session… and checks it."* `mcpsnoop` (Go, npm
  0.22.0 on 2026-09-11) records JSONL and runs `mcpsnoop check` offline with text, JUnit
  and SARIF output against hard-coded, revision-aware MUST rules
  (`internal/store/conformance.go`, 397 lines, covering `2026-07-28` MRTR and `_meta`).
  It has no requirement IDs, registry, exclusions, not-observed accounting or calibration.
  The survey in the ecosystem register was crates.io-only and missed it.
- The official suite now also ships per-SEP traceability (`src/seps/*.yaml`, 347 clause
  rows across 16 SEPs) and frozen per-revision requirement sets.

What remains unique (VERIFIED by absence across the surveyed tools, so PLAUSIBLE rather
than proven): a **registry-driven, clause-ID-level, offline judge of core spec text with
exclusions, not-observed/not-applicable semantics, and continuous calibration against the
official runner**. SEP-2484 itself names the protocol-debugger half as "valuable future
work". That is a real niche; it is narrower than the README claims.

### Medium

- **M1. Disputed exclusions.** Of 31 exclusions assessed across both revisions, 22 hold,
  3 hold only because of this project's own capture design, and 6 look wrong: TRAN-081/082
  (a tree walk, not a schema engine — and the identical sentence is *checked* under
  TOOL-034), TRAN-061/027 (the validator already pairs POSTs with statuses by capture
  order), TRAN-020 (partial), BASE-033/037 and DISC-003 (other SHOULDs are checked),
  TRAN-083/084 and TOOL-035 (partial). INFERRED per clause; each needs a corpus pair to
  settle.
- **M2. Nine MUST-family keywords on in-scope `2026-07-28` pages have no entry** — the
  `x-mcp-header` table, the caching table's "MUST NOT be shared across authorization
  contexts", and "These headers are REQUIRED". Cause: `tools/extract-clauses.py` drops
  table rows; the census counts MUST-family only and ignores REQUIRED/RECOMMENDED and all
  SHOULDs. On `2025-11-25`, 84/84 MUST-family keywords are covered and 12/70 SHOULD-family
  are not.
- **M3. Client-feature pages are excluded wholesale** though under MRTR they contain
  server-side, wire-checkable MUSTs (no undeclared elicitation modes; URL mode carries a
  valid `url`; no tool-enabled sampling without `sampling.tools`).
- **M4. No extension coverage** — tasks, skills (SEP-2640, Final 2026-09-11) and apps.
  The official suite has tasks and skills scenarios.
- **M5. Calibration is overstated.** The agreement check runs at `2025-11-25` only;
  `2026-07-28` is a weekly ratchet on a pre-release suite, deliberately kept out of the
  agreement gate.
- **M6. Performance on `2026-07-28` traces.** VALIDATED in code: `declared::is_known`
  re-parses the embedded registry set on every call, once per request carrying `_meta`.
  Measured once in this review on a 40k-message synthetic trace, in this review's cloud
  container, build profile not recorded: 0.19 s without `_meta`, 10.6 s with it (draft
  build), 21 s with `--revision 2026-07-28`. Indicative only; item 0.6 re-measures
  properly.
- **M7. Tap fidelity.** VALIDATED in code: an oversized request is forwarded as
  `Body::empty()` (`crates/mcp-everything-server/src/tap.rs`), so the tap alters the
  traffic it records; the host records `serde_json::to_value` of rmcp's typed message,
  not wire bytes. The validator rejects lines over 1 MiB (`reader.rs`) while the tap
  buffers up to 4 MiB. INFERRED: request `seq` is assigned after the response returns,
  and all sessionless traffic shares one file.
- **M8. Library ergonomics.** `engine::validate` panics on out-of-order `seq` (no
  `Result` variant); an empty trace returns `pass` from the library (the "judged nothing"
  refusal lives only in the binary); findings carry no spec quote or URL although the
  README says the report answers "verbatim from the spec"; every report is 260–310 lines
  with no failures-only mode.
- **M9. Distribution.** No prebuilt binaries (release assets are `.crate` files and
  `SHA256SUMS`), no container, no GitHub Action, no SARIF, no versioned JSON Schema for
  reports or traces. `cargo install mcp-reference-host` installs nothing (binary needs the
  non-default `cli` feature).
- **M10. Stale and contradictory living documents.** README calls `2026-07-28` "the
  next" revision; the book says `2025-11-25` is what implementations ship; the plan
  glossary calls it a release candidate. "Every claim is verified and dated" is false
  today. Four plan docs are past the 90-day review. The design note written for upstream
  quotes old registry counts (140/51/47/89 vs 142/55/51/87). `corpus/README.md` has stale
  arithmetic the book embeds. The security model claims capture redacts "token-shaped
  strings" — no such code exists. The roadmap's M2.5 status cell is 6,794 characters;
  its "time-boxed" milestone has been open 107 days.
- **M11. The a2a-rust bar is not met on the user-facing side.** Ahead of a2a-rust:
  traceability, determinism, calibration, release verification. Behind: no examples
  directory or quickstart, no SBOM, no per-PR semver checks, no coverage gate, fuzzing
  weekly rather than on PRs, no cross-SDK grading matrix, a 6-page book against 63.
  `04-engineering-standards.md` still calls OIDC publishing an upgrade over a2a-rust,
  which now uses OIDC too.

### Low

- 0.5.1 dated 2026-08-28 in CHANGELOG/CITATION; tag and crates.io say 2026-08-29.
- RELEASING.md still carries a v0.1/v0.2 status banner ("the next release is 0.3.0").
- Several docs still describe rmcp 1.7; the lockfile is on 3.1.4.
- `requirements | head` panics on a broken pipe.
- 153 `#[allow]` attributes (58 outside tests); `#![allow(deprecated)]` over whole
  modules in 8 files.
- Tap per-session state and file handles are never released; host capture does blocking
  `std::fs` writes inside async methods.
- 14 remote branches are fully merged and can be deleted.

### Proportionality

`xtask` is 8,134 non-test lines against 13,958 across all four product crates (58%) — more
than the validator itself (6,104). The Markdown corpus is ~115k words against ~49k lines of
Rust; the CHANGELOG is 172 KB for six releases (the 0.5.0 section alone is 18,424 words).
Gates that parse numeric claims out of prose in 25 documents, and a unit test that fails
on the calendar, have become the main source of red CI. The gates that protect *verdicts*
(falsifiability corpus, golden reports, spec-drift, agreement check, cross-arch
byte-identity) are worth their cost. The gates that protect *prose* are not, while the
user-facing gaps above are open. INFERRED judgment; the falsifier is a month in which the
doc gates catch a user-visible defect the verdict gates would have missed.

### Disclosure

The ecosystem register and the engagement doc profile named upstream maintainers by
commit count and include one third-party personal email address and one GitHub noreply
address. Nothing illegal, but it reads badly to the people whose goodwill M4 depends on.
The register and CHANGELOG also carry agent-session narration ("from a session with no
route to GitHub…"). No private filesystem paths were found in living docs.

### Understated strengths

These are real and the README buries or omits them: multi-revision judgment of one trace
(`--revision A --revision B`); the 272-entry `2026-07-28` registry with every quote
verified live (`spec-drift`: 414 quotes, 0 drifted today); the CACH-001 finding the
registry caught before the official runner did; byte-identical reports on big-endian and
32-bit targets; a zero-surviving-mutants sweep (green 2026-09-21); OIDC publishing with
byte-verified, attested crates; and the published `pmcp` tier-gap report.

## Plan

Each item states what "done" means as a check that can be run. Phases are ordered by
consequence; items within a phase are independent unless noted. Sizes: **S** ≤ a day,
**M** a few days, **L** a week or more — rough, INFERRED.

### Decisions only the owner can make

Answering these first changes what gets built. Recommendations are mine.

| # | Decision | Recommendation |
|---|----------|----------------|
| D1 | Amend the "not a gateway/proxy" non-goal to allow a **recording tap** (stdio wrapper + HTTP reverse proxy that records and forwards bytes unchanged) | Yes, by ADR. Without it the product has no input path for outside users (H1) |
| D2 | Allow a JSON Schema engine dependency in the validator | Yes, by ADR, as a separate check family that does not change existing clause verdicts (H2) |
| D3 | rust-sdk#902: read the thread, then withdraw the adoption offer and re-scope M4 and charter success criterion 2 | Withdraw. rmcp runs its own conformance server at 100% on both revisions; re-position ours as the validator's calibration subject and fixture generator |
| D4 | Authorization scope: enter the HTTP-observable auth clauses (401 + `WWW-Authenticate`, protected-resource metadata) or keep TRAN-009 excluded | Enter the observable subset after Phase 2's core work; keep full OAuth flows out |
| D5 | Retire or demote the prose-claim gates (`draft-coverage` prose parsing, `register-currency` as a blocking gate, `changelog-links`) and slim the roadmap/CHANGELOG | Demote to informational; keep every verdict-protecting gate blocking |
| D6 | Ship Phase 0 as 0.6.0 with a changed default (`2026-07-28` judged by default) | Yes; pre-1.0 minors may break and RELEASING.md already says so |

### Phase 0 — Restore green and stop wrong verdicts (target: 0.6.0)

| # | Work | Done when | Size |
|---|------|-----------|------|
| 0.1 | Make `cargo test` clock-independent: the real-register currency assertion moves out of unit tests (the weekly `register-currency --check` already covers it); unit tests keep fixed synthetic dates | `cargo test --workspace --all-features` passes with the system date set a year ahead (e.g. `faketime`) | S |
| 0.2 | Clear `claims-expire`: re-decide the two expired deferrals (D3 for #902), bump the toolchain pin to 1.98.1, re-verify register rows 1.5a/1.5b/1.5c/3.10 | Scheduled workflow green; issue #50 closed with the run link | S |
| 0.3 | Merge Dependabot #51 and #53; fix the reference host for sse-stream 0.3.0 or hold it at 0.2 with a reason | All three PRs resolved; CI green on `main` | S |
| 0.4 | Judge `2026-07-28` by default: ship the registry unconditionally (drop or invert the `draft-2026-07-28` feature), auto-select the revision a trace declares, and exit 2 with guidance when a trace declares a revision this build cannot judge | A default-feature build returns `pass` on the three conforming `corpus/draft/captured/` traces, both `corpus/draft/good/` traces and all four `corpus/good/` traces, and exactly the committed golden findings on the other two captures; a regression test runs the README's cross-revision claim **on the default build** (today `tests/book_examples.rs` runs it only with the feature on) | M |
| 0.5 | Multi-revision reports carry full findings (seq, message, quote); JUnit supported for multi-revision | Golden test on a `corpus/draft/violations/` trace shows `seq` and reason in human, JSON and JUnit | M |
| 0.6 | Cache the built-in `RegistrySet` (`OnceLock`) | Before/after timing on the 40k-message trace, release build, same machine, 5 runs each with spread reported; target within 2× of the no-`_meta` time | S |
| 0.7 | Correct the living docs: current revision, capture reality, calibration scope, the redaction claim, design-note counts, `corpus/README.md` arithmetic; add a 5-minute quickstart built on the working reference-host path | `cargo xtask gates` green; quickstart commands run verbatim in CI (the `readme_examples.rs` pattern) | S |

### Phase 1 — Usable by someone outside this repository (target: 0.7.0)

| # | Work | Done when | Size |
|---|------|-----------|------|
| 1.1 | Recording tap (after D1): `mcp-trace-capture stdio -- <cmd>` and `mcp-trace-capture http --upstream <url>`, forwarding bytes unchanged, assigning `seq` at observation time, one file per session, no body-size truncation (stream-through when over cap, recorded as a truncation event) | Round-trip tests prove forwarded bytes are identical; the official suite passes against the everything server *through* the tap; traces from the tap validate identically to the server's own tap | L |
| 1.2 | Publish the trace format as a versioned JSON Schema; validate every corpus trace against it in CI | Schema file committed; a corpus test fails on a schema-invalid trace | S |
| 1.3 | Reports: spec quote and URL on every finding; `--quiet` (failures and warnings only); SARIF output; versioned report JSON Schema | Golden tests for each format; SARIF validated against the 2.1.0 schema | M |
| 1.4 | Distribution: prebuilt binaries for Linux/macOS/Windows attached to releases with attestations; a container image; a GitHub Action wrapping capture + validate + JUnit/SARIF upload; make `cargo install mcp-reference-host` work by default | A fresh runner with no Rust toolchain validates a trace via the Action; `gh attestation verify` documented and run in the release job | M |
| 1.5 | Library: `try_validate` returning `Result` instead of panicking; the empty-trace refusal moves into the library; configurable line cap (default raised to match the tap) | Tests for each; semver-checks shows the additions as minor | S |
| 1.6 | Fix the everything-server tap (oversized bodies pass through; `seq` at request time; session-scoped files; handles released) and move host capture to the byte seam or document its limit | Tests pinning each; the 4 MiB body case validated end to end | M |
| 1.7 | Worked guides: capturing from the TypeScript and Python SDKs' example servers with the tap, and validating them | Both guides executed in a scheduled job; outputs committed as corpus with provenance rows | M |

### Phase 2 — Spec-complete for `2026-07-28` (target: 0.8.0)

| # | Work | Done when | Size |
|---|------|-----------|------|
| 2.1 | Gate the page set: `in_scope ∪ out_of_scope` must equal every spec page, and `out_of_scope` must carry a reason; enter cancellation, progress and patterns-index | Regression test fails when a page is removed from both lists; `spec-drift` census matches on the three pages | M |
| 2.2 | Census covers table rows, REQUIRED/RECOMMENDED and the SHOULD family; enter the nine uncovered MUST-family keywords and the 3 + 12 uncovered SHOULD-family ones | Census reports 0 uncovered keywords on in-scope pages for both revisions | M |
| 2.3 | Settle the six disputed exclusions: each becomes a check with a passing and a violating corpus trace, or keeps its exclusion with the reason corrected | Each clause's registry row cites its corpus pair or a reason that survives the evidence in M1 | M |
| 2.4 | Schema check family (after D2): vendor `schema/<rev>/schema.json` pinned by commit, drift-checked like quotes, validated per message with a JSON Schema 2020-12 engine; agreement with the suite's `wire-schema-valid` on the tapped sessions | Violation corpus for each undetected class listed in H2; agreement check reports 0 unexplained schema divergences | L |
| 2.5 | Server-side MUSTs on client-feature pages under MRTR (elicitation modes, URL mode, `sampling.tools`) | Each entered with a check and corpus pair, or an exclusion naming where it is enforced | M |
| 2.6 | Extensions as separately-scored registries (tasks, skills), mirroring the suite's `not_scored: extension` semantics | Extension rows reported but never affecting the core verdict; corpus pairs per check | L |
| 2.7 | Everything server exercises pagination, icons, `resource_link`, annotations, tasks | Captured-corpus evidence rises from 114 of 125 judgeable clauses; the remaining gap is listed by clause | M |
| 2.8 | Authorization, observable subset (after D4) | Entries for 401/`WWW-Authenticate`/resource-metadata clauses with corpus pairs | L |
| 2.9 | Draft tracking: TRAN-081's post-release wording change recorded; a `draft` registry skeleton so the next revision starts from data, not from a report | `spec-drift` run against `draft/` in the weekly job, informational | S |

### Phase 3 — Calibration and credibility

| # | Work | Done when | Size |
|---|------|-----------|------|
| 3.1 | Agreement check at `2026-07-28` using the suite's frozen `--requirements 2026-07-28` set on the pinned alpha, blocking once a stable 0.2.0 exists | Agreement report for both revisions in CI; divergence ledger per revision | M |
| 3.2 | Cross-SDK matrix (the a2a-rust TCK pattern): capture and validate the official TypeScript, Python, Go and Rust conformance servers on a schedule; publish the matrix in the book | Scheduled job green; matrix page generated from committed results | L |
| 3.3 | Second calibration source: diff verdicts against `mcpsnoop check` on the same traces; file disagreements in whichever project is wrong | A committed divergence table with each row triaged | M |
| 3.4 | Port from a2a-rust: per-crate CycloneDX SBOM with attestation, gate-falsification script, per-PR semver checks, coverage gate, fuzzing on PRs, a generated spec-compliance page from the registry | Each present in CI; the falsification script proves every gate can go red | M |

### Phase 4 — Upstream and positioning

| # | Work | Done when |
|---|------|-----------|
| 4.1 | Record rust-sdk#902's outcome (D3) in issue #9 and the engagement doc | Issue #9 closed with the outcome |
| 4.2 | Offer the unclaimed half upstream: an issue in the conformance repo proposing clause-level rows for core, non-SEP spec text (the suite's `src/seps/*.yaml` covers SEPs only) and offline trace judgment as the SEP-2484 "protocol debugger", with this registry as the worked example | Issue filed by the owner |
| 4.3 | The two small PRs still live at upstream HEAD today: the suite's mis-pathed `specReferences` URLs (`src/scenarios/server/tools.ts:912`, `:1045`, `:1180` and siblings) and rmcp's caret requirement on `rmcp-macros` (`Cargo.toml:8`, `version = "3.4.1"`) | PRs opened by the owner |
| 4.4 | Rewrite the README's problem statement to the niche that holds (H5), naming rmcp's conformance server, the suite's traceability and `mcpsnoop` as prior art | README reviewed against this report's H5 |

### Phase 5 — Make the process proportionate (after D5)

- Roadmap: one row per milestone, one line of status, links to evidence. The dated status
  logs move to git history, per the plan's own rule 2.
- CHANGELOG: user-facing changes only; audit narrative moves to commit messages.
- Remove maintainer profiling and third-party contact details from the register.
- Delete the 14 merged branches.
- Done when: no living document has a line over 400 characters, and the weekly job can
  only go red for a reason that affects a verdict, a dependency or a release.

## What to do first

1. Owner answers D1–D6.
2. 0.1 and 0.2 in one PR — they turn CI green and are prerequisites for everything else.
3. 0.4 with 0.5 and 0.6, released as 0.6.0 — this removes the wrong-verdict path.
4. 0.7 in the same release, so the README stops promising what the release does not do.
