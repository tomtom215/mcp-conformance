<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Stability policy

What you can rely on across releases of the five crates —
`mcp-conformance-core`, `mcp-trace-validator`, `mcp-trace-capture`,
`mcp-everything-server` and `mcp-reference-host` — and how a change to any of it
is made. [RELEASING.md](RELEASING.md) is the release runbook;
[CHANGELOG.md](CHANGELOG.md) records every change this policy covers.

## Versions

All five crates share one version and release together, under
[Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html). Before 1.0:

- a **patch** release (`0.6.0` → `0.6.1`) changes no public API and no
  behaviour described below — `0.5.1`, the only one so far, changed one locked
  dependency version;
- a **minor** release (`0.6` → `0.7`) may break any of it, and its changelog
  entry says what breaks and how to migrate, starting with a table of every
  breaking change ([0.6.0](CHANGELOG.md) is the model).

Fixes to wrong verdicts change behaviour, so they ship in minor releases.

## What counts as a breaking change

A conformance tool's output is part of its contract, not only its Rust API. Each
of these is a breaking change:

| Surface | Stable | Breaking |
|---------|--------|----------|
| Rust API | Every public item of the five crates; `cargo-semver-checks` runs against the crates.io baseline before each release | Removing or changing an item; adding a variant or field to a type that is not `#[non_exhaustive]` (protocol-facing types are) |
| Verdicts | The outcome of every requirement, and of the wire schema, for a given trace | A trace that passed now failing, or the reverse — a new check, a check fixed, a clause moving between checked and excluded, a refreshed vendored schema or a changed erratum ([ADR-0020](docs/plan/decisions/0020-wire-schema-validation.md)) |
| Requirement IDs | `BASE-003` names one clause, forever; an ID is never reused for another | Never done. A row found to bundle several clauses is split: its ID stays with one of them, the others get new IDs, and the changelog names each (`BASE-055`/`BASE-083` in 0.6.0) |
| Exit codes | `mcp-trace-validator`: `0` pass, `1` a clause failed (or warned, under `--strict`), `2` a bad invocation, nothing judgeable or output not written, `3` a malformed trace. `mcp-trace-capture`: its README's table | Any code changing meaning |
| CLI | Subcommands, flags and their defaults; the first line of `--version` | Removing or renaming a flag, or changing a default. A retired flag is first kept, accepted and hidden (as `--quiet` has been since 0.6.0) |
| JSON report | The shape [`report.schema.json`](crates/mcp-trace-validator/schema/report.schema.json) describes | Removing or retyping a member, or a new value of an enumerated member (as `incomplete` joined `verdict`). **Adding a member is not breaking: accept members you do not know** |
| JUnit report | Suite and test-case naming (a requirement's case is `ID (LEVEL)`; the schema's is `wire schema`); failures carry the finding in `message` | Changing either |
| SARIF log | `ruleId` is the requirement ID, or `wire-schema` for a message invalid against the revision's JSON Schema; findings are keyed by `partialFingerprints["mcpConformanceFinding/v2"]` | Changing the rule key, or the fingerprint (its key's version suffix moves with it, so alerts reopen once rather than silently) |
| Trace format | What [`trace-event.schema.json`](crates/mcp-conformance-core/schema/trace-event.schema.json) accepts: a trace that validates against a release's schema is read by that release's validator | Rejecting a trace an earlier release accepted |
| MSRV | 1.88 ([ADR-0008](docs/plan/decisions/0008-msrv-1.88.md)) | Raising it — done only in a minor release, with a changelog entry |
| Cargo features | Each feature's meaning; a retired feature stays as a no-op (`draft-2026-07-28` since 0.6.0) | Removing a feature or changing what a default feature enables |

Both schemas' `$id`s name the `main` branch, which moves. To pin the schema of the
release you run, use the tag's copy:
`https://raw.githubusercontent.com/tomtom215/mcp-conformance/vX.Y.Z/crates/mcp-trace-validator/schema/report.schema.json`.

## How a breaking change is made

1. **Deprecate first** where the Rust API allows it: the old item gains
   `#[deprecated]` naming its replacement, stays for at least one minor release,
   and its removal is announced in the release that deprecates it (`context::draft`:
   deprecated in 0.6.0, removed in the release after).
2. **Declare it**: the changelog's entry for the release names every breaking
   change and its migration; a verdict change names the clause and the traces it
   affects.
3. **Check it**: `cargo-semver-checks` must agree that the version bump admits the
   API change, so a declared behavioural break is never confused with an
   undeclared API break.

## Specification revisions

The validator judges the protocol revisions its registry describes — today
`2025-11-25` and `2026-07-28`. Adding a revision is a minor release; dropping one
is a breaking change.

## After 1.0

The same surfaces stay stable; breaking any of them then needs a major release,
and a deprecated item survives at least twelve months before removal
([04-engineering-standards.md](docs/plan/04-engineering-standards.md)).
