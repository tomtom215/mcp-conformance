<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# `2026-07-28` registry extraction: the quote-verified clause inventory

**Date:** 2026-08-06
**Status:** input to roadmap M2.5 line 2. The inventory is complete and
verified; the per-clause curation it feeds is **not** done. This report says
exactly where the boundary is, so nobody mistakes one for the other.

---

## What is finished, and why it is trustworthy

`tools/extract-clauses.py` turns a published spec page into candidate registry
entries whose quotes are guaranteed to survive `cargo xtask spec-drift`. That
guarantee is the point: the extraction method's error-prone half has never been
finding the clauses, it has been producing a quote that still matches the
published text after the gate's normalization.

The tool ports `xtask/src/spec_drift.rs`'s `normalize` and `quote_present`
exactly, and the port is **calibrated against the shipped registry**: all
**140/140** committed `2025-11-25` quotes verify under it, LIFE-009 included —
that one exercises the intro-colon path, where a quote cites a parent clause
plus a selected bullet and appears nowhere contiguously in the page.

Run over the fifteen in-scope `2026-07-28` pages, every one of the **274**
extracted clauses verifies against the live page text: **274/274**.

Two defects were found by that check failing, not by inspection, and both are
now encoded in the tool:

- **`spec-drift` does not strip MDX component tags.** A clause spanning
  `<Warning>` can therefore never match, however well-formed it looks. Tags are
  treated as clause boundaries.
- **Markdown tables restate prose as a matrix**, and their pipes corrupt
  sentence boundaries. Table rows are dropped.

## The inventory

Levels follow the registry's existing inclusion policy, measured from the
shipped data rather than assumed: `2025-11-25` admits **MUST / MUST NOT /
SHOULD / SHOULD NOT** only — no MAY, no RECOMMENDED, no OPTIONAL — across 140
entries (68/12/55/5), of which 52 carry checks and 88 carry documented
exclusions, matching the totals v0.4.0's changelog states.

| Page | MUST | MUST NOT | SHOULD | SHOULD NOT | Total |
|------|-----:|---------:|-------:|-----------:|------:|
| `basic/transports/streamable-http` | 42 | 5 | 12 | 1 | **60** |
| `basic/index` | 31 | 12 | 10 | 4 | **57** |
| `server/tools` | 10 | 2 | 14 | 3 | **29** |
| `basic/patterns/mrtr` | 13 | 8 | 5 | 0 | **26** |
| `server/utilities/caching` | 6 | 3 | 8 | 1 | **18** |
| `basic/transports/stdio` | 2 | 7 | 5 | 1 | **15** |
| `server/resources` | 6 | 2 | 5 | 0 | **13** |
| `server/prompts` | 7 | 1 | 3 | 0 | **11** |
| `basic/versioning` | 4 | 0 | 4 | 0 | **8** |
| `server/utilities/logging` | 1 | 3 | 3 | 1 | **8** |
| `basic/patterns/subscriptions` | 2 | 2 | 3 | 0 | **7** |
| `server/utilities/pagination` | 2 | 2 | 3 | 0 | **7** |
| `server/utilities/completion` | 3 | 0 | 3 | 0 | **6** |
| `basic/transports/index` | 3 | 0 | 2 | 0 | **5** |
| `server/discover` | 1 | 0 | 2 | 1 | **4** |
| **Total** | **133** | **47** | **82** | **12** | **274** |
For scale: the shipped `2025-11-25` registry is 140 entries over 9 pages. This
revision is **274 over 15** — roughly twice the surface, because the stateless
rework splits transports into three pages and adds four mechanisms that did not
exist before (`server/discover`, MRTR, subscriptions, caching).

## In-scope determination

In scope are the direct successors of the `2025-11-25` in-scope set, plus the
new core mechanisms that *replace* machinery which was already in scope:

- `basic/patterns/mrtr` replaces server-initiated requests
- `basic/patterns/subscriptions` replaces the HTTP GET stream and
  `resources/subscribe`
- `server/discover` replaces `initialize`'s capability advertisement
- `basic/versioning` replaces `basic/lifecycle`'s version negotiation
- `server/utilities/caching` adds required fields to list endpoints already in
  scope

Out of scope, carrying forward the `2025-11-25` reasons: `architecture` and
`server/index` (no keyword instances), `basic/authorization/*` (full OAuth —
the TRAN-009 boundary and the `auth-client-scenarios` ledger row),
`basic/patterns/{cancellation,progress,index}` (unclaimed surface, expansion
candidates), `client/*` (client-feature pages), `changelog`, `deprecated`,
`index`, `schema`.

## What is NOT done

