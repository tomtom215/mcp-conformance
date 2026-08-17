<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# `2026-07-28` registry extraction: the quote-verified clause inventory

**Date:** 2026-08-06
**Status:** roadmap M2.5 line 2 is **complete** as of 2026-08-17 — all fifteen
in-scope pages are curated, 272 entries, 0 unsupported. See Addendum 7 for the
finished state; the body below is the record of how it got there, kept because
the reasoning is the reviewable part.

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

*(Status line as of 2026-08-08 — see Addendum 6 for the current numbers; items
2, 3 and 4 are closed, 1 and 5 are the live work.)*

1. `registry/2026-07-28/*.json` per area, plus `sources.json` for the fifteen
   in-scope pages. **In progress — 2 of 15 pages.** Ten area files exist and
   `sources.json` grows as pages land, by the policy written into that file:
   `out_of_scope` means *deliberately excluded*, so a page reached by neither
   bucket is simply not extracted yet.
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
4. ~~`spec-drift` iterating both revisions — the helpers already resolve per
   revision, so this is a loop, not a rewrite.~~ **Done** — `REVISIONS` drives
   `verify_revision` per revision and skips one the built registry does not
   describe; 257 quotes across both, 0 drifted.
5. `corpus/draft/` good and violation pairs (DoD line 5). **In progress —
   two good sessions (stdio and Streamable HTTP) and 33 violation traces, one
   per clause, each with a provenance row in `corpus/README.md`.** The pairs
   arrive with their area, not afterwards: `corpus_falsifies_every_check`
   fails the build for any implemented check no corpus trace kills.

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


---

## Addendum 5 (2026-08-06): `basic/transports/streamable-http` entered — 60 clauses

The revision's largest page is curated. The registry is now **117 entries — 30
judged, 21 unsupported, 66 excluded** — across two of fifteen in-scope pages,
and `spec-drift` verifies 257 quotes across both revisions, 0 drifted.

Four checks carried over because the rule did not change: the `Accept` header
listing both content types, the single-message POST body, the `Content-Type`
the server may answer with, and the `MCP-Protocol-Version` header's presence.
One reuse was checked rather than assumed —
`transport.http-post-single-message` enforces *singleness* only, not which
message kinds are permitted, so it still fits a clause that narrowed from
"request, notification, or response" to "request or notification". The narrowing
itself is a separate new clause (clients may no longer POST responses at all).

Thirteen checks are named and not yet implemented, so their entries report
`unsupported`. **That created a conflict with an invariant this session had just
tightened.** `builtin_registry_and_check_inventory_cover_each_other_exactly` was
changed one commit earlier to drive the registry *set*, and it asserts every
referenced check exists — a rule stricter than the engine, which supports
`unsupported` deliberately. Rather than weaken it, the assertion now consults a
committed `PLANNED` list, and the test retires each row itself: implementing a
planned check without deleting its row fails, and so does a planned check no
requirement references. A misspelled check id therefore still fails loudly
instead of degrading silently into `unsupported`, which was the protection worth
keeping.

The 66 exclusions cluster in three honest places. Intermediary clauses bind a
party that is not an endpoint of the recorded session. Encode/decode clauses
bind a step that happens inside a receiver, where only the outcome reaches the
wire — and that outcome is checked. The `x-mcp-header` schema constraints
require walking a tool's JSON Schema, which `mcp-conformance-core` has no engine
to do by deliberate architectural choice, the same boundary BASE-073 draws.

**Next**: the thirteen `PLANNED` checks, with corpus pairs, exactly as the
fourteen for `basic/index` were done.

---

## Addendum 6 (2026-08-08): the planned checks are implemented — the transport judges

Every clause that reported `unsupported` now has an implementation and a
violation trace. The `2026-07-28` registry is **117 entries — 51 judged / 0
unsupported / 66 excluded**, and `PLANNED` is empty for the first time since it
was introduced. `spec-drift` verifies 257 quotes across both revisions, 0
drifted.

Addendum 5 committed to thirteen checks. **Eighteen landed**, and the difference
is the substance of this pass rather than scope creep.

