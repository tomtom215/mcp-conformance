<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Handoff — response to the 2026-11-11 independent audit

A working record for the next session, not a plan document: it is dated, it is exempt from the living-document
gates, and [the roadmap](../plan/06-roadmap.md) still owns milestone status. Delete it when its items are done or
moved there. It is a status table only: the full triage carries reproduction detail for items still open, so, as with
[the earlier security review](2026-10-24-session-state.md) (its "Security review status" table),
it is not committed.

Branch: `claude/loving-cerf-icrjxf`, from `main` at `8435f7e` (the commit the auditor read; its tree is
`cb027327722c7d393633a04e6cfc55d06dcc08f0`). **Releases stay on hold** (maintainer, 2026-10-25): no version bump, tag
or publish.

## What the audit is, and how it was used

A 40-page independent review of `8435f7e` (17 findings, A01 to A17, plus recommendations). The auditor had no Rust
compiler: their runtime reproductions ran against the published v0.6.0 binary, and for main they read the source and CI
logs. So each finding was **re-checked against HEAD with the HEAD binaries** before anything was changed. Result: 17
rows, 13 confirmed and 4 needing a decision, none already fixed, none a non-defect, none duplicated. Below row level one
claim is not a defect (A02: a non-JSON line on stdio is recorded and fails; what stdio omits is an over-limit message),
and the workbench findings (A03, A11, A13, the workbench half of A02) are in **unreleased** code: v0.6.0 has no
`mcp-conformance-cli` crate.

Priorities in the audit are sequencing decisions, not severity ratings. The triage rated A05, A08, A09 and A10 at P2.

## Maintainer decisions (2026-11-12)

| # | Row | Decision |
|---|---|---|
| D1 | A02 | Optional additive `capture` object on the closing lifecycle line, shown as a report note; unparseable 2xx JSON and SSE bodies are recorded as string payloads as stdio already does (a declared verdict change); opt-in `--require-complete-capture` and `--fail-on-unrecorded`; default exit codes unchanged |
| D2 | A05 | Keep the raw HTTP method token and compare exactly (a declared verdict change); third-party importers fold case |
| D3 | A08 | Typed classes for unjudged sessions: informational (never answered, nothing judged) or error (malformed, unreadable, registry, unsupported revision unless pinned as expected) |
| D4 | A12 | A bounded `watch` queue with explicit loss reporting (the final judgment still comes from the trace file), plus the cheap fixes |

Defaults taken without asking (reversible): `rmcp` to 3.9.x, with the interop Rust server staying on 3.7.0 under an
expiring ledger row; no verdict from a workbench sidecar; review and merge `wip/C1` for A09; private modes with the
existing sidecar's mode kept; per-value counter pseudonyms for redaction; `cargo +nightly`, round-robin mutation shards
and an aggregator; repository-side governance only (no GitHub setting is changed from here); `install.sh` verifies the
attestation, failing closed; spec sources pinned. Out of scope this round: the audit's section 10 and 11 enhancement
recommendations (they are not defects), non-Rust advisory scans, and Server Cards scope (an owner call).

## Status

