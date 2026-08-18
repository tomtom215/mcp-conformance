<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0010: Claims Expire — the Deferral Ledger and Scheduled Re-Verification Gates

**Date:** 2026-06-12
**Status:** Accepted (amended 2026-08-18 — see §Amendment)
**Author:** Tom F.

---

## Context

Three rounds of adversarial auditing produced the same lesson three ways: the
repository's falsehoods were never in fresh code — they were in *claims that
were true once* and nothing re-checked. The registry's spec quotes were
verified against the published text with a `/tmp` script that no longer
exists; deferred work ("auth scenarios later", "URL mode lands with the
host") was documented in prose that nothing re-read; the second audit's own
"verified" statements began rotting the day they merged (the npm `alpha`
dist-tag moved within 24 hours of a row asserting it had not).

Two standing mechanisms already work this way: the agreement baseline fails
on *stale* entries (an explanation for a divergence that no longer occurs),
and the register carries per-row verification dates with a 90-day rule. The
gap is everything else: deferrals, scheduled re-verifications, and the
registry's verbatim quotes.

## Decision

1. **The deferral ledger.** `docs/plan/deferrals.json` records every
   consciously deferred piece of work: what, why, what enforcement exists
   meanwhile, and a `review_by` date. `cargo xtask deferrals --check` fails
   once a row passes its date un-re-decided; the weekly scheduled job runs
   it, the PR gate does not (an expiry should page the schedule, not block
   unrelated work). Re-deciding means building the thing (delete the row) or
   re-dating it with a fresh reason in the same commit. Deferrals are never
   prose alone. Permanent decisions do not belong in the ledger — they get
   ADRs.
2. **The spec-drift gate.** `cargo xtask spec-drift` fetches every in-scope
   spec page and verifies each registry quote verbatim (under the
   documented whitespace/`"; "`-join normalization `SourceRef::quote`
   declares). It runs in the weekly scheduled job — network use puts it on
   the orchestration side of the same boundary as `conformance` — and any
   fetch failure fails the gate: an unverified page is not a verified page.
3. **The in-scope page set is explicit data.** The registry's completeness
   claim ("every MUST on an in-scope page enters") finally names its
   universe: `registry/2025-11-25/sources.json` lists the in-scope pages
   (mapping each to its published source file) and the deliberately
   out-of-scope pages with reasons. The spec-drift gate enforces, both
   directions, that the listed set and the set of pages registry entries
   actually cite are identical — the list cannot drift from the registry it
   describes.

## Consequences

### Positive

- Every "later" in the repository now has a date and a gate that fires when
  the date passes. The first ledger rows carry: the suite's `auth/*` client
  scenarios (TRAN-009's boundary), the rmcp SSE-resumption upstream filing
  (register 3.12), the rust-sdk#902 offer clock (risk R9's 60 days), the
  register's own 90-day sweep, and the suite 0.2.0 pin bump.
- The registry's quotes can no longer rot silently: the gate that verified
  them is committed, scheduled, and names the drifted entry and page.

### Negative

- The weekly job gains network fetches of nine spec pages. Bounded (one
  small file each, 30 s curl timeout, hard failure on error) and on the
  side of the network boundary that already dials npm.
- A spec-side prose reshuffle (same requirement, new wording) fails the
  gate until the quote is refreshed — deliberate: a quote refresh is
  exactly the review moment where a semantic change would otherwise slip by.

## Alternatives considered

### Re-verify dates in prose (the status quo)

Rejected by three rounds of evidence: prose dates are read by the next
audit, not by anything that runs.

### Failing the PR gate on expired deferrals

Rejected: an expiry is a scheduling event, not a defect in the PR being
merged. Blocking unrelated work on it teaches people to game dates.

### A full Markdown-AST quote matcher

Rejected for now: the documented normalization (whitespace collapse, bullet
markers, `"; "` joins, straight quotes) reproduces the extraction convention
`SourceRef::quote` already declares, and the gate fails closed on anything
it cannot match — a stricter matcher can replace the normalizer without
changing the gate's contract.

---

## Amendment (2026-08-18): the gate now files a tracking issue

Decision 1 said the weekly job runs `deferrals --check` and the PR gate does
not, so an expiry pages the schedule rather than blocking unrelated work.
That split is unchanged and is not what this amendment revisits. What it
revisits is the word *pages*.

The evidence: on 2026-08-18 two rows — `rust-sdk-902-offer-clock` (review-by
2026-08-10) and `suite-0-2-0-stable-pin-bump` (review-by 2026-08-15) — were
found expired **by hand**, eight and three days late. The gate had done its
job: it went red on the Monday runs and named both rows. Nothing downstream
of red existed except GitHub's default scheduled-failure email, which is
easy to miss and easy to filter, and which says only that a workflow failed.
A gate whose output nobody receives is prose with extra steps — the exact
failure mode this ADR was written against, reappearing one layer out.

Three changes close it, all inside the `claims-expire` job:

1. **A red run opens or updates a tracking issue**, whose body names the
   expired row ids and their dates. A green run **closes** it. The issue's
   open/closed state therefore tracks the gates' state, which is the property
   an email cannot have: a notification you missed is gone, an open issue is
   still open.
2. **The two gates report separately.** `spec-drift` now carries
   `if: always()`, so a red ledger no longer skips it — a week with an expired
   row used to measure the quotes not at all — and the issue body tabulates
   both outcomes. A transient spec fetch failure and an un-re-decided deferral
   were previously indistinguishable from outside the run log; now the issue
   says which.
3. **The row ids are read from the ledger, not scraped from a log.**
   `cargo xtask deferrals --expired` prints `id review_by` per expired row on
   stdout and exits zero; the workflow formats those lines. The gate's log
   format stays free to change without silently emptying the notification.

### Security surface

The deferral row that tracked this work predicted `issues: write` **plus a
new pinned action dependency**. The first is real and is granted on the
`claims-expire` job alone; the workflow default stays `contents: read`, so no
other job here gains anything. The second turned out to be avoidable: `gh`
ships on the runner and is already this repository's idiom for
`GITHUB_TOKEN` work (`release.yml`'s release step), so the change adds no
action to pin, review, or trust.

Two properties keep the write scope narrow in practice. This workflow
triggers only on `schedule` and `workflow_dispatch`, so no fork or
pull-request input ever reaches the token. And the issue body is composed
from the committed ledger and the two step outcomes only — `spec-drift`
reads remote specification text, and none of that text is republished into
an issue, because an auto-posted body is a poor place to render bytes
fetched from elsewhere.

### Cost

A GitHub issue can now be opened by CI without a human in the loop, which is
a small ongoing stream of automation-authored issues in a repository that
had none. Bounded by construction: one open issue at a time (a stable title
is the de-duplication key), a comment rather than a new issue while it stays
red, and an automatic close when it goes green.