**The curation.** Each of the 274 clauses still needs its `id`, `actor`,
capability gate, `applies` range, and — the real judgment — a check or a
*specific* documented exclusion. The tool deliberately does not guess these.

That work is genuinely large, and it should not be faked. Nearly every entry
will carry an exclusion at this stage, because the validator models the
`2026-07-28` lifecycle only (`context::draft`) and not MRTR, subscriptions,
discovery or caching. An exclusion is legitimate under the DoD — but only if
its reason is specific to the clause. Generating 274 boilerplate exclusions
would produce a registry that looks complete, passes its own gates, and tells a
reader nothing. That is a worse outcome than an inventory that is honest about
being an inventory.

## Wiring still required for M2.5 line 2

1. `registry/2026-07-28/*.json` per area, plus `sources.json` for the fifteen
   in-scope pages.
2. ~~**Every existing `2025-11-25` entry needs `applies: {removed: "2026-07-28"}`.**~~
   **Done** — all 140 entries are bounded, with two tests standing guard: one
   asserts the `2025-11-25` projection still reconstructs
   `Registry::builtin_2025_11_25()` byte-for-byte at 140 entries, the other that
   no embedded requirement applies at `2026-07-28`. An absent range means *every*
   revision, so without the bound all 140 entries — whose quotes cite
   `2025-11-25` pages — would have leaked into the new revision and read as if it
   had been extracted.
3. ~~`RegistrySet::builtin()` extended to describe both revisions, behind the
   off-by-default `draft-2026-07-28` feature.~~ **Done** — off by default;
   with the feature, `registry("2026-07-28")` answers with an empty-but-real
   registry, which `RegistrySet::registry`'s contract already anticipated.
4. `spec-drift` iterating both revisions — the helpers already resolve per
   revision, so this is a loop, not a rewrite.
5. `corpus/draft/` good and violation pairs (DoD line 5).

---

## Addendum (2026-08-06): the curation is blocked on checks, not on data entry

Curation was started page by page, beginning with `server/discover` (4 clauses).
It stopped at the first entry, on a policy question that decides all 274.

`Verification::Excluded` documents itself as "Not mechanically verifiable from a
recorded trace", and
[03-conformance-strategy.md](../plan/03-conformance-strategy.md) §What enters
the registry is explicit about the rule:

> Every MUST / MUST NOT on an in-scope page enters — with checks when a recorded
> trace can judge it, with a documented exclusion naming where it *is* enforced
> when it cannot. No exceptions: that is the SEP-2484 floor.

So `exclusion` means **a trace cannot judge this clause**. It does not mean
*the check has not been written yet*. Every exclusion in the shipped registry
holds to that: LIFE-011 excludes because "the client's supported-version set is
internal state with no wire footprint"; LIFE-015 because "checks may not
consult time"; LIFE-014 because stdin closure and signals "are host-OS actions
outside the trace event vocabulary".

Take the first `server/discover` clause, "Servers MUST implement it." A trace
that carries a `server/discover` request and a `-32601` answer falsifies it
outright. It is *wire-observable*, so under the rule it must carry a **check** —
and writing `exclusion: "the validator does not model 2026-07-28 yet"` would be
a false statement about the clause, not a documented limitation. Doing that 274
times would produce a registry that passes every gate while quietly inverting
the meaning of its own central field, which is precisely the "private dialect"
the strategy document opens by forbidding.

**What this means for M2.5 line 2.** The remaining work is not data entry. Most
of the 274 clauses are wire-observable, so the honest path requires the
validator to actually model the revision — the stateless lifecycle beyond
`context::draft`'s phase model, MRTR, subscriptions, discovery and caching —
and a check per observable clause. The extraction tool and inventory remove the
quoting risk from that work; they do not shrink it.

Three ways forward, none of which should be chosen silently:

1. **Implement checks alongside entries, area by area.** Faithful to the rule
   and to the DoD. Largest, and the only one that yields a registry that
   *judges* anything.
2. **Extend `Verification` with a third variant** — a `Deferred { reason }`
   that means "wire-checkable, check not yet implemented" — so the inventory can
   land as data without lying about coverage. This is a core schema change: the
   coverage manifest, the agreement check and report denominators all read this
   enum, and ADR-0006's not-applicable semantics sit next to it.
3. **Land only genuinely unobservable clauses now.** Honest, and nearly useless:
   it would populate the registry with exactly the requirements no trace can
   judge.

Recommendation: **(2) first, then (1) incrementally.** The variant makes the
inventory landable and visible without overstating coverage, and keeps the
pass-rate denominators honest, while checks arrive area by area behind the
`draft-2026-07-28` feature. It should be an ADR, because it changes what a
registry entry can claim.
