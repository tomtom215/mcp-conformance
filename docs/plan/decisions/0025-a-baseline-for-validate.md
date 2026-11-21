<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0025: A Baseline for `validate` — Accept Known Findings, Gate on New Ones

**Date:** 2026-10-25
**Status:** Accepted (by the maintainer, 2026-11-20; implemented 2026-11-19 with its
documentation; `mcp-conformance check` and `mcp-conformance validate` take the same flags)
**Author:** Tom F. (maintainer); drafted by Claude Code

---

## Context

An author whose server already has findings cannot adopt `validate` as a CI gate. The
2026-10-24 dogfood audit ([§1.8, §3.4, §4.3](../../reports/dogfood-audit-2026-10-24.md))
met it on every TypeScript-SDK server: `BASE-086` ("RECOMMENDED to use JSON Schema
2020-12") fires once per tool schema, the SDK's generator emits draft-07, the author
cannot change that, and the only ways to a green run are to drop `--strict` or to stop
gating. Reproduced on `corpus/captured/sdk-typescript-2025-11-25.jsonl`: eight identical
`WARN BASE-086` lines at `seq 12`, one per tool, verdict `pass-with-warnings`, and
`probe-2026-07-28-http.jsonl` has 37 findings. What an author in that state needs is
"fail on a finding that is **new**", with the known ones still in the report.

`mcp-conformance test` already has the shape (`--divergences`: each entry carries a
triage class and a link, and "an entry that no longer occurs" fails the run, so an
explanation cannot lie in wait for the next failure that matches it). `validate` has
none, but it already has a stable per-finding identity: SARIF's
`partialFingerprints["mcpConformanceFinding/v2"]`.

**What that fingerprint is** (`sarif.rs`, read and then checked by running the tool on
2026-10-25): `{rule}:{check}:{detail with every digit run folded to #}:{n}`, where `rule`
is the requirement ID (or `wire-schema`), `n` counts findings of the same stem in the
trace, in registry then finding order, and the trace's path is not part of it. Checked
on the SDK trace above:

- adding 100 to every `seq` and renaming the file leaves all eight fingerprints equal;
- renaming one tool changes that tool's fingerprint and no other (7 of 8 persist);
- tools named `t_1` and `t_2` share a stem and are told apart by `n`; swapping which is
  which leaves the set of fingerprints unchanged;
- `wire-schema` becomes `wire-schema@2025-11-25` when the schema fails under more than
  one judged revision, so a wire-schema finding's fingerprint depends on the revision
  selection (the two shipped revisions share no requirement ID — 348 and 501 IDs,
  intersection empty, checked with `requirements --format json` — so a requirement's
  fingerprint does not).

It changes, therefore, with: the non-digit text of a detail (a renamed tool, a different
schema URL), a check's name, the requirement ID, and — in this tool — any release that
rewords a detail or renames a check. The changelog already announces those as "an open
alert closes and reopens once" ([STABILITY.md](../../../STABILITY.md), SARIF row).

## Decision

1. **One definition of a finding's identity.** The baseline is keyed by the SARIF
   fingerprint, computed by one function (`fingerprint.rs`) that SARIF and the baseline
   both call, so they cannot drift. The file names the scheme it holds
   (`"fingerprint_key": "mcpConformanceFinding/v2"`); a build that computes another
   refuses the file (exit 2) rather than report every entry stale.

2. **The file** is JSON, versioned, closed (unknown members, duplicate fingerprints and
   blank reasons are errors), with a schema in the crate (`schema/baseline.schema.json`,
   `baseline::JSON_SCHEMA`), like the report's:

   ```json
   { "version": 1,
     "fingerprint_key": "mcpConformanceFinding/v2",
     "findings": [ { "fingerprint": "BASE-086:base.schemas-use-2020-12:…:1",
                     "reason": "the TypeScript SDK's generator emits draft-07",
                     "expires": "2027-03-31" } ] }
   ```

   `reason` is required and must contain a non-blank character. `expires` is optional,
   `YYYY-MM-DD`, UTC, **inclusive**. An entry whose date has passed is *lapsed*: it
   accepts nothing, its finding counts again, and the report names it. A lapse is not a
   second kind of failure; it is the same finding the run would have reported without the
   entry. This is the only place the validator reads the clock, at the command-line edge
   and only when an entry carries `expires`; the library takes the date as an argument.

3. **Semantics** (`--baseline FILE`, one file per run):
   - A baselined finding stays in every format, tagged, and is counted separately
     (`baseline` in JSON: findings accepted, findings not). Row outcomes and `totals` are
     the trace's facts and do not change, as under `--party`.
   - The verdict and the exit status are computed from the findings **not** accepted
     (and, under `--party`, held against the viewed party). The verdict line and the JSON
     `baseline` member say a baseline was applied and how many findings it accepted, and
     no format describes an accepted finding as passing: a row keeps its `FAIL` or `WARN`
     marker, and JUnit gives a row all of whose findings are accepted `<skipped>`, not a
     passing case.
   - A new finding is never hidden: matching is exact on the fingerprint, so a second
     tool with the same defect is a new finding (a different name, or `n+1`).
   - An entry that matches no finding in the run is **stale**. It is listed in every
     format and the run exits `1`, as the repository's own `conformance/*.json`
     baselines do; `--allow-stale-baseline` relaxes that to a listed note. Staleness is
     judged over the whole run (several traces), and ignores `expires`: an entry that
     matches nothing is stale even if it has lapsed.

4. **Creating one**: `--write-baseline FILE` records the findings that count toward the
   verdict under this invocation's view: the fail and warn rows and a failed wire
   schema, never an extension's (ADR-0023), whatever `--strict` says (strictness changes
   an exit status, not what a finding is). Its reason is the fixed sentence
   `recorded by --write-baseline from the findings present at the time; not reviewed`.
   That is not a stub: it says what is true (a machine recorded this, nobody judged it),
   it promises nothing the entry cannot back, and every later run reports how many
   accepted findings still carry it, so the unreviewed state stays visible instead of
   passing for a decision. The command refuses to overwrite a file it has not read:
   an existing target must be the `--baseline` in use, and then each entry that still
   matches keeps its reason and its `expires` (a refresh never renews an expiry), new
   findings get the sentence, stale entries are dropped and counted on stderr. It writes
   nothing, and says so, if any named trace could not be judged. The run that writes a
   baseline prints the report it would print with that file applied, so the next run's
   output is the same.

