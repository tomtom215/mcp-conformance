<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0020: Judge Every Message Against the Revision's Own JSON Schema

**Date:** 2026-10-13
**Status:** Accepted (reverses the "no JSON Schema engine" boundary that
[02-architecture.md](../02-architecture.md) and several registry exclusions relied on;
decision D2 of the [2026-09-24 review](../../reports/project-review-2026-09-24.md))
**Author:** Tom F.

---

## Context

The specification calls its TypeScript schema "the source of truth for all protocol
messages and structures" and publishes a JSON Schema generated from it "for use with
various automated tooling". Most of what that schema constrains has no sentence of its
own in the prose: a result's required members, a member's type, an enumeration's values,
a content block's shape. A registry built one row per RFC 2119 sentence therefore has no
row for any of it, and the validator, which carried no JSON Schema engine, never looked.
A `resources/list` result whose resource has no `uri`, a log at level `chatty`, a content
block of type `txt` — each passed every check (review finding H2).

The official suite closed the same gap on its side with a `wire-schema-valid` check that
validates every message against the vendored schema of the negotiated revision. Two
consequences of not doing so here were measurable: this project's calibration claim had a
blind spot exactly where the suite now looks, and several exclusions (`BASE-017`,
`BASE-073`, `TOOL-041`, `TOOL-045`, `TRAN-082`, `TRAN-137`) and one partial check
(`TOOL-011`/`TOOL-040`, which judged only the presence of `structuredContent`) rested on
the engine's absence rather than on anything about the wire.

## Decision

1. **Vendor the schema.** `mcp-conformance-core/spec-schema/<revision>.schema.json` is the
   specification repository's `schema/<revision>/schema.json`, byte for byte, at a pinned
   commit (`wire_schema::UPSTREAM_COMMIT`), from `main` rather than the release tag
   because `main` carries upstream's post-release corrections. The files are the
   specification's work under its licence, shipped beside it in `spec-schema/LICENSE`;
   `mcp-conformance-core`'s licence becomes `MIT AND Apache-2.0`. Core gains data only —
   no engine.
2. **Validate in the validator.** `mcp-trace-validator` takes `jsonschema` (no default
   features: no remote `$ref` resolution, no network) and judges every message event
   against the definition the suite's dispatch picks: a request or notification by
   `method`, an error by `code`, a result by the method of the request it answers, the
   JSON-RPC envelope otherwise. Format assertion stays off: in 2020-12 `format` is an
   annotation, and asserting `uri` would fail URIs the prose permits.
3. **Report it beside the rows, not as one.** The authority sentence carries no RFC 2119
   keyword, and inventing a `MUST` row for it would misquote the specification. The report
   gains `wire_schema` — outcome, messages checked, one finding per invalid message, the
   pinned schema URL, and the authority quote — which a `fail` fails the verdict through.
   It has no requirement ID and does not count in `totals`; JUnit renders it as one more
   test case and SARIF as the rule `wire-schema`.
4. **Correct the generator, never the file.** Where the generated JSON Schema is stricter
   than `schema.ts`, the validator patches the compiled copy from a recorded erratum
   (`wire_schema::Erratum`: pointer, upstream value, correction, the `schema.ts`
   declaration that decides it). There are two: `ElicitResult.content` values and, at
   `2026-07-28`, `JSONValue`, both generated as `integer` where `schema.ts` says `number`
   (and `JSONValue` dropping `null`). The first was found by our own reference host's
   capture: a float answer to a `NumberSchema` field, which the suite's SEP-1034 scenario
   itself sends. A test fails when an erratum's upstream value no longer matches.
5. **Drift-check it.** `cargo xtask spec-drift` compares each vendored file with `main`
   byte for byte and re-verifies the authority quote, as it does every registry quote.
6. **Retire the exclusions the engine's absence justified.** Each is re-decided on what a
   trace carries, and where a check replaces it, it ships with a passing and a violating
   corpus trace like any other.

## Consequences

### Positive

- Every message is held to the protocol's own structure, matching what the suite checks;
  the first corpus run found nothing new in the conforming traces and confirmed
  `CACH-001` on the suite's capture independently.
- The engine removes the reason, not merely the symptom, for the exclusions in decision 6.
  Re-decided on what a trace carries: `BASE-017`/`BASE-073` (schema validity in the
  declared dialect) and `TOOL-045`/`TRAN-137`/`TRAN-082` (`x-mcp-header` placement,
  found by a keyword-aware walk that never mistakes instance data for a schema) are
  judged; `TOOL-011`/`TOOL-040` judge conformance, not only presence; `TOOL-041` stays
  excluded for a reason that never needed the engine — a client's validation leaves no
  message behind.

### Negative

- Verdicts change: a session with a schema-invalid message that passed now fails. Pre-1.0,
  in the next minor, declared in the changelog's breaking-changes table.
- Binary size: the release `mcp-trace-validator` grows from 2.1 MB to 6.9 MB (measured
  2026-10-13, `x86_64-unknown-linux-gnu`, the workspace release profile), mostly the
  engine and its regex stack, plus about 350 KB of embedded schemas.
- New transitive dependencies (`jsonschema`, `referencing`, `fancy-regex`, `ahash` and
  others), each through `cargo deny`'s licence and advisory policy.
- Runtime: no measurable change on a 40,000-message `2026-07-28` trace — 0.82–1.10 s before,
  0.80–1.00 s after, five runs each, release builds, one 4-vCPU Xeon at 2.10 GHz; peak RSS
  140 MiB before and 160 MiB after (one run each). The schema compiles once per process.

### Follow-up (2026-10-13): agreement with the suite's check

The agreement step now compares this member with the suite's `wire-schema-valid`
message by message over the tapped sessions, with a ledger of its own
(`conformance/wire-schema-divergences.json`, see
[03-conformance-strategy.md](../03-conformance-strategy.md) §Calibration). The suite's
bundled schema copies are not this decision's vendored files: it applies its own
`NumberSchema` correction and none of the errata above, asserts `format`, and its
`2026-07-28` copy predates the pinned commit. A divergence from those differences is
recorded as class `schema-pin-skew`.

### Neutral

- The schema is an independent second reading of the same session; where it and a clause
  check both fire (`CACH-001` and a missing `ttlMs`), the report says so twice, which is
  the honest accounting of two rules broken by one message.