### One reuse was vacuous, and would have reported a pass

TRAN-071 ("every POST request MUST include an `MCP-Protocol-Version` header")
was pointed at the `2025-11-25` check of the same purpose. That check begins by
locating the negotiated version in the `initialize` result and returns early
without it — and `2026-07-28` has no `initialize`. It would have inspected
nothing and reported **pass**, which is worse than the `unsupported` it
replaced: an absent check is visible in the totals, a vacuous one is not.
TRAN-071 now names `transport.protocol-version-header-present`, written against
the POST itself. The other three carried-over reuses were re-read for the same
failure mode and are sound — none consults the handshake.

### Five clauses were being judged by a check that bundled their neighbours

`transport.header-value-encoding` covered TRAN-077/086/087/089/092 and
`transport.header-mismatch-rejected` covered TRAN-073/096/098/102. Because the
engine attributes a check's finding to every requirement naming it, a trace
carrying an unencoded non-ASCII value reported TRAN-089 — the *marker-case*
rule — as failed. The product's claim is requirement-level findings, so the two
were split along the rules they actually state:

| Check | Requirements | Rule |
|---|---|---|
| `transport.header-value-encoding` | TRAN-077, TRAN-086, TRAN-087 | a value that cannot be carried plainly is Base64-encoded |
| `transport.sentinel-marker-case` | TRAN-089 | the sentinel markers are exactly lowercase |
| `transport.sentinel-pattern-encoded` | TRAN-092 | a plain value shaped like the sentinel is encoded too |
| `transport.version-mismatch-rejected` | TRAN-073 | a version header/body mismatch is rejected |
| `transport.invalid-param-header-rejected` | TRAN-096 | a recognized `Mcp-Param-*` with invalid characters is rejected |
| `transport.header-mismatch-status` | TRAN-098, TRAN-102 | a `HeaderMismatch` rejection carries HTTP 400 |

The pairs that remain shared — TRAN-077/086/087, TRAN-097/100, TRAN-098/102 —
state *one* rule in several sections, which is the same reason TRAN-025/039
share a check at `2025-11-25`.

### `http_status_after` searched the wrong direction

The helper the HTTP-status clauses use looked *forward* from a message for the
next recorded status. The tap records a response's `http` event **before** the
message it framed (`mcp_everything_server::tap::record_response`), and every
captured trace in `corpus/good/` shows that order. On a real capture the helper
would therefore have read the *next* exchange's status, or none at all — so
BASE-032 and BASE-036 would have passed vacuously since the day they landed.
It now scans backwards to the nearest server-sent status, which also handles
SSE correctly: every frame of one response rides one status event. The two
corpus traces that had been authored to fit the bug are reshaped to the capture
order.

### What the trace can now judge that it could not before

- **Base64 sentinel decoding.** A ~25-line RFC 4648 decoder next to the existing
  validator (no new dependency; the judgment surface stays `serde`-only, and it
  is gated with its caller). Header/body comparison now decodes first — the
  comparison TRAN-091 and TRAN-103 require of servers — instead of abstaining on
  the case the specification spends most of its words on. Its test decodes the
  spec's own encoding table verbatim.
- **`x-mcp-header` annotations by property path.** The walk follows chains of
  `properties` keys only, which is the specification's *statically reachable*
  definition, and reads the argument at the exact annotated path. This is a
  structural walk, not a JSON Schema engine — the reachability clauses that do
  need one (TRAN-081, TRAN-082) stay excluded.
- **TRAN-080 in full.** All four constraints its quote states — non-empty,
  field-name token syntax, case-insensitive uniqueness within one `inputSchema`,
  and primitive-typed properties only — rather than the first two.
- **TRAN-074's obligation side.** When a trace carries the server's own
  `server/discover` result, a request naming a version outside its
  `supportedVersions` must draw `-32022`. The list comes from what the server
  said about itself; there is no assumption about which versions it ought to
  implement.

### Boundaries the specification drew, and the checks respect

