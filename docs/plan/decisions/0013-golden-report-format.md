<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0013: The Golden Report Format — Trace Facts and Registry Facts Are Pinned Separately

**Date:** 2026-08-18
**Status:** Accepted
**Author:** Tom F.

---

## Context

Every trace in `corpus/` has a byte-pinned report under `corpus/golden/`. That
pinning is the corpus's whole value: a check that starts misfiring moves a
golden, and the diff *is* the alarm. Nothing here disputes that.

What the format got wrong is *what* it pinned. A report row is one of two very
different kinds of fact, and the single-file format stored both the same way:

- **Trace facts.** `pass`, `fail`, `warn`, `not-observed`, `not-applicable` —
  what this session did, or failed to do, or never came near.
- **A registry fact.** `excluded`. `engine::build_row` maps
  `Verification::Excluded { exclusion }` straight to the outcome and the
  registry's prose, consulting nothing about the session. `engine`'s own
  `happy_path_passes_every_checked_requirement` has always asserted it in so
  many words: *"every documented exclusion reports as excluded, regardless of
  trace."*

Because every golden pinned the full registry, the second kind was written out
once per trace. Measured on `f4ccac7`:

| | Reports | Rows each | `excluded` each | Lines |
|---|---:|---:|---:|---:|
| `corpus/golden` (`2025-11-25`) | 53 | 140 | 88 (63%) | 43,607 |
| `corpus/golden/draft` (`2026-07-28`) | 79 | 272 | 148 (54%) | 121,538 |
| **Total** | **132** | | | **165,145** |

98,136 of those 165,145 lines — **59%** — were `excluded` rows, and every one of
them was a copy. Across all 53 shipped goldens there was exactly **one** distinct
excluded set; across all 79 draft goldens, exactly one. The same 88 and 148
paragraphs of exclusion prose, 132 times over.

This is the duplication [ADR-0001](0001-plan-documentation-model.md) exists to
forbid — one fact, one place — and it had the failure mode that principle
predicts. Editing one exclusion reason meant touching 53 or 79 files; the
resulting diff was indistinguishable, at a glance, from a diff in which a check
had changed its verdict. The signal the goldens exist to carry was buried in
its own boilerplate, and the cost grew as `O(traces × registry entries)` — a
product of two numbers this project intends to keep increasing.

## Decision

A trace's report is pinned across two artifacts, split along the seam that was
already there.

### `corpus/golden/<stem>.json` — what the trace decided

The full report **minus its `excluded` rows**: the revision, the complete
`totals`, and every `pass` / `fail` / `warn` / `not-observed` /
`not-applicable` / `unsupported` row, in registry order, byte-identical to
before.

`totals` is deliberately left whole. `totals.excluded` staying in all 132 files
is what ties each trace back to its revision's ledger, and it is the cheap
per-trace assertion that the excluded set is still the size the ledger says: a
clause entering or leaving the set moves every golden, one line each, loudly.

### `corpus/golden/exclusions/<revision>.json` — what the registry declines to judge

One ledger per revision, generated from the registry, holding each excluded
clause's id, level and documented reason in registry order. `2025-11-25.json`
carries 88 rows; `2026-07-28.json` carries 148.

### Only `excluded` collapses

`not-observed` was measured before being ruled out, not assumed: the 53 shipped
goldens carry **28** distinct not-observed sets (15–28 rows each) and the 79
draft goldens carry **67** (35–105 rows each). It is per-trace evidence —
precisely the evidence [ADR-0012](0012-not-observed-outcome.md) added — and
collapsing it would delete the thing that makes the captures differentiate each
other. `not-applicable` is per-trace for the same reason: it is decided by the
capabilities *this session* negotiated ([ADR-0006](0006-capability-gated-applicability.md)).

`unsupported` is a property of (registry, build) rather than of the trace, so it
is structurally in the same class as `excluded`. It is **not** collapsed, and
the asymmetry is deliberate: it means the build is missing a check the registry
names, which is a defect, and a defect should scream from every report rather
than be tidied into a shared file. It is also empty in every committed golden,
so collapsing it would be generality bought speculatively.

### The split has to be provably lossless

Three assertions in `tests/golden.rs`, each stating a property the others do not:

1. `check_golden` byte-pins the judged half, as before.
2. `exclusion_ledger_matches_the_registry` (and its `draft::` twin) byte-pins
   each ledger against its registry. This is the single place exclusion prose is
   asserted.
