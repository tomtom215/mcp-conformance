<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0001: Plan Documentation Model

**Date:** 2026-06-09
**Status:** Accepted
**Author:** Tom F.

---

## Context

This project starts as a plan before it is code, and the plan must remain authoritative for
the life of the project — growing as scope grows, without rotting. The failure mode is
well-understood from direct experience: a2a-rust's `docs/implementation/plan.md` was a
single monolithic document that served the build phase well, then aged into a "Historical
Reference" carrying correction banners ("Some details are outdated…") because volatile facts,
status, decisions, and intent all lived interleaved in one file. Each kind of content rots on
a different clock; a monolith rots on the fastest one.

A second failure mode is **changelog creep**: documents accumulating "Update (date): …"
prefaces and edit-history sections until the current state is something readers reconstruct
rather than read. Git already stores history with perfect fidelity; duplicating it inline
trades readability today for archaeology forever.

## Decision

The plan is a modular corpus under `docs/plan/`, governed by six rules:

1. **One concern per file.** Charter, verified facts, architecture, conformance strategy,
   standards, security, roadmap, engagement, risks — each owns its domain. New concerns get
   new files. No file becomes a dumping ground.
2. **Documents state current intent only.** No inline changelogs, no "Update:" banners, no
   strikethrough archaeology. Git history is the changelog. A document that needs a
   correction gets corrected.
3. **Each content type has exactly one home, matched to its rate of change.** Volatile
   external facts → the [register](../01-ecosystem-context.md), every row dated and sourced.
   Status → the [roadmap](../06-roadmap.md) only. Decisions → ADRs, append-only. Everything
   else links to these homes instead of copying from them — one fact, one place, many links.
4. **Volatile numbers never appear in prose.** Versions, download counts, star counts, tier
   tables live only in register rows where their verification date is visible.
5. **Metadata over banners.** Every document carries `Status` and `Last reviewed`. The
   90-day review sweep ([plan README](../README.md)) is what prevents silent staleness —
   the a2a-rust banner was the symptom of reviews that had no scheduled trigger.
6. **Numbered prefixes are reading order; numbers are never reused or shuffled.** Links must
   not break. New documents take the next free number even if the thematic order suffers.

ADRs use the a2a-rust format (Context / Decision / Consequences incl. negative /
Alternatives Considered) with statuses transitioning forward only; superseding means a new
ADR, not an edit.

## Consequences

### Positive

- The plan scales by addition, not by swelling: growth lands in new files and new register
  rows, so review diffs stay local and legible.
- Staleness is detectable mechanically (review dates, register dates) instead of by vibes.
- Readers get current truth in one pass; historians get `git log`, which they trust more
  than inline notes anyway.
- Facts carry provenance, so a refuted fact can be traced to everything built on it
  (the register's `Used by` column).

### Negative

- More files means navigation overhead; the plan README's index and "read when" column are
  load-bearing and must be maintained.
- Cross-linking discipline takes real effort; a lazy edit that inlines a number instead of
  linking the register will pass CI and must be caught in review.
- The 90-day sweep is unglamorous recurring work with no feature output.

## Alternatives Considered

### Single living plan.md

The a2a-rust approach. Rejected: empirically observed to decay into a historical artifact
with correction banners; review diffs touch one giant file; volatile and stable content
share a fate they shouldn't.

### Wiki / external tracker (GitHub wiki, Notion, issues-as-plan)

Rejected: splits truth from the repo, breaks PR-reviewed changes to the plan, invisible to
clones and forks, and wikis have even worse staleness mechanics than monoliths.

### Changelog sections per document ("Document history" tables)

Rejected explicitly: this is changelog creep institutionalized. Git provides it for free
with authorship, dates, and diffs; inline copies go stale and pad every file.

### Pure ADR stream (no synthesized documents, decisions only)

Rejected: ADRs capture *why* superbly and *what is currently true* terribly — reconstructing
current architecture from 40 ADRs is the monolith problem inverted. Synthesis documents plus
append-only decisions is the standard resolution.