Notification POSTs are not judged by any header clause: the revision states
that "header requirements for notification POSTs are not defined by this
revision". Cancellation is anchored to a recorded transport close or abort,
because closing a request's response stream *is* the cancellation signal
(TRAN-069) — and only ids still outstanding at that moment are judged.

### Corpus

A conformant HTTP session (`draft/good/streamable-http-session.jsonl`) exercises
discovery, an `x-mcp-header` annotation mirrored into `Mcp-Param-Region`, an SSE
response with `X-Accel-Buffering: no`, and a non-ASCII `Mcp-Name` riding the
sentinel — 51 checked entries pass, none vacuously. Eighteen violation traces
follow; where one falsifies more than one requirement, `corpus/README.md` says
which and why, and there are only two reasons: several sections stating one
rule, or a clause whose antecedent is itself the other party's violation (a
server cannot fail to reject a bad header unless the client sent one).

Two files crossed the 500-line cap on the way and were split at subject seams
rather than at arbitrary line counts: the check inventory left `checks/mod.rs`
for `checks/inventory.rs`, and the response-stream clauses left
`draft/transport.rs` for `draft/transport/stream.rs`.

**Next**: the thirteen remaining in-scope pages of the revision.

---

## Handover (2026-08-08): where this stands, and how to resume

This report is the working document for roadmap M2.5 line 2. Everything below is
verified state, not intent.

### State

| | |
|---|---|
| Branch | `claude/text-release-check-suq6cc`, 13 commits ahead of `main`, all gates green |
| Pages entered | **2 of 15** in-scope: `basic/index`, `basic/transports/streamable-http` |
| `2026-07-28` registry | **117 entries — 51 judged, 0 unsupported, 66 excluded** |
| `2025-11-25` registry | **140 entries, unchanged** — byte-equality with `Registry::builtin_2025_11_25()` is a test, not a hope |
| Quote verification | `spec-drift`: **257 quotes across 2 revisions, 0 drifted** |
| Corpus | 2 good `2026-07-28` sessions (stdio, Streamable HTTP) + 33 violation traces, each with a `corpus/README.md` row |
| Feature gate | everything `2026-07-28` is behind `draft-2026-07-28`; the default build is untouched |
| `PLANNED` ledger | **empty** — nothing in the registry names a check that does not exist |

### Resolved: the draft checks are unit-tested (PR #35)

The diff-scoped mutation gate failed this branch at **391 mutants tested, 75
missed** — every missed mutant a behaviour change no test observed. The cause
was a gap against this repository's own standard: each `2025-11-25` check module
carries 6–11 unit tests, and all six draft modules carried **zero**. The corpus
proves *falsifiability* — one trace kills each check — but one trace exercises
one path, so branch conditions, boundary comparisons and the pure helpers went
unobserved.

Closed by a `tests` sibling per module (siblings rather than inline, because
inline tests count against the 500-line cap) plus a shared `testkit`: **145 lib
tests, up from 116**, and a scoped re-run reporting **356 mutants tested, 356
caught, 0 missed**.

Three findings are worth carrying forward, because they are the parts that were
not simply "write more tests":

1. **Two mutants were provably equivalent** and no test could ever have killed
   them, so the code changed rather than being suppressed. `decode_base64`
   accumulated with `(acc << 6) | sextet`, where the shift clears the low six
   bits and a sextet occupies only those — `|`, `^` and `+` are numerically
   identical there, so `+` is now used. `no_messages_after_cancellation`
   compared `event.seq < closed_at`, but a close is a *lifecycle* event and no
   message can share its `seq`, so `<` and `<=` were indistinguishable by
   construction; it is now one pass that flips a flag at the close.
2. **A test exposed a real rule question.** `header_value_encoding` skipped
   anything *shaped* like the sentinel, but `=?base64?café?=` is not an encoded
   value — it is a header that still cannot be transmitted. It is now judged,
   and only the miscased spelling is deferred to TRAN-089.
