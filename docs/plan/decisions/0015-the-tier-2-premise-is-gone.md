<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0015: The Tier-2 Premise Is Gone — What the Scope Actually Rests On

**Date:** 2026-08-24
**Status:** Accepted (re-examines [ADR-0002](0002-product-scope.md)'s context; the scope
decision itself stands, narrowed in its justification)
**Author:** Tom F.

---

## Context

[ADR-0002](0002-product-scope.md) chose this project's scope against five "decisive facts"
from the [register](../01-ecosystem-context.md). The second read:

> The Rust SDK is officially Tier 2 with verified, specific gaps: no everything server
> (SEP-1730's appendix artifact), no MSRV, no RustSec advisory for its CVSS 8.8 CVE
> (register 2.8, 3.4, 3.5, 4.3).

The [charter](../00-charter.md)'s third premise says the same thing at more length, and
[03-conformance-strategy](../03-conformance-strategy.md) repeats it a third time.

The 2026-08-24 90-day sweep re-verified all four of those register rows against primary
sources. **Three of the four clauses are refuted, and the fourth is unchanged:**

| Clause | Register row | State on 2026-08-24 |
|---|---|---|
| "officially Tier 2" | 2.8 | **Refuted.** The published table at `/docs/sdk` places Rust in **Tier 1** |
| "no MSRV" | 3.5 | **Refuted, and refuted a month ago.** `rust-version = "1.88"` since rmcp `3.0.0-beta.2`, via [rust-sdk#1034](https://github.com/modelcontextprotocol/rust-sdk/pull/1034), merged 2026-07-23 |
| "no RustSec advisory" | 4.3 | **Refuted.** `RUSTSEC-2026-0189` is keyed `package = "rmcp"`, `patched = [">= 1.4.0"]` |
| "no everything server" | 3.4 | **Unchanged.** Still none; rmcp's own client examples drive the *TypeScript* everything server over `npx` |

The tier move is corroborated three ways rather than read off one page: the rendered table,
a diff of upstream `main`'s per-revision sources (`2025-11-25/sdk.mdx` still says Tier 2;
`2026-07-28/sdk.mdx` and `draft/sdk.mdx` say Tier 1), and rust-sdk's own `ROADMAP.md`, which
opens "**Status: all SEP-1730 Tier 1 requirements are met**" with 100% server and client
conformance on both dated suites. It is also *later than the revision ship*: the 2026-07-28
blog post still said "All four Tier 1 SDKs speak 2026-07-28 as of today: TypeScript Python Go
C#" and placed Rust outside that set. The exact assignment date could not be established from
this session and is deliberately not claimed.

Two things about how this was found matter more than the facts themselves.

**The MSRV clause had been refuted for a month and the chain never moved.** Register row 3.5
was corrected on 2026-07-26 with a full account of rust-sdk#1034. The charter, ADR-0002, and
the conformance strategy went on saying "no MSRV" for the next twenty-nine days. The
register's own maintenance rule 3 — *"a refuted row is corrected immediately and anything
derived from it is re-examined; the derivation chain is what the `Used by` column tracks"* —
was applied to the row and not to the chain. The rule was followed halfway, which is
indistinguishable from not following it for anyone reading the charter.

**Nothing would have failed if the sweep had not been run.** The sweep happened because a
hand-written deferral-ledger row scheduled it, and that row's own scope figure ("50 rows") was
a `grep` miscount. The register states a 90-day rule in its own prose and no gate reads it.

Meanwhile, two findings from the same sweep cut the *other* way, and an honest record has to
carry both. SEP-2484 supersedes SEP-1627 and says so in as many words: *"SEP-1627's
golden-trace approach was not carried forward… **SEP-1627's protocol-debugger ideas remain
valuable future work**"* (row 2.12). And the roadmap published 2026-08-22 names, as one of five
priority areas, investing in the SDKs' "ergonomics and **their conformance with the
specification**" (row 1.9).

## Decision

1. **The scope decision in [ADR-0002](0002-product-scope.md) stands. Its justification
   narrows to one leg, and that is stated rather than glossed.** ADR-0002 rested on two
   independent arguments: *the Rust SDK has gaps*, and *nobody has built the offline half of
   conformance in any language*. The first has largely closed. The second is the one the
   product is on, and the sweep strengthened it: the official suite's answer to
   "what does this revision require" is now frozen per-revision requirement sets of **live
   scenarios** (register 2.18), and the authority has explicitly declined the golden-trace
   fixture model while calling the trace-analysis half open work (register 2.12). Nothing
   in the sweep found a second implementation of what this workspace builds.

2. **The charter's third premise is corrected to the one gap that survives**, and its other
   premises are corrected where the sweep moved them. A premise that has been refuted is not
   softened or hedged in place; it is replaced by what is true, with the refutation visible.

3. **Engagement framing changes, because three of the four things we planned to offer are
   already done.** Offering to close a gap that closed is not a credible approach and
   damages the offer that remains. Concretely: the MSRV-policy issue draft and the RustSec
   advisory draft are **obsolete — do not file** (both already recorded as such in rows 3.5
   and 4.3); the everything-server contribution is the one that still answers an open need;
   and the tier-gap report ([M5](../06-roadmap.md)) must state which tier it measured against,
   since "closing a Tier-2 gap" is no longer the frame.

4. **Risk [R2](../08-risk-register.md) is re-stated, not left armed.** Its watch signal was
   "SEP-1627 leaving Draft" as an early warning that the official suite would absorb trace
   validation. The signal fired — with the opposite polarity. The authority declined the
   fixture half and named the analysis half unclaimed. A watch signal still armed for an
   event that already happened is a defect in the risk register, not vigilance.

5. **The register's 90-day rule becomes a gate: `cargo xtask register-currency`.** It reads
   the register's own `Verified` dates and fails once any row passes ninety days. It runs in
   the weekly `claims-expire` job under [ADR-0010](0010-deferral-ledger-and-scheduled-reverification.md),
   never as a PR gate, for ADR-0010's own reason: an expiry must page the schedule, not block
   unrelated work. Its structural half — every row's status is one of the three the register
   defines, every row carries a parseable date, no date is in the future — runs inside
   `cargo xtask ci`, because a malformed row is a defect in the change that introduced it.
   This replaces the hand-written ledger row that scheduled this sweep: the dates the gate
   reads are the ones the rows already carry, so the scope cannot be miscounted and the
   schedule cannot be forgotten.

6. **A row that could not be re-verified keeps its old date.** The sweep could not reach
   GitHub issue state (`api.github.com` refuses cross-repository reads; `github.com` HTML and
   `.atom` return 403), so rows 2.13, 3.6, 3.11 and half of 3.7 were *not* advanced. They
   stay outside the citation window and will trip the new gate on 2026-09-07 if still
   unchecked, which is the correct outcome. Re-dating a row nobody re-checked is the single
   failure that would make this register worthless, and it is the cheapest mistake to make.

## Consequences

### Positive

- The charter says what is true. A reader who checks premise 3 against the published tier
  table now finds agreement instead of a fourteen-week-old snapshot.
- The scope's justification is honest about being narrower. One strong argument that survives
  contact with the evidence is worth more than four that have to be defended.
- Rule 1 stops depending on someone remembering. The gate reads dates that already exist in
  the rows, so the schedule is derived from the data rather than mirrored beside it.
- Rule 3's failure mode is now visible in the record. The register documents that the MSRV
  correction did not propagate for twenty-nine days, so the next reader knows the chain is
  the part that gets skipped.
- The engagement offer that remains — the everything server — is the one SEP-1730's appendix
  actually asks for, and it is now the *only* published gap, which sharpens rather than
  weakens it.

### Negative

- **The project's legitimacy story is weaker than it was.** "Helping a Tier-2 SDK reach Tier
  1" was a clean, externally-verifiable motivation. "Building the offline half nobody has
  built" is true and better-evidenced, but it is an argument about absence, and arguments
  about absence age badly — the sweep that produced this ADR is exactly how one would be
  refuted.
- **The tier promotion is dated only to a four-week window.** This session could not read
  upstream commit history, so "between 2026-07-28 and 2026-08-24" is the honest bound. A
  reader who needs the date must go and get it; this ADR does not supply one.
- **`register-currency` will fail on 2026-09-07** for the four rows GitHub blocked, in a job
  that opens a tracking issue. That is the gate working, but it means the first thing it does
  is report a known problem, and a gate whose first act is a known failure is easy to learn
  to ignore.
- **The gate reads prose.** The register is Markdown tables, not data, so the gate parses a
  document meant for humans; a table reshaped for readability can break it. The structural
  half running in `ci` limits the blast radius to the change that reshapes it, but the
  coupling is real and is the price of not converting the register to JSON.
- **Some corrected prose is now longer than what it replaced.** Rows that record their own
  refutation carry both the old reading and the new one. That is deliberate — the
  supersession is often the finding — but the register is measurably harder to skim than it
  was, and no gate protects against that.

## Alternatives Considered

### Leave the charter alone; the register is the source of truth anyway

Rejected. The charter is the document a reader meets first, and rule 3 exists precisely
because "the register has it right somewhere" is not a defence. The twenty-nine-day MSRV lag
is the demonstration: the register did have it right, and three other documents went on being
wrong.

### Treat the Tier-1 promotion as invalidating the project and re-scope

Rejected as an over-reading of one fact. The tier table measures an SDK against SEP-1730's
requirements; it says nothing about whether recorded-trace validation exists, and SEP-2484
says in terms that it does not and that the idea is open. Re-scoping on premise 2's collapse
while premise 5 stands would be the same error in the other direction — reacting to the
loudest fact rather than the load-bearing one.

### Make `register-currency` a PR gate rather than a weekly one

Rejected for [ADR-0010](0010-deferral-ledger-and-scheduled-reverification.md)'s reason,
which applies verbatim here: a row ageing past ninety days has nothing to do with whatever
pull request happens to be open, and blocking that PR neither re-verifies the row nor finds
the person who can. The structural half *is* a PR gate, because a malformed row is caused by
the change that introduced it.

### Convert the register to JSON so the gate reads data instead of prose

Rejected for now, and not comfortably. It would remove the parser's coupling to table
layout, and `deferrals.json` proves the shape works. But the register's value is in rows that
argue with themselves — quoting the source, recording what was refuted and how the earlier
reading went wrong — and that prose is the artifact, not decoration around a date. Splitting
dates into JSON and prose into Markdown would put the two out of sync exactly the way the
charter and register went out of sync. Revisit if the parser breaks twice.
