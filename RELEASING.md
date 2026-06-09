<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Releasing

> **Status:** no version has been published yet. The tag-triggered automation
> (`.github/workflows/release.yml`, ADR-0007) is in place; the first release follows
> the bootstrap procedure below, after which the pipeline is token-less for good.

## Principles

- All publishable crates share one version and release together
  (`mcp-conformance-core`, `mcp-trace-validator`, `mcp-everything-server`,
  `mcp-reference-host`; `xtask` is never published).
- [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html). Pre-1.0, minor releases may
  break APIs; the changelog says so explicitly when they do.
- **Trusted publishing (OIDC)** to crates.io — no long-lived registry tokens exist
  anywhere in this project's configuration. One scoped exception, dead on arrival:
  crates.io cannot configure trusted publishing for a never-published crate
  ([register 2.14](docs/plan/01-ecosystem-context.md)), so the **bootstrap release
  only** uses a crate-scoped, short-expiry token in the `release` environment, deleted
  immediately after (procedure below; decision in
  [ADR-0007](docs/plan/decisions/0007-release-pipeline-and-trusted-publishing.md)).

## Publish order

Dependency order, with index-propagation waits between steps:

1. `mcp-conformance-core` (no internal deps)
2. `mcp-trace-validator` (depends on core)
3. `mcp-everything-server`, `mcp-reference-host`

## Release checklist

1. **Prepare** on a `release/vX.Y.Z` branch:
   - Bump `version` in `[workspace.package]` (one place; all crates inherit).
   - Move `[Unreleased]` to `[X.Y.Z] - YYYY-MM-DD` in `CHANGELOG.md`; add a fresh
     `[Unreleased]` section.
   - `cargo xtask ci` green; `cargo deny check` green; `cargo package --workspace`
     green.
   - Update the supported-versions table in `SECURITY.md`.
2. **Merge** via PR (CI must be green; no exceptions for release PRs).
3. **Tag**: `git tag -a vX.Y.Z -m "Release vX.Y.Z"` on `main`; push the tag.
4. **Automation** (`release.yml`): validates tag ↔ version ↔ changelog agreement,
   re-runs the full gate set (including MSRV clippy/tests and cross-OS tests), packages
   all crates with verification builds (`cargo package --workspace --locked` — the
   workspace-wide dry run; per-crate `--dry-run` cannot resolve unpublished sibling
   dependencies), attests SLSA build provenance over the `.crate` files, creates the
   GitHub Release with the changelog excerpt and checksummed artifacts, then — behind
   the `release` environment's required-reviewer approval — re-packages,
   **byte-compares against the attested SHA256SUMS**, and publishes to crates.io in
   dependency order. Re-running a partially published tag is safe: already-published
   crates are skipped and the chain resumes.
   Rehearse first: `Actions → Release → Run workflow` from the `release/vX.Y.Z`
   branch runs every gate and packaging step but can never publish.
5. **Verify**: crates on crates.io, docs on docs.rs, install path
   (`cargo install mcp-trace-validator`) works on a clean machine.

## When publishing fails mid-way

First, simply re-run the `publish` job: "already uploaded" crates are skipped and the
chain resumes. If the failure is in the code itself, fix forward: bump the patch
version for all crates, update the changelog, re-tag. Versions are never re-published
and tags are never moved.

## Bootstrap (first release only)

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