3. **Reading is not verification.** 73 of the 75 kills were predicted correctly
   by inspection; two were not, and both had the same shape — the mutation
   admitted an extra candidate that something further down silently discarded,
   so no assertion moved. Neither was visible without running the gate.

**For the next area: write the unit tests with the checks, not after.** The
corpus and the unit tests answer different questions — "does this check ever
fire?" and "does it fire on exactly the right thing?" — and only the second one
scales with the number of branches a check has.

### Resuming

The backlog is the clause inventory above: thirteen pages, extracted and
quote-verified, waiting on curation. Per page, the loop that produced the two
finished areas:

1. **Fetch the page** from `docs/specification/2026-07-28/<page>.mdx` in the
   `modelcontextprotocol/modelcontextprotocol` repository into a scratch
   directory.
2. **Extract candidates**: `python3 tools/extract-clauses.py <spec-root> <page>`.
   The tool ports `spec_drift.rs`'s normalization exactly and is calibrated
   against all 140 shipped `2025-11-25` quotes, so a quote it emits already has
   the shape the gate compares against. Never hand-transcribe a quote — BASE-039
   was written by hand and failed the gate on a reference-style link.
3. **Curate by hand**, one clause at a time — `id`, `actor`, capability gate,
   `applies` range, and the real judgment: a named check, or an exclusion whose
   reason is specific to *that* clause. The tool guesses none of these, and
   neither should the curator.
4. **Add the page to `in_scope`** in `registry/2026-07-28/sources.json`.
   `spec-drift` enforces both directions — every listed page is cited, every
   cited page is listed.
5. **Write the checks with their corpus pairs**, in the same commit or the next
   one. An entry may name a check that does not exist yet — the engine reports
   it `unsupported`, which is honest — but the id must then be listed in
   `PLANNED` (`checks/inventory.rs`), which retires each row automatically when
   its check lands.
6. **Verify**: `cargo xtask ci`, then
   `cargo run -q -p xtask --features draft-2026-07-28 -- spec-drift` (network),
   then `cargo xtask conformance`.

### What will catch a mistake, so it does not have to be remembered

- `describing_2026_07_28_does_not_change_what_2025_11_25_requires` — the shipped
  revision cannot drift while the new one is built.
- `builtin_registry_and_check_inventory_cover_each_other_exactly` — a misspelled
  check id fails loudly instead of degrading into `unsupported`.
- `corpus_falsifies_every_check` — an implemented check no trace kills fails the
  build.
- `every_trace_has_a_provenance_ledger_row` — an undocumented fixture fails the
  build.
- `spec-drift` — a quote that is not in the published text fails the build.

### Two hazards this work has already hit, both worth re-reading before starting

- **A reused `2025-11-25` check can be vacuous here.** Anything that consults the
  `initialize` exchange returns early at `2026-07-28` and reports *pass* without
  inspecting anything. Read a candidate for reuse to the bottom before pointing a
  clause at it (Addendum 6, TRAN-071).
- **A check that bundles adjacent rules makes every requirement naming it
  imprecise**, because the engine attributes a finding to all of them. Share a
  check only where the clauses state one rule across several sections
  (Addendum 6, the six-way split).

---

## Addendum 7 (2026-08-17): the extraction is complete — 15 of 15 pages

All fifteen in-scope pages of `2026-07-28` are curated. The revision's registry
is **272 entries — 124 judged / 0 unsupported / 148 excluded**, `spec-drift`
verifies **412 quotes across both revisions with none drifted**, and the
`2025-11-25` surface is still byte-identical to `Registry::builtin_2025_11_25()`.

| | |
|---|---|
| Pages entered | **15 of 15** |
| `2026-07-28` registry | 272 entries — 124 judged, 0 unsupported, 148 excluded |
| Implemented checks | 119 (48 shipped + 71 for this revision) |
| Lib tests | 247, up from 145 |
| Draft corpus | 2 conforming sessions + 72 violation traces, each byte-pinned |
| `PLANNED` ledger | empty — nothing names a check that does not exist |
| Mutation gate | **270 mutants, 263 caught, 0 missed** (7 unviable) over this pass's diff |

