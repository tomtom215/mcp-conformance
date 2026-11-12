<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# Repository rulesets, for the owner to import

Nothing in this directory is applied by the repository: a ruleset is a repository
setting, and only the owner can change one (Settings → Rules → Rulesets → New ruleset →
Import a ruleset). Nothing here was imported or tested against GitHub's importer, so read
each file before importing it; the exported shape was written from GitHub's documentation,
not checked against a live repository.

| File | What it would do | Why |
|------|------------------|-----|
| [`protect-main.json`](protect-main.json) | Keeps the existing protections (no deletion, no force push), and adds a required pull request (no approving review is required, since one maintainer cannot approve their own) and the required checks `CI gate` and `Mutants (PR diff)`, each the one aggregate job of its workflow | As of the 2026-11-11 audit the repository's only ruleset protects against deletion and force pushes, required status checks are off, and no merged pull request (38 of 38) had a review |
| [`protect-release-tags.json`](protect-release-tags.json) | Stops a `v*` tag from being moved or deleted | `release.yml` runs on `v*` tags and no ruleset covered them. Restricting who may *create* one needs the owner's actor id as a bypass actor, which this file does not guess |

Two settings are outside a ruleset: **private vulnerability reporting** (Settings → Code
security), which [`SECURITY.md`](../../SECURITY.md) points reporters at and which the audit
found off, and the `release` environment's protection.
