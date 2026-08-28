<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Releasing

> **Status:** v0.1.0 (2026-06-10, bootstrap token) and v0.2.0 (2026-06-11, OIDC)
> are published. The publish job authenticates only via OIDC, and the v0.2.0
> publish is the proof that trusted publishing is configured for all four crates —
> its first attempt failed (crates.io: `No Trusted Publishing config found`), the
> owner added the config, and the re-run published. The owner confirmed on
> 2026-06-11, after that correction, that trusted publishing is working as
> intended — the per-crate **"Trusted Publishing Only"** switch and the bootstrap
> token's revocation rest on that statement, since the registry exposes no
> external check (ADR-0007 §Correction).

## Principles

- All publishable crates share one version and release together
  (`mcp-conformance-core`, `mcp-trace-validator`, `mcp-everything-server`,
  `mcp-reference-host`; `xtask` is never published).
- [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html). Pre-1.0, minor releases may
  break APIs; the changelog says so explicitly when they do.
- **Trusted publishing (OIDC)** to crates.io — no long-lived registry tokens exist
  anywhere in this project's configuration. The one scoped exception is spent:
  crates.io cannot configure trusted publishing for a never-published crate
  ([register 2.14](docs/plan/01-ecosystem-context.md)), so the **v0.1.0 bootstrap**
  used a crate-scoped, short-expiry token in the `release` environment — deleted and
  revoked immediately after (procedure record below; decision in
  [ADR-0007](docs/plan/decisions/0007-release-pipeline-and-trusted-publishing.md)).

## Publish order

Dependency order, with index-propagation waits between steps:

1. `mcp-conformance-core` (no internal deps)
2. `mcp-trace-validator` (depends on core)
3. `mcp-everything-server`, `mcp-reference-host`

## v0.3.0 pre-flight (third audit, 2026-06-13)

The next release is **0.3.0** — the version-class call and its two breaking
changes are stated at the top of `CHANGELOG.md` `[Unreleased]`. Checklist
legs pre-run on the audited tree (commit `669c5b4`):

| Leg | Result |
|-----|--------|
| `cargo xtask ci` (now incl. the MSRV clippy leg) | green |
| diff-scoped mutants vs `origin/main` | 0 missed (every audit slice) |
| full `--all-features` mutation sweep | 857 mutants: 741 caught, 116 unviable, **0 missed** (42 min) |
| `cargo +nightly miri test -p mcp-conformance-core` (round's new dimension) | green — 63 tests, 0 findings (isolation off for proptest's cwd persistence; PROPTEST_CASES=4; nesting depth 500 under cfg(miri)) |
| `cargo package --workspace --exclude xtask --locked` | green |
| `cargo audit` | green (233 dependencies, no advisories) |
| `cargo xtask conformance` (server 40/40 + agreement; client 4 scenarios + stdio smoke + agreement) | green |
| `cargo xtask spec-drift` | 140/140 quotes verified |
| minimal-versions floors (reqwest 0.13.2 / futures 0.3.30 / sse-stream 0.2.0) | build green |

Remaining for the owner: SECURITY.md's table flips to `0.3.x yes / 0.2.x no`
in the release PR; then steps 1–5 below.

## v0.5.0 pre-flight (2026-08-26)

Measured on the release tree, not asserted:

| Leg | Result |
|-----|--------|
| `cargo xtask ci` | green (MSRV clippy + cargo-deny report SKIPPED locally; CI enforces both) |
| `cargo xtask semver` (cargo-semver-checks 0.50.0) | green — "no semver update required" for all four crates at `0.4.0 → 0.5.0` |
| `cargo package --workspace --exclude xtask --locked` | green — all four crates packaged with verification builds |
| `cargo xtask version-sync` | green — README + `CITATION.cff` both `0.5.0` |
| `cargo xtask changelog-links` | green — 5 headings, `[Unreleased]` compares against `v0.5.0` |
| `cargo xtask register-currency --check` | green — all 72 rows inside the 90-day window |
| `cargo xtask deferrals --check` | green — 4 open rows, none expired |
| `cargo xtask coverage --check` / `draft-coverage --check` | green — 114 of 125 judgeable clauses evidenced across 5 captures |
| `Cargo.lock` diff | exactly 5 lines, the workspace crates only |

Run at the current version *before* the bump, `semver` reported three
undeclared API breaks; they are declared in the `0.5.0` section now. Not yet
run for this release: the full `--all-features` mutation sweep and miri, both of
which v0.3.0's audit covered and neither of which is in the standing checklist.

## v0.5.1 pre-flight (2026-08-28)

Measured on the release tree, not asserted:

| Leg | Result |
|-----|--------|
| `cargo xtask ci` | green (reports MSRV clippy SKIPPED locally; run separately below) |
| **MSRV clippy (1.88) × 3 feature modes** | **green** — run directly with the 1.88 toolchain installed, closing the gap every prior pre-flight left open |
| `cargo xtask semver` (cargo-semver-checks 0.50.0) | green — "no semver update required" for all four crates at `0.5.0 -> 0.5.1`; 196 checks pass, 58 skip, each |
| `cargo package --workspace --exclude xtask --locked` | green — all four crates packaged with verification builds |
| `cargo deny check` | green — advisories ok, bans ok, licenses ok, sources ok (the gate this release exists to un-break) |
| `cargo xtask version-sync` | green — README + `CITATION.cff` both `0.5.1` |
| `cargo xtask changelog-links` | green — 6 headings, `[Unreleased]` compares against `v0.5.1` |
| `cargo xtask register-currency --check` | green — all 72 rows inside the 90-day window |
| `cargo xtask deferrals --check` | green — 4 open rows, none expired (earliest review-by 2026-09-01) |
| `cargo xtask coverage --check` / `draft-coverage --check` | green — 114 of 125 judgeable clauses evidenced across 5 captures |
| `Cargo.lock` diff | exactly 5 lines, the workspace crates only — the chacha20 line came in ahead of the bump, via [#47](https://github.com/tomtom215/mcp-conformance/pull/47) |

**The semver gate means more here than it did at v0.5.0, and the checklist's own
caveat is why.** That caveat — pre-1.0 the minor position is the breaking
position, so `0.x -> 0.(x+1)` licenses any break and passes regardless — applies
to a *minor* bump. This bump moves the patch position only, which licenses
nothing, so "no semver update required" is a real assertion about all four
crates rather than a vacuous pass. It is also cheap to believe: no source file
changed between `0.5.0` and this tag.

**`SECURITY.md` is deliberately untouched**, against a checklist line that says to
update it. Its table tracks *minors* — "fixes land on the latest 0.x minor only" —
and `0.5.1` is inside `0.5.x`, so `0.5.x yes / 0.4.x no` is already correct. A
cosmetic edit to satisfy the checklist would have made the table say the same
thing with a newer timestamp; the checklist line is right for minors and vacuous
for patches.

Not run for this release: the full `--all-features` mutation sweep and miri.
Neither is in the standing checklist, and no source changed, so both would be
re-measuring `0.5.0`. The diff-scoped mutation gate did run on the PR that
carried the lockfile change, finding no mutants in changed code — the diff is
`Cargo.lock` and `CHANGELOG.md` only.

## Release checklist

1. **Prepare** on a `release/vX.Y.Z` branch:
   - Bump the version. This is **seven places, not one** — the claim that
     `[workspace.package]` alone suffices was wrong until v0.5.0 corrected it,
     and following it literally fails the build with
     `error: failed to select a version for the requirement mcp-conformance-core = "^0.4.0"`,
     because the internal dependency declarations pin an exact version rather
     than inheriting one:
     `Cargo.toml` `[workspace.package].version` and the three
     `[workspace.dependencies]` entries for `mcp-conformance-core`,
     `mcp-trace-validator` and `mcp-everything-server`;
     `crates/mcp-everything-server/Cargo.toml`'s `mcp-trace-validator` entry;
     `README.md`'s status line and `CITATION.cff`'s `version` (both enforced by
     `cargo xtask version-sync`, which fails the release otherwise), plus
     `CITATION.cff`'s `date-released`. Then `cargo update --workspace --offline`
     to move the five workspace crates in `Cargo.lock` without dragging in
     dependency updates the release never measured — the diff should be exactly
     five lines.
   - Move `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD` in `CHANGELOG.md`; add a fresh
     `[Unreleased]` section. Update the link-reference definitions at the foot of
     the file too: add `[X.Y.Z]: …/releases/tag/vX.Y.Z` and repoint `[Unreleased]:`
     to `…/compare/vX.Y.Z...HEAD` (`cargo xtask changelog-links` enforces both —
     the step v0.3.0 forgot, leaving `[0.3.0]` rendering as literal text and
     `[Unreleased]` still comparing against `v0.2.0`).
   - `cargo xtask ci` green; `cargo deny check` green; `cargo package --workspace --exclude xtask --locked`
     green.
   - **Run `cargo xtask semver` and read its output, before tagging.** It needs
     `cargo install cargo-semver-checks --locked`; without it the task reports
     SKIPPED, which in a green log is indistinguishable from a pass. That is how
     v0.5.0 shipped to this checklist with three API breaks nobody had written
     down. Note what the gate can and cannot do: it verifies the *bump is large
     enough*, and pre-1.0 the minor position is the breaking position, so
     `0.x → 0.(x+1)` licenses any break and passes no matter what the changelog
     says. Nothing mechanical checks that the breaks are declared. Reading this
     output against `[Unreleased]` is the whole control.
   - State the **version class** at the top of the new `[X.Y.Z]` section, and
     tabulate every breaking change with its migration — the pattern v0.3.0 set
     and v0.5.0 restored. Pre-1.0 minors may break APIs; this file's Principles
     say the changelog states so explicitly, and a reader upgrading should not
     have to find that out from `cargo build`.
   - Update the supported-versions table in `SECURITY.md`.
2. **Merge** via PR (CI must be green; no exceptions for release PRs).
3. **Tag**: `git tag -a vX.Y.Z -m "Release vX.Y.Z"` on `main`; push the tag.
4. **Automation** (`release.yml`): validates tag ↔ version ↔ changelog agreement,
   re-runs the full gate set (including MSRV clippy/tests and cross-OS tests), packages
   all publishable crates with verification builds (`cargo package --workspace --exclude xtask --locked` — the
   workspace-wide dry run; per-crate `--dry-run` cannot resolve unpublished sibling
   dependencies), attests SLSA build provenance over the `.crate` files, creates the
   GitHub Release with the changelog excerpt and checksummed artifacts, then — behind
   the `release` environment's required-reviewer approval — re-packages,
   **byte-compares against the attested SHA256SUMS**, and publishes to crates.io in
   dependency order. Re-running a partially published tag is safe: already-published
   crates are skipped and the chain resumes.
   Rehearse first: `Actions → Release → Run workflow` from the `release/vX.Y.Z`
   branch runs every gate and packaging step but can never publish.
5. **Verify**: crates on crates.io and docs on docs.rs — both still eyeball
   steps. The install path is **no longer one of them**: the `verify-install`
   job (added v0.4.0) runs `cargo install mcp-trace-validator` on a clean,
   uncached runner on both stable and MSRV the moment `publish` finishes, then
   runs the installed binary and checks it reports the published version.
   It is a detector rather than a gate — by the time it runs the version is
   immutable on crates.io — but the bug it exists for is a packaging one (a
   file missing from the `.crate` that every pre-publish gate passes, because
   the workspace still has it on disk), and finding that from CI in minutes
   beats finding it from a user.

   It cannot run any earlier: `mcp-trace-validator` depends on
   `mcp-conformance-core`, so installing it from the registry is impossible
   until the sibling is published — the same circularity that makes
   `cargo package` rather than per-crate `--dry-run` the pre-publish check.

## When publishing fails mid-way

First, simply re-run the `publish` job: "already uploaded" crates are skipped and the
chain resumes. If the failure is in the code itself, fix forward: bump the patch
version for all crates, update the changelog, re-tag. Versions are never re-published
and tags are never moved.

## Bootstrap (first release only — v0.1.0, 2026-06-10; record corrected 2026-06-11)

> What actually happened (evidence in ADR-0007 §Correction): steps 1–3 ran for
> v0.1.0 on 2026-06-10 — crates.io attributes v0.1.0 to the owner's token. Step 4
> did **not** happen then, although this file said it had: the v0.2.0 publish
> failed its OIDC exchange on 2026-06-11 (`400: No Trusted Publishing config found
> for repository tomtom215/mcp-conformance`,
> [run 27348688178](https://github.com/tomtom215/mcp-conformance/actions/runs/27348688178)),
> the owner then configured trusted publishing, and the re-run published all four
> crates via OIDC — the actual completion of step 4's first half. The "Trusted
> Publishing Only" toggle (step 4's second half) and step 5's secret deletion and
> token revocation are owner-visible only; the owner confirmed on 2026-06-11,
> after the record correction, that trusted publishing is working as intended
> (ADR-0007 §Correction records the confirmation and its evidentiary weight).
> Kept as the procedure record for any future first-publish of a new crate name.

1. On crates.io: Account Settings → API Tokens → **New Token** — name it
   `mcp-conformance bootstrap`, expiry **7 days**, scopes **publish-new** +
   **publish-update**, crate pattern `mcp-*`.
2. On GitHub: Settings → Environments → **New environment** `release` →
   add **Required reviewers** (yourself) and restrict **Deployment branches and
   tags** to tag rule `v*` → add **Environment secret** `CARGO_REGISTRY_TOKEN`
   with the token.
3. Release v0.1.0 per the checklist above; approve the `release` environment when
   the run pauses.
4. Immediately after all four crates are live, on crates.io for **each** crate:
   Settings → Trusted Publishing → **GitHub**: repository owner `tomtom215`,
   repository `mcp-conformance`, workflow `release.yml`, environment `release` —
   then enable **"Trusted Publishing Only"**.
5. Delete the `CARGO_REGISTRY_TOKEN` environment secret on GitHub and revoke the
   token on crates.io. From the next release on, the same workflow authenticates
   via OIDC; no edits are needed and token publishing is registry-rejected.