### First: upstream had not moved

The `2026-07-28` tree in `modelcontextprotocol/modelcontextprotocol` has not
been touched since the revision was published; the last commit under
`docs/specification/2026-07-28/` is dated 2026-07-28. Every quote already in the
registry still verified before any new work started. `rmcp` is current at 3.1.2.
The dependency floor is unchanged.

### The corpus contract was weaker than it was documented to be

`corpus/draft/` was described as being "held to the same contract as the
`2025-11-25` one" and was not. Violation traces there were covered only by
`corpus_falsifies_every_check` — "*some* trace kills each check" — which cannot
see a finding that has drifted onto a neighbouring requirement, and their
reports were not pinned at all. That is precisely the defect class the
2026-08-08 pass found by hand when splitting
`transport.header-value-encoding`.

`draft_violation_traces_fail_and_match_goldens` now applies the shipped
corpus's name-attribution assertion and byte-pins every report, with goldens in
`corpus/golden/draft/`. All 33 traces that existed at the time passed unchanged
— the attribution was already right, it just was not enforced.

### Six defects found, five of which were live

1. **Five capability checks would have reported vacuous passes.** Every feature
   page states "Servers that support X MUST declare the X capability", and the
   `2025-11-25` checks for it resolve declarations through
   `support::server_capability`, which abstains unless the trace carries an
   `initialize` **result**. This revision has no `initialize`. Reused as-is, each
   would have inspected nothing and reported `pass` — the TRAN-071 failure mode,
   five times over, on the one clause every feature page repeats.
   `checks/draft/capabilities.rs` reads the `server/discover` result instead.
2. **`transport.unsupported-version-error` bundled an HTTP status into a rule
   stated without one.** TRAN-074 says "400 Bad Request *and* an
   `UnsupportedProtocolVersionError` listing its supported versions"; VERS-001
   states the same rule with no status. One check covering both would have
   reported a wrong status against a clause that never mentions statuses. Split
   into `transport.unsupported-version-{error,status}`.
3. **Splitting it exposed a rule no trace had ever falsified.** The HTTP-400
   half had been riding the kills of the rules it was bundled with;
   `corpus_falsifies_every_check` could not tell the difference. It now has its
   own trace.
4. **`meta.missing-required-field-rejected` reported a conforming server.**
   BASE-031 requires `-32602` for a request missing `_meta` fields, and a legacy
   `initialize` reaching a modern server is missing them by definition — but
   `basic/versioning`'s compatibility matrix states that for exactly that
   exchange "the exact code is implementation-defined". The check was failing
   every cross-era capture. `initialize` is now outside the rule, by method.
5. **`transport.client-no-responses` filtered on Streamable HTTP** and would
   have been inert for the stdio clause that states the same rule. The filter is
   gone: the revision removed server-initiated requests, so there is nothing on
   any binding for a client response to answer.
6. **`transport.no-messages-after-cancellation` anchors on a transport close**,
   which is HTTP's cancellation signal. stdio signals with
   `notifications/cancelled`, so a separate check reads that instead of the
   existing one being pointed at a clause it cannot see.

Two invariants earned their keep without being changed: the good-session test
caught both conforming sessions serving tools and resources without declaring
them once the capability clauses landed, and caught a `resources/read` answering
with an empty `contents` array once RES-022 did.

### Where the 148 exclusions concentrate, and why

They are not spread evenly, and the clustering is the argument for their
honesty. `requestState` is opaque *by design*, so everything MRTR asks a server
to do with it — integrity-protect, validate, put a principal and a TTL inside,
consume once — is invisible to a recording carrying only the blob (10
exclusions). Caching is mostly about what a *client* does with a hint, and a
cache hit is exactly the case where nothing reaches the wire (14). Elapsed time
is not available to checks (LIFE-015), which takes every freshness, rate-limit,
jitter and "promptly" clause. Host-OS actions — `stderr`, process exit, stream
closure — are outside the trace vocabulary (LIFE-014). And a family of clauses
requires classifying the *meaning* of arbitrary JSON — credentials, PII,
"sensitive" parameters, "relevant" context — which `mcp-conformance-core` has no
engine for, the same boundary BASE-073 draws for JSON Schema.