3. `assert_reconstructs_the_full_report`, run on every trace, splices golden and
   ledger back together in registry order and asserts the result **is** the live
   report — row for row, plus revision and totals. It reads only committed
   artifacts, so it fails if a judged row goes missing, if a ledger drifts, or if
   the two interleave in any order but the registry's.

`every_golden_belongs_to_a_living_trace` is extended to the ledgers in both
directions: a revision with a golden directory must have one, and a ledger
without a golden directory is stranded prose that still reads as load-bearing.

## Consequences

### Positive

- **The alarm gets louder, not quieter.** A clause that stops being excluded now
  moves three things: the ledger loses a row, `totals.excluded` decrements in
  every golden, and every golden gains that clause's actual outcome. Previously
  it moved 132 near-identical walls of text in which a reviewer had to *find*
  the change.
- **One fact, one place.** Editing an exclusion reason touches one file. It is
  no longer possible to edit 87 of 88 copies.
- **Growth is bounded by evidence.** `O(traces × registry)` becomes
  `O(traces × judged rows) + O(registry)`. Each new shipped trace costs 52 rows
  instead of 140; each new draft trace 124 instead of 272. Entering a new
  revision's registry no longer multiplies its exclusion prose by the corpus.
- **The corpus fits in a review.** 165,145 lines become 68,199 (132 goldens plus
  1,190 lines of ledger); 6.55 MB become 1.32 MB, a 79.8% reduction.
- Losslessness was verified, not asserted: reconstructing all 132 pre-change
  files from the new pair reproduces each one **byte for byte**, so the only
  change to any golden is the removal of rows no trace decided.

### Negative

- A reader of one golden no longer sees the whole registry in it. The count is
  still there in `totals`, and the reasons are one file away, but "open this file
  and read the complete verdict" becomes "open two". This is the real cost of the
  decision and it is accepted: the excluded rows carried the same 88 or 148
  paragraphs every time, so what a reader loses from any single file is text they
  had already read 131 times.
- A third artifact kind joins `bless`, with its own invariants to keep. That is
  paid for by assertion 3, which makes the pair provably equivalent to the whole.
- The reconstruction test does real work on every trace, roughly doubling the
  golden suite's parsing. Measured at well under a second for all 132; if that
  ever stops being true it is a cost worth re-examining, not a reason to have
  skipped the proof now.
- Consumers that read goldens must not count `excluded` rows.
  `xtask draft-coverage` did, and now reads `totals.excluded` — the report's own
  aggregate, which is the more honest source either way. It is the only such
  consumer in the tree; a checked-in reader outside it would have to change.

### Neutral, and worth recording

Fixing this exposed an unrelated defect in the tool that maintains the goldens.
`cargo xtask bless` ran `cargo test -p mcp-trace-validator --test golden` with
default features, but all three `draft::` golden tests are gated on
`draft-2026-07-28`, which is not a default feature. Blessing therefore ran six
tests, regenerated the 53 shipped goldens, left all 79 draft goldens stale, and
exited 0 — a vacuous pass of exactly the shape ADR-0012 was written about, in the
command whose entire job is to regenerate goldens. CI's own all-features test leg
did catch the resulting staleness, so nothing wrong shipped; what it caught was a
failure `cargo xtask bless` could not fix. The task now passes `--all-features`
and runs 11 tests.

## Alternatives Considered

### Keep the format and compress it

Serialize rows one-per-line, or drop `level` (also derivable from the registry).
Rejected: it attacks the symptom. The duplication would still be
`O(traces × registry)`, and a denser format is a *less* reviewable diff, which
gives up the property the goldens exist for.

### Drop `excluded` from `Report` itself

The cheapest change — no test machinery, no second artifact. Rejected outright.
`report.rs` says why in its own doc comment: "`excluded` and `unsupported` are
first-class: inflating pass rates by hiding them is how conformance tools lose
trust." The published report is the product; the golden is a test fixture. This
ADR changes what the fixture pins and does not touch a byte of what the tool
emits.

### Store goldens as a diff against a baseline report

Maximum compression: pin only where a trace departs from some canonical report.
Rejected. It makes every golden unreadable on its own and couples all 132 files
to a baseline whose own changes would silently re-interpret every one of them.
The split chosen here has a seam that already existed in the data; a diff format
would invent one.

### Assert exclusions in a unit test and commit no ledger at all

The excluded set is already implied by the registry, so a test could check it
without a committed artifact. Rejected: the point of a golden is that the diff is
reviewable. An exclusion reason changing from "enforced by the transport layer"
to something else is exactly the kind of edit that deserves to appear in a pull
request, and a test that recomputes both sides shows nothing.
