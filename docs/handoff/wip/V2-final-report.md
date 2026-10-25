<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# V2 final report — a baseline file and filters for `validate` (ADR-0025, Proposed)

The agent's own report, saved in substance when the 2026-10-25 session wrapped up. Its claims of what
was run are the agent's; the lead did **not** re-run them or review the diff. What the lead checked,
mechanically: branch `wip/V2` (tip `397b5c0`) sits on `cec63bb` with five commits and a clean tree;
every changed file is under `crates/mcp-trace-validator/` or is the ADR and its index row; nothing under
`corpus/` changed (so the goldens are byte-identical); no personal data in any diff or commit. **Not
merged into PR 68.** The commits are branch `wip/V2` in [`wip-v2.bundle`](wip-v2.bundle) (see
[`README.md`](README.md); fetch with
`git fetch docs/handoff/wip/wip-v2.bundle 'refs/heads/wip/V2:refs/remotes/wip/V2'` on a checkout that has
`cec63bb`).

**Original branch (lost with the container; `wip/V2` in the bundle is the record):**
`worktree-agent-a0a36e6337274795b`, five commits on `cec63bb`, 67 files, +7127 −794.

## Summary

Items 1 (the ADR), 2 (the baseline) and 3 (filters) are done and verified by the agent. **Item 4
(grouped repeats) is not shipped. Item 5 (docs) is not done**: the shipped flags are documented only by
`--help` and the ADR. Nothing is pushed or merged.

Commits (each ends with the Co-Authored-By and Claude-Session trailers):

- `29f2b4f` docs(plan): ADR-0025, a baseline for validate (Proposed)
- `193e13e` refactor(validator): one definition of a finding's identity
- `b474c2b` feat(validator): a baseline of accepted findings, and the library to apply it (ADR-0025)
- `e8a4599` feat(validator): validate --baseline, --write-baseline, --allow-stale-baseline, --area and --level (ADR-0025)
- `397b5c0` docs(plan): ADR-0025 says grouped repeats are not shipped

**Disclosure by the agent:** the `b474c2b` message says the files split out of `report.rs` and `sarif.rs`
were "unchanged"; that is true only of `Verdict` and the SARIF notifications. History was not rewritten.

## Setup facts the agent gave

- Its worktree started at `a64fa94`, 53 commits behind the stated tip; it ran `git merge --ff-only cec63bb`.
- It ran no `cargo xtask` task other than `gates`, at the very end, from a private copy of the binary
  (checked with `strings`). Nothing that writes (bless, mcpsnoop, coverage) was run.
- A shared-target hazard hit once: a CLI build linked a foreign validator library. It touched its crate's
  sources and rebuilt, checked with `strings` and behaviour, and touched all crate sources again before the
  final checks.
- It may have overwritten a shared scratchpad file named `mutate.py`; afterwards it used a `v2-` prefix.

## Item 1: the ADR (`docs/plan/decisions/0025-a-baseline-for-validate.md`, status Proposed)

1. **Format.** JSON `{"version":1,"fingerprint_key":"mcpConformanceFinding/v2","findings":[{fingerprint,
   reason, expires?}]}`, keyed by the SARIF `partialFingerprints["mcpConformanceFinding/v2"]`, whose
   definition moved into one module (`fingerprint.rs`). The parse is strict (unknown fields, duplicates,
   blank reasons and bad dates are rejected); the file is written sorted.
2. **Fields.** `reason` is required and non-blank. `expires` is an optional UTC date, inclusive. A lapsed
   entry (matched, `expires < today`) accepts nothing, so the finding counts again; a lapsed entry that
   matches nothing is stale.
3. **Semantics.** Accepted findings stay in every format, marked and counted separately; `totals` and row
   outcomes stay the trace's facts. The verdict and exit follow only unaccepted findings. No new `verdict`
   enum value (STABILITY.md says that is breaking); the label reads `verdict (server findings only,
   baseline applied)`, never "passed". A stale entry (matches no scored finding anywhere in the run) is
   listed in every format and the run exits 1 unless `--allow-stale-baseline`. Extension findings are
   unscored (ADR-0023) and are never accepted or recorded.
4. **`--write-baseline`.** Records the findings that count under the invocation's view. The reason is the
   non-stub text "recorded by --write-baseline from the findings present at the time; not reviewed",
   counted as unreviewed on every run. It refuses to overwrite a file it has not read (the target must be
   the `--baseline` in use; reasons and expiries are then kept and stale entries dropped), never writes to
   `-`, and writes nothing if any trace was unjudged.