Two exclusions are worth singling out because they look checkable and are not:
`inputRequests` keys "MUST be unique" (MRTR-005) is destroyed at parse time — a
duplicate JSON key is collapsed before any check sees it — and TRAN-129 forbids
keying a fallback to one specific error code, which is a property of the
client's *decision function*: a session exercises one code and one outcome, and
no number of sessions settles what it would have done with another.

### Where a judgement was made rather than a rule mechanically applied

Four checks are narrower than a literal reading, each because a literal reading
would report conforming implementations. Each says so in its own doc comment:

- `meta.missing-required-field-rejected` exempts `initialize` (defect 4 above).
- `caching.hints-on-cacheable-results` exempts results produced by an MRTR
  retry: CACH-003 forbids caching them, and the page's own words for interim
  results — "not cacheable and carry no caching hints" — give the principle.
  Without it, CACH-001 and CACH-003 would contradict each other.
- `discover.dual-era-probe-first` fires only when the session witnesses *both*
  eras; a legacy-only client does not match the clause's antecedent, and judging
  it would report every legacy capture as a client defect.
- `mrtr.*` treats a request carrying `inputResponses` or `requestState` as the
  only kind of retry, so an ordinary follow-up request is never judged as a
  half-finished one.

Two checks report with a stated assumption rather than abstaining, both at
SHOULD (warn) level: `mrtr.missing-input-reasked` reads an error answering an
incomplete retry as evidence the missing input was necessary, and
`pagination.invalid-cursor-rejected` treats "never issued in this session" as
the witness for "invalid".

### The corpus is no longer purely authored — and why that mattered

An authored corpus can only ever confirm its author's reading of the
specification. A check that is wrong in the same way the author is wrong passes
its unit tests *and* its corpus, and neither notices. For a tool whose output is
a conformance claim about someone else's implementation, that is the weakest
point in the whole design, and it was initially filed here as an acceptable
limitation to be closed by roadmap M3. That was wrong: the blocker was not
"no implementation exists", it was a defect in our own capture path.

**The tap could not record this revision at all.** It keyed every exchange on
`Mcp-Session-Id` and returned early without one — and `2026-07-28` removes the
session concept outright (SEP-2575), so *every* exchange of the revision took
that branch. It failed silently: an empty trace directory, indistinguishable
from a server nobody talked to. `cargo xtask draft-readiness` had been driving
the official suite's `2026-07-28` scenarios against a tapped server and
discarding every byte; the task's own comment described its tap as
"irrelevant here", which read as a design choice and was a symptom.

With that fixed, the independent cross-check exists:
`corpus/draft/captured/official-suite-2026-07-28-scenarios.jsonl` is the
official suite `0.2.0-alpha.9` driving its `2026-07-28` scenario set, 91 events
over 22 POST exchanges, recorded off the wire. Judged by the registry it reports
**121 pass, 2 fail, 1 warn, 148 excluded**, and every finding was checked
against the recorded bytes:

| Finding | Verified as |
|---|---|
| TRAN-058 | Real — all 22 POSTs carry no `Mcp-Method`. A defect in the *official suite's client*, not ours. |
| TRAN-068 | Real — all 22 SSE responses lack `X-Accel-Buffering: no`. |
| CACH-001 | Real — the `complete` results carry no `ttlMs`. |

The last two are our `2025-11-25` server held to a revision it does not
implement, which is the correct answer. **No false positives**, and 121
requirements passed on traffic nobody in this repository authored.

`captured_traces_match_goldens` asserts no verdict — a real implementation is
whatever it is — and byte-pins the report instead, so a check that starts
misfiring on real traffic moves the golden.

### What is *not* done

