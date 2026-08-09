<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0011: The rmcp Pin Holds at 1.7 Until Both the Spec Text and rmcp 3.0 Are Final

**Date:** 2026-07-27
**Status:** Accepted
**Author:** Tom F.

---

## Context

The workspace pins `rmcp 1.7.0` and passes the official suite's `2025-11-25`
server scenarios 40/40 — the standing hard gate from M2 onward, and the
calibration anchor the validator's agreement check replays against.

The SDK line has moved a long way underneath that pin
([register 3.1](../01-ecosystem-context.md)). Since 2026-06-23 it has gone
`1.7.0 → 1.8.0 → 2.0.0 → 2.1.0 → 2.2.0 → 3.0.0-beta.1 → 3.0.0-beta.2`, with
`2.2.0` the latest *stable* and the `3.0.0-beta` line implementing the
`2026-07-28` revision end to end: stateless lifecycle, `server/discover`,
the tasks extension, MRTR, the TTL-honoring cache. `3.0.0-beta.2` is also the
first rmcp release ever to declare an MSRV — 1.88, exactly the floor
[ADR-0008](0008-msrv-1.88.md) measured empirically.

Three facts constrain what we do about that:

1. **The `2026-07-28` text has not shipped.** `/specification/2026-07-28`
   returns 404 and the spec repo carries no such directory
   ([register 1.5f](../01-ecosystem-context.md)). The draft that rmcp 3.0-beta
   implements has itself moved twice under us — four inventory items added on
   2026-06-27, one Major reworded on 2026-07-19
   ([1.5d](../01-ecosystem-context.md), [1.5e](../01-ecosystem-context.md)).
2. **An upgrade here is never a hygiene bump.** The 2026-07-19 attempt at the
   *smallest possible* move (1.7.0 → 1.8.0, for the merged `enumNames` fix)
   was reverted: 1.8.0 bundles SEP-2577's forward-deprecation of
   Roots/Sampling/Logging — all active and required for our `2025-11-25`
   surface — and changes tool-argument-deserialization failures from an MCP
   protocol error to a tool-error result, which is conformance-relevant and
   unvalidatable without the npx suite ([register 3.8](../01-ecosystem-context.md)).
   Dependabot's [#28](https://github.com/tomtom215/mcp-conformance/pull/28)
   proposed 1.7 → 2.2 and produced ~70 compile errors.
3. **Nothing in any open DoD is blocked on the SDK.** M2.5's remaining lines
   are registry entries and `corpus/draft/` pairs extracted from the *final*
   text; M5's last line is the `draft-2026-07-28` feature-gate drop. None of
   them moves because rmcp moves. The everything server passing draft
   scenarios is not a DoD line anywhere — it is a measurement, and
   `cargo xtask draft-readiness` now takes it (1 passing / 20 failing /
   1 informational, [register 1.5g](../01-ecosystem-context.md)).

## Decision

**Hold the pin at `rmcp 1.7.0`.** Do not adopt `2.2.0` as an interim step, and
do not adopt any `3.0.0-beta`.

The next rmcp upgrade is a single deliberate move, taken when **both**
conditions hold:

- the `2026-07-28` specification text has shipped, and
- rmcp has cut a **stable** `3.0.x` (not a pre-release).

That upgrade is executed as one piece of work together with M2.5's registry
extraction — the same commit range re-runs `cargo xtask conformance` for the
40/40 gate and re-blesses `conformance/draft-readiness.json`, so the migration
is measured rather than asserted.

Compliance is checkable three ways: the workspace manifest's `rmcp` requirement,
the `rmcp-macros` ceiling shim in `crates/mcp-everything-server/Cargo.toml`
(which mechanically prevents `cargo update` from performing the upgrade —
rmcp 1.8.0 requires `rmcp-macros ^1.8.0`, which the bound forbids), and the
`adopt-rmcp-enumnames-fix` deferral row, whose `review_by` forces a
re-decision on **2026-09-01** whether or not upstream has moved.