5. **Interactions** with `--party` (applies to the recorded set), `--strict` (does not), `--revision`,
   several traces and multi-revision runs. Inherited limitation, stated in the ADR: `wire-schema@rev` and
   `ID@rev` fingerprints change when the number of failing revisions changes ("keep one file per
   selection").
6. **Filters** (`--area`, `--level`): display-only and human-only; they never change totals, verdict or
   exit, and the report says how many findings they leave out. A filter with json/junit/sarif/html is
   refused (exit 2); an unknown area is an error (exit 2).
7. **Grouped repeats:** rewritten in `397b5c0` to say not shipped (item 4).

The ADR lists the rejected alternatives (stub reason, expired as exit 2, filters that change the verdict,
grouping across seqs, YAML/TOML, others). It is about 190 lines, long for a proposal.

## Item 2: implementation (`e8a4599`, `b474c2b`, `193e13e`)

- **Library:** new modules `fingerprint`, `baseline` (`Baseline`, `Entry`, `Date`, `apply`, `record`,
  `Notes`, `TraceReports::new`, `GENERATED_REASON`, `JSON_SCHEMA`) and `selection`; `report`, `multi`,
  `junit`, `sarif`, `html`, `validate`, `requirements` and `main` extended. `Report` and `Finding` gained
  fields, so a library user who builds those literals needs the new fields.
- **CLI:** `--baseline`, `--write-baseline`, `--allow-stale-baseline` (requires `--baseline`).
- **Output:** human (an `[accepted]` tag, "accepted by baseline (N): reason", a `baseline:` tally, the
  verdict label); JSON (additive `baselined` and `baseline` members, present only with a baseline); JUnit
  (`<skipped message="accepted by baseline: ...">`, counted as skipped rather than failures, plus a
  `baseline` suite for stale and lapsed entries); SARIF (native `suppressions` with `kind: external`,
  `status: accepted` and a justification, level `note`; a stale notification as a warning); HTML (an
  "accepted" pill, the reason, a note; the stylesheet is unchanged, so the CSP digest is unchanged).
- **JSON Schema:** `schema/report.schema.json` changed additively; `schema/baseline.schema.json` is new.
- **Tests:** `tests/cli_baseline.rs` (14), `tests/cli_filters.rs` (9), unit tests in `baseline/tests/*`,
  `fingerprint/tests.rs`, `report/tests/{baseline,filter}.rs`, `junit/tests/baseline.rs`,
  `sarif/tests/baseline.rs`, `html/tests/baseline.rs`, and an ignored SARIF-schema test in `tests/sarif.rs`.
- **Mutations (VERIFIED earlier in its session; the harness and specs lived in the scratchpad, now gone):**
  12 library mutations and 22 renderer/CLI mutations, each applied from a `cp` backup, each turned a named
  new test red, each file restored byte-identical (SHA-256). The last run printed "ALL KILLED AND
  RESTORED". They ran on `e8a4599`; the only change since is the ADR file (`git diff --stat e8a4599 HEAD`).
  Library: lapse boundary, extension filter, counting rule, stale rule, matched flag, record filter, dedupe,
  blank reason, duplicate, NoDate, `effective_verdict`, serialization guard. Renderer/CLI examples: removing
  the `[accepted]` tag; flipping the stale exit-code override; disabling the overwrite guard; dropping the
  SARIF suppression; a filter with a machine format.
- **Reproduce then improve, on the real TypeScript SDK capture (earlier in its session; not re-run at the
  end):** before, 8 identical `BASE-086` lines and `--strict` exits 1; after, `--write-baseline` then
  `--baseline --strict` exits 0, a new tool exits 1, a stale entry exits 1 (`--allow-stale-baseline` gives
  0), a refresh keeps reasons, an expired entry counts again. `tests/cli_baseline.rs` encodes this flow. The
  SARIF `suppressions` shape was checked against the pinned OASIS schema (665 results).
- **Fingerprint refactor safety (earlier):** SARIF output byte-identical before and after over 732 corpus
  files.

## Item 3: filters (`e8a4599`)

`--area BASE,TOOL` and `--level MUST,SHOULD` share one `Selection` parser with `requirements`: commas and
any letter case are accepted. For `requirements` that is a behaviour widening (INFERRED from reading the old
code, not tested against the old binary). Display-only and human-only; the wire-schema result and extension
blocks are not filtered. Tests: `tests/cli_filters.rs` (9) and `report/tests/filter.rs`.

## Item 4: grouped repeats — NOT shipped

A draft module (quoted-value splitter, `Group` with a `details()` round trip, a linear-time scan, a
`HumanOptions::group` option) was integrated, but no tests (round trip, negatives, cap interplay),
mutations or corpus re-measurement existed before the stop request, so it was reverted; the tree holds none
of it and the default human listing is unchanged. **The unfinished draft existed only in the scratchpad and
is not preserved anywhere.** The ADR originally claimed "363 goldens, 606 findings, exactly one qualifying
run"; that was not re-measured with a shipped implementation, so it was removed in `397b5c0`. Whether
grouping is worth shipping at all is undecided: baseline acceptance already covers the adopter's CI need.

## Item 5: docs — NOT done

Not written: `book/src/{reading-a-report,getting-started,ci,upgrading}.md`,
`crates/mcp-trace-validator/README.md`, the root `README.md`, and any worked example. Because default output
is unchanged, the agent believes no existing sentence became false (INFERRED, not audited line by line). Not
checked: how `action.yml` / `action/validate.sh` pass extra arguments to `validate` (relevant to `ci.md`).

## Final checks the agent says it ran (sources identical to `e8a4599`)

All with `CARGO_TARGET_DIR=/home/user/mcp-conformance/target`:

- `cargo fmt --all -- --check`: exit 0
- `cargo clippy -p mcp-trace-validator --all-targets -- -D warnings`, default and `--all-features`: exit 0
- `cargo test -p mcp-trace-validator --all-features`: exit 0 (746 library tests plus the integration test
  binaries; every `test result` ok, 0 failed, 2 ignored)
- the two golden test commands (`--test golden --test golden_html`): exit 0 (12 and 4 passed); `cargo xtask
  bless` was never run
- `RUSTDOCFLAGS="-D warnings" cargo doc -p mcp-trace-validator --no-deps`, default and `--all-features`: exit 0
- after `git add -A` and the final commit, `gates` from a private xtask copy: exit 0

**Not run by the agent:** `cargo +1.88 clippy` (MSRV compatibility unverified), `--workspace --all-features`
(never), the ignored SARIF-schema test in the final pass (passed earlier), the mutation specs after the final
commit, the real-capture demo at the end.

## Surprises and things outside scope, as reported

- `docs/handoff/2026-10-24-session-state.md` listed grouped repeats and the filters as outstanding (now
  rewritten by the lead).
- `wire-schema@rev` and `ID@rev` fingerprints change when the number of failing revisions changes: inherent
  to the shared SARIF fingerprint, documented in the ADR.
- **Rewording a finding's detail text changes its fingerprint** and reopens baselined entries (the entry
  goes stale and the finding counts again): a cost of baselining on message text. Detail strings are now
  effectively part of the baseline contract.