- **The captured half is one recording, and only its client is independent.**
  The server on the other end is ours and implements `2025-11-25`, so the
  capture exercises the checks against *non-conformant* server behaviour. The
  server-side clauses that pass on it largely pass by abstention rather than by
  observing correct behaviour.

  What it would take to close that, verified against rmcp 3.1.2's source rather
  than estimated:

  | Piece | Cost |
  |---|---|
  | Stateless routing (no sessions) | Configuration: `StreamableHttpServerConfig::legacy_session_mode = false` |
  | Per-request `_meta` enforcement | Configuration: `stateless_protocol_metadata_required = true` |
  | `server/discover` | Free — rmcp's `ServerHandler` has a default `discover()` returning `DiscoverResult::from_server_info` |
  | `resultType` on every result | Free — rmcp models it and strips it only for legacy peers (`strip_result_type_for_legacy_peer`) |
  | Advertising only `2026-07-28` | One `supported_protocol_versions` override |
  | **Caching hints (`ttlMs`, `cacheScope`) on the six cacheable operations** | **Not free** — handler-level, per result type |
  | **MRTR, `subscriptions/listen` behaviour** | **Not free** — rmcp ships the types, the server still has to use them |

  So a *substantially* conforming server is close to configuration, and a
  *fully* conforming one is handler work across the feature surface. Both are
  ordinary work rather than a blocked dependency — which is the correction that
  matters, because the previous entry here filed the whole thing as gated on a
  milestone.
- **The feature is still off by default.** Nothing about `2026-07-28` reaches a
  default build, which is deliberate while the revision is new.
- **Expansion candidates stay out of scope**, unchanged and for the reasons
  `sources.json` records: full OAuth, the client-feature pages, and
  `basic/patterns/{cancellation,progress,index}`.

### For the next session

The extraction loop is finished and the corpus now has an independent half. The
next work is to make that half *conformant* as well as real: stand up a
`2026-07-28` server on rmcp 3.1.2's stateless surface, drive it, and capture a
session where the pass paths are exercised by an implementation nobody here
wrote. That is the remaining way to raise confidence in these 124 checks.

The two hazards from the last handover both recurred, and a third joins them:

- **A reused `2025-11-25` check can be vacuous here.** It happened five more
  times, all on the capability clause. Read a candidate to the bottom, and
  specifically look for `support::server_capability` and anything touching
  `context.initialize()`.
- **A check that bundles adjacent rules makes every requirement naming it
  imprecise.** It happened once more, on TRAN-074/VERS-001.
- **The equivalent-mutant shape recurs.** Three of the five mutants this pass
  missed were `<` versus `<=` or `>` versus `>=` between two events that can
  never share a `seq` — a server result and a client request, a notification and
  a response. The construction keeps arising because "the most recent X before
  Y" is a natural way to write these checks, and it is always better expressed
  as one ordered pass that flips state at X. That is the third and fourth time
  this repository has hit it; expect a fifth.

- **New:** run `cargo xtask ci`, not `cargo clippy --all-features`. The repo
  lints in three feature modes, and a `#[cfg]`-gated item that becomes unused
  with the feature *off* is invisible to the all-features run — which is how an
  unused macro and an unused re-export reached the end of this session's work.

## Addendum 8 (2026-08-17): the conforming server exists, and it is captured

The previous addendum closed with the remaining work stated as a cost table:
stand up a `2026-07-28` server on rmcp 3.1.2's stateless surface, drive it,
capture it. That is done, and doing it found a second capture-fidelity defect
that had been reporting conforming implementations as violating ones.

### What was built

`mcp-everything-server --protocol-version 2026-07-28`. The revision is chosen
when the server is constructed (`ServedRevision`), not per request:

| Piece | How |
|---|---|
| Stateless routing | `legacy_session_mode = false` and `stateless_protocol_metadata_required = true`, set together — either alone is a shape the specification does not describe |
| `server/discover` | rmcp's default `discover()`, plus caching hints |
| Version advertisement | `supported_protocol_versions` narrowed to `2026-07-28`; the legacy mode keeps rmcp's full list verbatim |
| `initialize` | **Refused** with `-32022` carrying `data.supported` (see below) |
| Caching hints | `ttlMs` + `cacheScope` on the four cacheable results this crate builds; rmcp's handler macros already emit their own on `tools/list` and `prompts/list` |

