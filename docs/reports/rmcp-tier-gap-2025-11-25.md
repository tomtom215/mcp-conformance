<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# rmcp tier-gap report — `2025-11-25`

**What this is.** A reproducible measurement of the official Rust SDK
([`rmcp`](https://github.com/modelcontextprotocol/rust-sdk))'s server-side
conformance against the official suite, the requirement-level reading of where
it falls short, and a concrete close-the-gap checklist. It is one of this
project's stewardship artifacts ([roadmap M5](../plan/06-roadmap.md);
[strategy §Supporting rmcp's path to Tier 1](../plan/03-conformance-strategy.md));
the method is fully reproducible from the commands in the last section.

**This is not a verdict handed down.** The official suite is the authority on
conformance ([strategy §Position](../plan/03-conformance-strategy.md)); the
numbers below are *its* numbers, captured by running it, with our registry used
only to read each failure back to a spec clause. Every claim has a command that
reproduces it.

## Measurement

| Field | Value |
|-------|-------|
| Subject | `modelcontextprotocol/rust-sdk` `conformance-server`, head `266f870` |
| Suite | `@modelcontextprotocol/conformance@0.1.16` (this repo's pin) |
| Spec revision | `2025-11-25` |
| Transport | streamable HTTP, loopback |
| Date | 2026-06-13 |
| **Server scenarios** | **38 passed, 2 failed (38/40)** |

Failing scenarios:

| Scenario | Result | Failure (per [register 3.10](../plan/01-ecosystem-context.md), checks.json retained) |
|----------|--------|--------|
| `prompts-get-with-args` | 0/1 | template arguments not substituted into the returned messages ("arg1 not substituted in prompt; arg2 not substituted in prompt") |
| `elicitation-sep1330-enums` | 4/5 | "Missing or invalid enumNames array for legacy titled enum" (field `legacyEnum`) |

This reproduces, at the current head, the 38/40 measured at head `52e731b` on
2026-06-11 ([register 3.10](../plan/01-ecosystem-context.md)) — the gap is
stable, not a transient. **Trajectory:** rmcp's own 2026-02-25 in-repo
assessment (`conformance/results/2026-02-25-rust-sdk-assessment.md`, "Tier 3")
listed *five* failing server scenarios — `prompts-get-with-args`,
`prompts-get-embedded-resource`, `elicitation-sep1330-enums`,
`elicitation-sep1034-defaults`, `dns-rebinding-protection`. Three of those five
now pass (`prompts-get-embedded-resource` ✓, `elicitation-sep1034-defaults` 5/5,
`dns-rebinding-protection` 2/2 — the CVE-2026-42559 class is closed). Two
remain.

Captured suite output (excerpt; the other 36 scenarios pass and are elided):

```text
✓ elicitation-sep1034-defaults: 5 passed, 0 failed
✓ server-sse-multiple-streams: 2 passed, 0 failed
✗ elicitation-sep1330-enums: 4 passed, 1 failed
...
✓ prompts-get-simple: 1 passed, 0 failed
✗ prompts-get-with-args: 0 passed, 1 failed
✓ prompts-get-embedded-resource: 1 passed, 0 failed
✓ dns-rebinding-protection: 2 passed, 0 failed

Total: 38 passed, 2 failed
```

## A note on `tier-check` (read before trusting a tier number)

The suite ships a `tier-check` subcommand that aggregates conformance, label
taxonomy, and issue-triage speed into a SEP-1730 tier verdict. Two things make
the `server` subcommand above — not `tier-check` — the trustworthy conformance
signal:

1. **`tier-check` is gated on GitHub auth and a `--repo`.** It bails before the
   conformance leg without a token (`GitHub token required … --token <token>`),
   because its label and triage legs hit the GitHub API. The conformance leg it
   would then run is the *same* scenario set the `server` subcommand runs
   directly.
2. **`tier-check`'s conformance counter has a known upstream bug**
   ([register 2.13](../plan/01-ecosystem-context.md); conformance
   [#182](https://github.com/modelcontextprotocol/conformance/issues/182)):
   it "reports 0/30 server conformance despite all tests passing." So even with
   a token, its raw tier number understates reality.

The honest conformance figure is therefore the `server` subcommand's **38/40**,
not a `tier-check` aggregate. A reader with a GitHub token can still run the full
`tier-check --repo modelcontextprotocol/rust-sdk` for the label/triage legs; this
report deliberately reports the reliable number instead of a buggy one.

## Requirement-level reading (our validator's lens)

The point of a requirement-level tool is to say *which clause* a failure is
about, not just *which scenario*. Reading the two failures back through the
[`2025-11-25` registry](../plan/01-ecosystem-context.md) gives a result that is
itself informative:

- **`prompts-get-with-args` — below the registry's normative floor.** The
  registry has no MUST for argument *substitution*. The `2025-11-25` text states
  substitution descriptively (in the prompt-template schema prose), not with an
  RFC 2119 keyword, and per the [extraction policy](../plan/03-conformance-strategy.md)
  schema-doc constraints without a keyword are not registry entries. The nearest
  normative neighbours are `PROM-007` (SHOULD return a `-32602` for a missing
  required argument) and `PROM-008` (SHOULD validate prompt arguments before
  processing) — both about *rejecting* bad input, neither about *substituting*
  good input. So this scenario tests real, useful behavior that sits **just
  below** the spec's normative floor: a recorded trace would show the
  un-substituted `arg1`/`arg2`, but no `MUST`/`SHOULD` clause to attribute it
  to. (That gap between "what the suite checks" and "what the spec mandates" is
  worth a conformance-repo note in its own right.)
- **`elicitation-sep1330-enums` — a schema-fidelity bug, not a clause
  violation.** There is no `2025-11-25` registry requirement for elicitation
  `enumNames`; it is a SEP-1330 schema detail. The failure is an rmcp
  *serialization* bug, already mechanism-verified and filed:
  [register 3.8](../plan/01-ecosystem-context.md) /
  [rust-sdk#903](https://github.com/modelcontextprotocol/rust-sdk/issues/903)
  (dossier in [#10](https://github.com/tomtom215/mcp-conformance/issues/10)) —
  the untagged `EnumSchema` tries the untitled variant before the legacy one and
  the untitled struct does not reject unknown fields, so a legacy titled enum
  matches it and `enumNames` is silently dropped on round-trip. Our own server
  passes this scenario because it constructs the typed `Legacy` variant
  directly; rmcp's `conformance-server` builds the schema via
  `serde_json::from_value` and hits the bug.

The reading that matters for adopters: **neither failure is a `2025-11-25`
normative-clause violation** the validator would flag as a MUST finding. One is
a sub-keyword behavioral expectation; the other is a serialization bug in a SEP
schema. That is good news for rmcp's conformance *posture* and bad news for its
suite *score* — and it tells you exactly where to push.

## Close-the-gap checklist

1. **`elicitation-sep1330-enums`** — fix the `EnumSchema` deserialization
   ordering so a legacy titled enum keeps `enumNames`. Mechanism, 20-line repro,
   and a regression test are filed as
   [rust-sdk#903](https://github.com/modelcontextprotocol/rust-sdk/issues/903);
   a `skip_serializing_if` on `enum_names` closes the latent `"enumNames": null`
   sibling. Closing #903 closes this scenario.
2. **`prompts-get-with-args`** — substitute declared prompt arguments into the
   messages returned by the `conformance-server`'s `prompts/get` handler. This is
   a fix in rmcp's *conformance fixture*, not the SDK core; a small,
   obviously-correct conformance-repo or rust-sdk PR.

Closing both takes rmcp's server scenarios to 40/40 at this suite pin — the
first of SEP-1730's Tier-1 criteria ([register 2.5](../plan/01-ecosystem-context.md)).
The remaining Tier-1 criteria (RC-window feature cadence, triage SLAs, P0
resolution, documented releasing) are maintainer-process commitments outside an
external contributor's reach.

## Reproducible method

```sh
# 1. Build rmcp's conformance server (records the head you measured).
git clone --depth 1 https://github.com/modelcontextprotocol/rust-sdk /tmp/rmcp
git -C /tmp/rmcp rev-parse --short HEAD            # the subject SHA
cargo build --manifest-path /tmp/rmcp/Cargo.toml -p mcp-conformance --bin conformance-server

# 2. Serve it on loopback.
PORT=8765 /tmp/rmcp/target/debug/conformance-server &

# 3. Run the pinned official suite's server scenarios.
npx -y @modelcontextprotocol/conformance@0.1.16 server \
  --url http://127.0.0.1:8765/mcp --spec-version 2025-11-25

# (Optional, needs a GitHub token) the full SEP-1730 tier assessment:
#   npx -y @modelcontextprotocol/conformance@0.1.16 tier-check \
#     --repo modelcontextprotocol/rust-sdk \
#     --conformance-server-url http://127.0.0.1:8765/mcp \
#     --spec-version 2025-11-25 --output markdown
# Note conformance#182: its conformance counter under-reports — prefer the
# `server` subcommand's figure above.
```

Refresh this report when the suite pin moves (this repo's
[suite-version policy](../plan/03-conformance-strategy.md)) or when #903 lands.