- Shared `CARGO_TARGET_DIR` plus path-relative hashing means worktrees can link each other's libraries.

## Proposed CHANGELOG paragraphs (the agent's; `CHANGELOG.md` was not edited)

Added:

- `mcp-trace-validator validate --baseline FILE` accepts known findings. Each baseline entry (fingerprint, a
  required reason, an optional `expires` date) marks the matching finding as accepted. Accepted findings stay
  in every report, are marked and counted separately, and no longer fail the run; the verdict line says
  "baseline applied" and never reads as a pass. A stale entry (it matches nothing) fails the run unless
  `--allow-stale-baseline`; an entry past its `expires` date accepts nothing. `--write-baseline FILE` records
  the current findings with a reason saying they were not reviewed, and refuses to overwrite a file it has not
  read. See ADR-0025.
- JSON output gains `baselined` on findings and `baseline` on reports and the multi-trace envelope, present
  only when a baseline is used; `schema/report.schema.json` is extended additively and
  `schema/baseline.schema.json` describes the baseline file. JUnit reports accepted rows as skipped, with a
  `baseline` suite for stale entries. SARIF uses native `suppressions` (kind external, status accepted, with
  the reason as justification). HTML shows an "accepted" pill and the reason.
- `validate --area BASE,TOOL` and `--level MUST,SHOULD` list only the selected clauses in the terminal
  report. They never change totals, verdict or exit status, say how many findings they leave out, and are
  refused with JSON, JUnit, SARIF and HTML. `requirements` now shares their parser, so it also accepts
  comma-separated lists and any letter case.
- Library (`mcp-trace-validator`): new `baseline` and `fingerprint` modules; `render_run_with_baseline` in
  `junit`, `sarif` and `html`; `Report::render_with_filter` and `Filter`; new public fields on `Finding` and
  `Report`.

Changed: none to default output. Without the new flags every report, golden and exit status is unchanged.
Grouped repeats: no entry, because the feature was not shipped.
