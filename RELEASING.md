<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Releasing

> **Status:** no version has been published yet. This documents the process the first
> release (roadmap M1) will follow; the tag-triggered automation lands together with
> that release, and this file is the specification it is built to.

## Principles

- All publishable crates share one version and release together
  (`mcp-conformance-core`, `mcp-trace-validator`, `mcp-everything-server`,
  `mcp-reference-host`; `xtask` is never published).
- [SemVer 2.0.0](https://semver.org/spec/v2.0.0.html). Pre-1.0, minor releases may
  break APIs; the changelog says so explicitly when they do.
- **Trusted publishing (OIDC)** to crates.io — no long-lived registry tokens exist
  anywhere in this project's configuration.

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
4. **Automation** (the release workflow, from M1): validates tag ↔ version ↔
   changelog agreement, re-runs the full gate set, packages with SLSA build-provenance
   attestation, publishes a dry run, creates the GitHub Release with notes from the
   changelog, then publishes to crates.io in dependency order via trusted publishing.
5. **Verify**: crates on crates.io, docs on docs.rs, install path
   (`cargo install mcp-trace-validator`) works on a clean machine.

## When publishing fails mid-way

Fix forward: bump the patch version for all crates, update the changelog, re-tag.
Versions are never re-published and tags are never moved.