The cost table was accurate except in one place it could not have anticipated.
rmcp's default `initialize` does not *refuse* an unsupported version — it
negotiates down to the server's own and answers with a **result**. A
stateless-only server would therefore complete a handshake that leads nowhere:
the client believes it is connected, and every subsequent request is rejected
for want of per-request metadata it does not know to send. Overriding
`initialize` to refuse with `-32022` is what VERS-001 requires of a version
refusal and what VERS-008 asks a modern-only server to give a legacy client,
and it is the only piece of this work that was a correctness decision rather
than configuration.

Hints key off the **served** revision, never the negotiated version of the
request in hand. `draft-readiness` drives `2026-07-28` requests at the legacy
server on every run, so keying off the request would have moved a committed
ratchet as a side effect of adding a mode nobody enabled.

### The second capture defect

The first capture of the new server judged **122 pass, 1 fail, 1 warn**. Both
non-passes were ours:

- **TRAN-058** — "client POSTs lack `Mcp-Method`". rmcp's transport *rejects* a
  `2026-07-28` request without that header, before dispatch, with `-32020`. The
  official suite scored 23/23 against this server, so it sent the header on
  every POST.
- **TRAN-068** — "SSE responses lack `X-Accel-Buffering: no`". rmcp sets that
  header on every SSE response it builds (`server_side_http.rs`).

Both were sent; neither was recorded. The tap's allowlist held seven header
names chosen when `2025-11-25` was the only revision it recorded, and four
`2026-07-28` clauses are proved or falsified by headers absent from it. The
allowlist now carries `mcp-method`, `mcp-name`, `x-accel-buffering` and a
`mcp-param-` prefix arm — a prefix because those names are chosen by a tool's
`x-mcp-header` annotation and cannot be enumerated.

This is the same class as the sessionless-drop defect fixed earlier on this
branch, and worse in kind. An empty trace directory is at least visibly empty.
A trace missing a header reads as a complete recording of a violating client,
and it had already been written up in `corpus/README.md` as a finding about the
official suite. **The general lesson: a check is only as honest as the
recording it reads, and the recording is part of the judgment surface.** It
should be reviewed whenever a clause's evidence lives somewhere the tap has not
had to look before.

### What the captures show

`cargo xtask draft-readiness` now runs both server modes and commits both
recordings. Same client, same scenarios, same run; one variable.

| | Legacy server | Stateless server |
|---|---|---|
| Official runner | 23/23 | 23/23 |
| This registry | 123 pass, **1 fail**, 0 warn, 148 excluded | **124 pass, 0 fail, 0 warn**, 148 excluded |

The one finding is CACH-001 against the legacy server — no `ttlMs` on cacheable
results — which is the correct answer for a server held to a revision it does
not implement. The stateless capture is the control that proves the check
reports the server rather than the recording.

**The runner cannot tell the two servers apart.** Its `2026-07-28` scenarios
exercise features, and rmcp answers a per-request-versioned POST whichever
revision the handler advertises, so both score 23/23. The registry here judges
124 clauses of the specification's prose and sees the one place they differ.
That is the clearest evidence yet for why this workspace exists, and it is now
a committed artifact rather than an argument.

### What this closes

The limitation filed at the end of the last session — "the corpus is authored,
not captured; no implementation here serves the stateless surface end to end"
— is closed. Every one of the 124 judged clauses now has a real recording of a
conforming implementation behind it, produced by a client nobody here wrote.

### For the next session

- The `2026-07-28` feature is still off by default, deliberately.
- `json_response` is left at rmcp's default, so stateless responses are SSE.
  Both framings are conformant; the JSON one is one config field away if a
  capture of it is ever wanted.
- The suite pin is still `0.2.0-alpha.9`, now two alphas behind. The
  `suite-0-2-0-stable-pin-bump` deferral governs that move; both legs of the
  ratchet re-measure when it happens.
