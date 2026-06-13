<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0010: Claims Expire — the Deferral Ledger and Scheduled Re-Verification Gates

**Date:** 2026-06-12
**Status:** Accepted
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