5. **Interactions.** `--party`: orthogonal (a finding counts when it is held against the
   viewed party *and* not accepted). `--strict`: unchanged (it promotes a counted
   warning to an exit status). Several traces: one file for the run; an entry matches in
   every trace where its fingerprint occurs, because the fingerprint carries no path.
   `--revision`/several revisions: the fingerprint carries the requirement ID, so a
   baseline fits the revision selection it was written for; run it under another and the
   entries for the other revision's IDs are stale, and `wire-schema` entries do not
   carry across a change in how many revisions fail the schema (the inherited limit
   above). Keep one file per selection.

6. **Filters are display-only.** `--area BASE,TOOL` and `--level MUST,SHOULD` choose
   which clause rows and findings the **human** report lists; they never change a
   verdict, a total or an exit status, and the report says how many findings they hide.
   They are parsed by the code `requirements` uses, so the two commands agree. JSON,
   JUnit, SARIF and HTML always carry everything (as `--all` already says), so a filter
   with one of them is refused (exit `2`) instead of silently ignored. A verdict computed
   on a subset would need a different name, and nothing in the audit asks for one: the
   level case is `--strict`'s default, and what readers asked for is a shorter report.

7. **Grouped repeats are not shipped by this change.** The audit asked for repeated
   findings (`BASE-086` once per tool schema) to be listed once in the human report,
   but only if nothing is lost. The rule considered is narrow: consecutive findings of
   one clause share a group only when they are held against the same party, point at
   the same `seq`, are accepted (or not) by the same entry, and their text differs in
   exactly one quoted value (a double-quoted string or a backtick span), at least three
   of them; the group would print the party, the `seq`, the text with `<…>` where the
   value was, and every value, so each finding stays recoverable; `--all` would list
   every finding on its own line, as before; the cap of 20 would count findings. That
   rule has not been implemented, tested or measured against the corpus here, so this
   ADR makes no claim about how many reports it would change, and the default human
   listing is unchanged. Folding findings at different `seq`s is rejected below. A
   baseline already answers the adopter's need (accepted findings are marked and
   counted separately); the grouping only shortens a listing, and a lossy one would
   cost more than the lines it saves.

## Consequences

### Positive

- An author can gate CI on "no new findings" the day they adopt the tool, without losing
  sight of what is accepted, and an accepted finding that is fixed makes the next run say
  so (stale) rather than rot.
- One definition of identity: a fix to the fingerprint is a fix to both.
- Existing output, JSON, JUnit, SARIF and HTML are unchanged without `--baseline`
  (members present only when a baseline is in use); the committed goldens are untouched.

### Negative

- **Entries are long** (they contain the finding's text) and **break with wording**: a
  release that rewords a detail or renames a check makes its entries stale and its
  findings new, once, announced in the changelog's table like a SARIF alert.
  `--write-baseline --baseline` refreshes, but a reworded entry loses its reason.
- **A baseline fits one revision selection** (above); keep one file per selection.
- **`--write-baseline` accepts what is there.** The sentence is honest, but the
  command cannot judge whether a finding is acceptable; a reviewer reads the diff.
- **Wall-clock enters one code path** (`expires`), so a run can change with the date.
- Another file in the repository for the author to keep, and a stale entry is a red run.

## Alternatives Considered

- **Ignore by requirement ID** (`--ignore BASE-086`, the audit's phrasing). Rejected: it
  hides every future finding of that clause, including a new tool with the same defect and
  a different defect of the same clause; the point is "no new findings".
- **Key by clause and `seq` or trace line.** Rejected: a re-recording moves every `seq`
  (the reason the SARIF fingerprint folds digits).
- **Key by trace path plus fingerprint.** Rejected: paths are the instability SARIF already
  declined, and a baseline shared by many sessions (28 in a `test` run) would need
  an entry per session.
- **Hide accepted findings, as most linters' baselines do.** Rejected: a report that
  omits a defect the trace contains is no longer a report of the trace, and `--party`'s
  precedent is to keep every finding and say which ones the verdict set aside.
- **A new verdict value** (`pass-with-baseline`). Rejected: a new value of `verdict` is a
  breaking change ([STABILITY.md](../../../STABILITY.md)) for consumers that switch on it,
  and the `baseline` member carries the same information.
- **A `TODO` reason from `--write-baseline`, or an optional reason.** Rejected: a stub
  reads as a decision; an absent reason removes the only record of why.
- **An expired entry as an error (exit 2).** Rejected: the finding it accepted is an
  ordinary finding again, and exit `1` for it is what the same run would have given
  without the entry.
- **Filters that change the verdict.** Rejected above; if a use for a subset verdict
  appears it is its own decision, worded like `--party`'s.
- **Group findings by text template across different `seq`s.** Rejected: it would
  shorten a run of one defect spread over several events, but the reader then has to
  pair each value with its event in a notation the report does not otherwise use; the
  cap already bounds such a run.
- **YAML or TOML.** Rejected: `serde_json` is already a dependency, and this crate
  validates JSON against schemas already.
