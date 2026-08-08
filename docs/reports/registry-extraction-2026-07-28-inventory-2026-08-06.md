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


---

## Addendum 2 (2026-08-06): the blocker was wrong — the engine already had the state

The addendum above concluded that a faithful extraction needed either a new
`Verification` variant or a full check implementation first, because `exclusion`
may not mean "check not written yet". The first half of that stands. The
conclusion did not: **the engine already models exactly the missing state**, one
layer down from the schema.

A requirement whose `checks` name an unimplemented check is reported
`unsupported` — first-class in `Totals`, listed with the missing check IDs, and
*outranking* fail/warn/pass in verdict priority (`engine.rs`,
`verdict_priority_is_unsupported_fail_warn_pass`). The engine's own comment is
the point: `unsupported` "is a property of (registry, build), not of what one
trace negotiated".

So an entry can say, truthfully, "this clause is verified by check X" while the
build reports that X is absent. That is not a placeholder exclusion — it is a
true statement plus a visible build fact, and it cannot be mistaken for a pass.
No schema change is needed, and the recommendation to add a `Deferred` variant
is withdrawn: it would have duplicated, at the schema layer, a distinction the
engine already draws better.

**First slice landed on that basis:** `basic/index#meta`, 16 clauses, curated
individually as BASE-025…040. Five reuse the implemented `base.meta-key-format`
check, because the `_meta` key grammar is unchanged from `2025-11-25` and the
check already carries a corpus violation. Five carry genuine exclusions — two of
them because the clause's own "unless specifically configured not to do so"
escape hatch makes an absent field unattributable, which is a property of the
clause, not of our engine. Six name checks that do not exist yet and therefore
report `unsupported`, which is the honest answer until they are written.


---

## Addendum 3 (2026-08-06): `basic/index` complete — 57/57 entered

The page's remaining 41 clauses are curated, so `basic/index` is fully entered
and the first in-scope page is closed. All 57 quotes verify against the live
text (`spec-drift`: 197 across both revisions, 0 drifted).

| Disposition | Entries | Meaning |
|---|---:|---|
| Judged today | 12 | Named check is implemented and already carries a corpus violation |
| `unsupported` | 14 | Named check does not exist yet; the build says so, loudly |
| Excluded | 31 | No recorded trace can judge the clause, reason stated per entry |

**Twelve clauses judge on day one** because the JSON-RPC envelope did not change:
message shape, request-ID typing and nullability, result/error ID correlation,
the error `code`/`message` shape, integer error codes, notification IDs and the
`_meta` key grammar all reuse checks the `2025-11-25` corpus already falsifies.
That is the payoff of extracting per revision rather than sharing entries — the
quotes differ, the checks do not.

**One clause looked reusable and is not.** At `2025-11-25`, BASE-003 forbids a
request ID that "MUST NOT have been previously used by the requestor within the
same session". At `2026-07-28` the rule is narrower: the ID "MUST NOT match the
ID of any other request the sender has issued and **not yet received a response
for**" — reuse after completion is now legal. Pointing BASE-045 at the existing
`base.request-id-unique` would have reported conforming traces as violations, so
it names a new `base.request-id-unique-in-flight` and reports `unsupported`
until that exists. This is exactly the failure mode per-clause curation is for;
a bulk mapping from old IDs to new would have shipped it.

**Where the exclusions concentrate**, and why they are honest rather than
convenient: the statelessness clauses bind *reliance* and *preparedness*, which
are inferences a trace cannot witness; the JSON Schema clauses bind validator
behavior inside each implementation, which `mcp-conformance-core` deliberately
cannot evaluate (no JSON Schema engine — 02-architecture.md); the icon clauses
bind consumer conduct after the session ends; and the `resultType`
backward-compatibility rules bind how a *client interprets* an absent field.
Several restate `2025-11-25` clauses whose exclusions this project already
argued, and those entries say so and cite the original.

The registry file was split at the page's own section seams — `messages`,
`statelessness`, `schema`, `meta`, `icons` — when it crossed the 500-line cap.

**Next**: the 14 `unsupported` checks are the natural unit. Each needs an
implementation plus a `corpus/draft/` violation pair, and
`corpus_falsifies_every_check` needs extending to drive the `2026-07-28`
registry over that corpus — today it only runs `builtin_2025_11_25()`.


---

## Addendum 4 (2026-08-06): the 14 checks are implemented — `basic/index` judges

All 14 clauses that reported `unsupported` now have implementations and a
violation trace each. The `2026-07-28` registry is **26 judged / 0 unsupported /
31 excluded**: nothing in it is aspirational any more.

Six read the message envelope — `resultType` presence, in-flight request-ID
collision, and the four error-code partition rules (legacy sub-range, undefined
codes in the MCP-reserved sub-range, withdrawn `-32002`/`-32042`, and
application codes misplaced inside the JSON-RPC reserved range). Eight read the
`_meta` envelope — required request fields, the `-32602` a malformed envelope
must draw and its HTTP 400, the `-32021` shape and *its* 400, undeclared-capability
reliance surfaced through MRTR `input_required`, subscription tagging, and W3C
`traceparent` grammar.

Each is falsifiable by construction: where a clause has only a positive form
("the server never relied on X"), the check reports the one wire-visible way it
fails rather than pretending to prove the positive. Clauses with *no* such form
stayed excluded.

**Three existing invariants had to be widened, and each caught a real gap:**

1. `builtin_registry_and_check_inventory_cover_each_other_exactly` drove
   `Registry::builtin_2025_11_25()`, so it failed the moment a check existed
   that only the newer revision names. It now drives the registry **set** —
   both halves still bind, across revisions.
2. `corpus_falsifies_every_check` drove one corpus. It now unions both, so the
   invariant is "*some* corpus kills each implemented check", which is the
   property that actually matters.
3. `every_trace_has_a_provenance_ledger_row` covered only `good` and
   `violations` — the 15 new fixtures were undocumented and the test was
   silently fine with it. It now covers `draft/` too, and `corpus/README.md`
   carries a row per trace.

The check registrations are gated on `draft-2026-07-28` alongside the data, so
a default build has neither, and the falsifiability invariant holds in both
feature modes rather than only the one that happens to be tested.