| Change | Rows | State |
|---|---|---|
| 1. `rmcp` 3.9.0, bounded host reads, audit of every lock | A01 | **Done, pushed** (`593208f`). Gates run: `cargo test --workspace --locked` in three feature sets (2047, 2010, 1700 passed, 0 failed), clippy `-D warnings` in three, clippy and the host and everything-server tests on 1.88, rustdoc, `xtask gates`, `falsify-gates` 17/17, `cargo deny`, `cargo audit` over every lock, minimal-versions (nightly), `xtask conformance`, `draft-readiness`, `draft-capture`. **Not run here:** the CI `visual`, Action and image jobs (this change touches none of what they render). Remaining A01 gaps: no non-Rust advisory scan; `docs/reports/pmcp-harness/Cargo.lock` is skipped by design (a frozen harness with seven advisories) |
| 2. Private workbench artifacts | A11 | **Done, pushed** (`a8c6f4e`, `c8ec9ef`). The workbench's imports, sidecars, redacted copies, exports and tar members, the everything server's `--tap-dir` and the reference host's `--trace-dir` are created owner-only; what it replaces keeps its mode; an already-readable library is said so once. Unix only: Windows ACLs are not set and were not tested. Diff-scoped mutation testing of `a8c6f4e`: 47 mutants, 35 caught, 9 unviable, 3 missed, all in `private.rs`, fixed by `c8ec9ef` |
| 3. Validator correlation (A04, A06, A07 and the same-pattern checks) | A04, A06, A07 | **Done, pushed** (`40d7120`). MRTR-020, retry pairing, CACH-015/016, MRTR-024, plus DISC-001/VERS-003, RES-022 and the `x-mcp-header` designation table judge a message only by what had been said before it. Three conforming corpus traces added; no existing golden moved. A declared verdict change. A generic suffix-invariance property test over the whole corpus was **not** written (each fix has its own) |
| 4. Workbench evidence integrity | A03, A13, A02 (workbench tail) | **Done, pushed** (`31489c6`). No verdict is read from a sidecar; redaction keeps equality with numbered stand-ins and records both verdicts; a partial verdict is marked provisional. The CI `visual` job was **not** run: the workbench's JSON entry gains a `provisional` member the page does not use, but baselines have not been checked |
| 5. Durable capture completeness | A02 | **Done, pushed** (`a007a4e`; ADR 0025). A body that is not JSON is recorded as a string over HTTP (a declared verdict change); the closing event carries a `capture` note, per trace; `validate --require-complete-capture` and `mcp-trace-capture --fail-on-unrecorded` are opt-in. Gates run: workspace tests in three feature sets (2096, 1762, 2134 passed, 0 failed), clippy `-D warnings` over the workspace, rustdoc `-D warnings`, `xtask gates`, `falsify-gates` 17/17, `conformance`, `draft-readiness`, `draft-capture`. **Diff-scoped mutation testing of `a007a4e`: running when this was written; see below.** Not done: the workbench does not display the note; MSRV 1.88 clippy and minimal-versions not re-run for this change; the CI `visual` job not run (the HTML note reuses the unanswered note's list class, so no stylesheet or golden changed) |
| 6. Outcome integrity and the Action output | A08, A09, A10 | **Done, not yet pushed when this was written** (`3c9af5c`..`de2a37d`). A09: the six `wip/C1` commits merged (one conflict, resolved by keeping both sides; its report's claims re-run, not trusted), and `xtask conformance` now requires each product leg's verdict to be `pass` and gates `set_aside` and `unjudged` lists in `conformance/wire-findings.json` both ways (seen red when either list is emptied). A08 (D3): unjudged sessions are `informational` or `error`; an error makes `test` exit 2 with verdict `incomplete`; `--expected-unjudged` names the exceptions and a stale one is an error. A10: `validate.sh` makes the report directory, exits 2 with an `::error::` for a report it cannot write, and escapes `<`, `&`, `"` and control characters (`action/test-validate.sh`, nine checks red before, none after; it runs in the `action` CI job). Gates run: workspace tests in three feature sets (2138, 1773, 2176 passed, 0 failed), clippy `-D warnings` in three, `xtask gates`, `falsify-gates` 17/17, `conformance`, `draft-readiness`, `draft-capture`. **Not run:** `shellcheck` (not installed here, so `action/test-validate.sh` is unlinted), the CI `action` job itself, the `visual` job (C1's report says the fixture library page now names a server for `sdk-rust-2026-07-28`; baselines not checked), MSRV 1.88 tests, and diff-scoped mutation testing of this change. `wip/V2` is still unmerged and its ADR-0025 collides with the repository's ADR 0025 |
| 7. Literal HTTP method token | A05 | Not started |
| 8. Resource bounds | A12 | Not started |
| 9. CI, governance, release verification, spec routing | A14, A15, A16, A17 | Not started |

## Owner-only actions (not done, not attempted)

- A15: the repository has one ruleset with deletion and non-fast-forward protection only; required status checks are off,
  no tag ruleset covers `v*`, no merged PR (38 of 38) has had a review, and private vulnerability reporting is off while
  `SECURITY.md` points reporters at it. Applying settings needs the owner; the repository-side work is a `ci-gate` job and
  honest governance text.
- Open Dependabot PRs: #75 (rmcp-macros 3.8.0) is superseded by the 3.9.0 bump and can be closed; #73 and #74 (jsonschema
  0.60.2), #66, #69, #72 and #76 are unrelated to the audit. Issue #71 (the weekly claims-expire run) is open and red.
- Server Cards scope (upstream is inconsistent about its status), and the treatment of register row 3.16 (a one-off
  measurement that cannot be satisfied by re-reading).

## Environment notes for the next session

- `cargo xtask conformance` builds with the ambient environment but launches binaries from `./target/debug`. If you set
  `CARGO_TARGET_DIR`, make `./target` a link to it (`target` is git-ignored), or the gate says "cannot spawn the server
  binary". Set `CARGO_PROFILE_DEV_DEBUG=line-tables-only` and `CARGO_INCREMENTAL=0` to keep a full build near 12 GB.
- Installed here and not in the repo: `cargo-deny` 0.20.2, `cargo-audit` 0.22.2, Rust 1.88.0 and nightly toolchains.
- Run `git add` before `cargo xtask gates`: it reads `git ls-files`.