## Consequences

### Positive

- The 40/40 conformance gate and the agreement check keep a stable floor
  through the window where the spec, the SDK, and the suite are all moving at
  once. Exactly one of those three variables changes at a time.
- One migration instead of two. Adopting 2.2.0 now would mean re-validating
  the whole conformance surface twice to reach the same destination.
- Published crates keep depending only on released versions. `mcp-everything-server`
  and `mcp-reference-host` are on crates.io; a `3.0.0-beta.2` requirement would
  drag every downstream consumer onto a pre-release, since Cargo's pre-release
  semantics do not let a caller opt back out.
- The decision cannot rot quietly: the deferral date fires, and the ceiling
  shim fails resolution loudly if someone bumps `rmcp` without moving it.

### Negative

- **We ship on a knowingly old SDK.** rmcp 1.7.0 is five releases and one
  major behind stable. Any reviewer comparing the manifest to crates.io sees
  that gap; this ADR is the answer, but the gap is real.
- **We forgo the merged `enumNames` fix** ([rust-sdk#905](https://github.com/modelcontextprotocol/rust-sdk/pull/905))
  for now. Acceptable because it does not bite our path: we construct `Legacy`
  schemas directly and serialize correctly, while the loss occurs on
  `serde_json::from_value` construction ([register 3.8](../01-ecosystem-context.md)).
  `tests/roundtrip.rs` keeps pinning the current behavior, so the day we adopt a
  fixed rmcp the test fails loudly and forces the update.
- **The eventual upgrade is larger for having waited**, accumulating 1.8's two
  behavior changes, all of 2.x, and 3.x's breaking changes into one review.
  Accepted deliberately: that review happens once, against a final spec, with
  the suite and the ratchet both able to judge the result.
- **A security advisory against `rmcp 1.7.0` would override this immediately.**
  None exists today — `cargo audit` is green and CVE-2026-42559 scoped to
  `rmcp < 1.4.0` ([register 4.3](../01-ecosystem-context.md)) — but this
  decision is not a commitment to stay put through a vulnerability.

## Alternatives considered

### Adopt `3.0.0-beta.2` now, to develop against the new revision early

Rejected on two independent grounds, either sufficient. It is a pre-release and
we publish, so the cost lands on downstream consumers rather than on us. And it
implements a draft, not a specification: the text it targets has not shipped and
has already changed twice during the window, so early adoption buys migration
work we would redo. A supporting signal that the ecosystem agrees the revision
is not yet live — rmcp 3.0.0-beta.2's own `ProtocolVersion::LATEST` is still
`V_2025_11_25`.

### Adopt `2.2.0` now as an interim, then `3.0` later

Rejected: two migrations and two full conformance re-validations to reach one
destination. The only thing 2.2.0 uniquely buys is the `enumNames` fix, which
our construction path does not hit.

### Bump only `rmcp-macros` to escape the resolution defect

Rejected — it is backwards. The broken pair is *newer macros against older
library* ([register 3.13](../01-ecosystem-context.md)); raising the macro crate
is the failure mode, not the fix. The ceiling shim pins it down to the library's
minor instead.

### Let Dependabot decide, merging SDK bumps as they arrive

Rejected as the status quo that produced the problem: grouping made
[#28](https://github.com/tomtom215/mcp-conformance/pull/28) unmergeable and held
six routine bumps hostage to one major. `rmcp` now has its own Dependabot group
so the proposal stays *visible* as milestone input, and this ADR — not the PR
queue — decides when it lands.

## Amendment (2026-07-28): condition one is met, condition two is not

The `2026-07-28` specification text **shipped on its scheduled date**
([register 1.5h](../01-ecosystem-context.md)), which falsifies Context point 1
above as of this date. Read that point as history: it recorded the state on
2026-07-27, and the decision it supports is unaffected.

**The pin still holds, on the second condition alone.** This ADR requires
*both* the text and a stable `3.0.x`. Checked 2026-07-28: rmcp's newest release
is `3.0.0-beta.4` (published the same day) and `max_stable_version` is still
`2.2.0`. The `3.0.x` line remains pre-release, so every argument in
§Alternatives against adopting a beta — we publish, and Cargo's pre-release
semantics do not let downstream opt back out — applies unchanged. The
`rmcp-macros` ceiling shim and the `adopt-rmcp-enumnames-fix` deferral
(`review_by` 2026-09-01) both stay in force.

One supporting signal from §Alternatives has now expired and should not be
re-cited: `3.0.0-beta.2`'s `ProtocolVersion::LATEST` still reading
`V_2025_11_25` was offered as evidence that *the ecosystem agrees the revision
is not yet live*. That reading is dead — the revision is live. If the beta line
still lags the released text, that is now an argument about rmcp's readiness,
not about the spec's.

**What this does not gate.** Context point 3 said nothing in any open DoD is
blocked on the SDK, and that remains true and now load-bearing: M2.5's open
lines are registry entries and `corpus/draft/` pairs extracted from the final
text, which is exactly what shipped. The Decision's phrase "executed as one
piece of work together with M2.5's registry extraction" scopes *when the
upgrade lands*, not when extraction may start — extraction proceeds now against
the released text, and the upgrade joins it when a stable `3.0.x` exists. Read
the other way, a beta SDK would hold the milestone hostage indefinitely, which
no part of this ADR argues for.

## Amendment (2026-08-06): both conditions are met — the pin is released

`rmcp 3.0.0` — a **stable** release, not a pre-release — published on
2026-07-28, the same day the specification text shipped. The line has since
moved to `3.0.1` (2026-07-29), `3.1.0` (2026-07-31) and `3.1.1` (2026-08-05)
([register 3.14](../01-ecosystem-context.md)). Both conditions this ADR set now
hold, so **the hold expires by its own terms.** No further decision is needed
to upgrade; what remains is executing it as the single measured move the
Decision describes.

**Target `3.1.1`, not `3.0.0`.** The Decision says one deliberate move rather
than a sequence, and by the time both conditions were checkable the `3.1` minor
already existed. Adopting `3.0.0` now would schedule a second migration to
reach where the ecosystem already is.

**What makes this safe to execute rather than merely permitted.** The upgrade's
sharpest risk was never compile breakage — it was losing the calibration
anchor. The 40/40 `2025-11-25` conformance gate is what judges whether the
migration worked, and if `3.x` had dropped `2025-11-25` the upgrade would have
retired that gate *before* the registry could describe `2026-07-28`, leaving a
window with nothing to measure against. It did not: `3.1.1` supports all three
revisions and `ProtocolVersion::LATEST` is still `V_2025_11_25`, with
`2026-07-28` reachable via explicit opt-in (register 3.14, read first-hand in
the crate source). The gate survives the upgrade, so exactly one variable moves
at a time — the sequencing §Consequences claimed as this decision's main
benefit, now actually available.

**What does not change.** The `rmcp-macros` ceiling shim stays: `3.1.1` still
caret-pins its companion at `^3.1.1` (register 3.13 is unfixed upstream), so
the bound moves to the `3.1` minor rather than being deleted. The suite pin is
untouched and unrelated — the official runner's stable `latest` is still
`0.1.16`, so `suite-0-2-0-stable-pin-bump` is not released by this and neither
is M5's feature-gate drop, which additionally requires the `2026-07-28`
scenarios to stabilize.

**Status.** This ADR is **superseded in effect** on its operative clause: the
pin no longer holds. It stays Accepted as the record of *why* it held from
2026-07-27 to 2026-08-06, and of the three-variable sequencing argument that
still governs how the upgrade is executed and validated.
