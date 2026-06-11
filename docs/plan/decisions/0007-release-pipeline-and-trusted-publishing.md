<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0007: Release Pipeline — OIDC Trusted Publishing with a One-Time Bootstrap Token

**Date:** 2026-06-09
**Status:** Accepted (amended 2026-06-10 — bootstrap complete, conditional removed)
**Author:** Tom F.

---

## Context

RELEASING.md's standing principle is that no long-lived registry tokens exist anywhere
in this project's configuration, and the roadmap's M1 line requires publishing via
trusted publishing. Two verified facts shape the implementation
([register 2.14](../01-ecosystem-context.md)): crates.io trusted publishing cannot be
configured on a crate that has never been published — the first release of a new crate
must use an API token — and, since January 2026, crate owners can enable a **"Trusted
Publishing Only"** mode that makes the registry itself reject token-based publishes.
The a2a-rust release pipeline is the proven baseline this project committed to exceed;
it works, but it keeps a permanent `CARGO_REGISTRY_TOKEN` secret, installs toolchains
through an unpinned third-party action (`dtolnay/rust-toolchain@master`) inside the
publish privilege context, packages with `--no-verify`, and attests artifacts without
ever checking that the published bytes match them.

## Decision

One tag-triggered workflow (`release.yml`), structured like a2a-rust's
(verify → cross-OS tests → package/attest → GitHub Release → environment-gated
publish), with these commitments:

1. **OIDC permanently, token once.** The publish job authenticates via
   `rust-lang/crates-io-auth-action` (SHA-pinned). A `CARGO_REGISTRY_TOKEN` secret —
   scoped to `publish-new` + `publish-update`, short expiry — exists in the `release`
   environment only for the bootstrap release. The job prefers the secret when present
   and OIDC otherwise, so deleting the secret completes the migration with no workflow
   edit. After the bootstrap: trusted publishing configured on all four crates
   (workflow `release.yml`, environment `release`), **Trusted Publishing Only**
   enabled, the secret deleted, and the token revoked — returning the project to its
   stated zero-token configuration with registry-side enforcement.
2. **No third-party code in the toolchain path.** Toolchains install via plain
   `rustup`, as in ci.yml; every action that does run is pinned by commit SHA.
3. **Packaging is verified and deterministic, provably.** `cargo package --workspace
   --exclude xtask --locked` with verification builds (never `--no-verify`); the publish job
   re-packages and byte-compares against the attested SHA256SUMS before uploading, so
   the SLSA attestation describably covers what was published rather than assuming
   `cargo publish`'s internal re-package matches.
4. **Resumable fix-forward.** Publishing is sequential in dependency order;
   "already uploaded" counts as success so a partially published tag can be re-run to
   completion. No yank/unyank recovery: this project repairs releases by shipping the
   next patch version, never by mutating a published one.
5. **Rehearsable.** `workflow_dispatch` runs every gate (tag check excepted) and can
   never reach the publish or release jobs, so the pipeline is exercised before the
   first real tag.

## Consequences

### Positive

- The M1 "trusted publishing" requirement is met in steady state, and "no long-lived
  tokens" becomes enforced by crates.io itself, not just by policy prose.
- A compromised workflow run after bootstrap can mint only a 30-minute scoped token
  via OIDC; there is no standing credential to exfiltrate.
- Supply-chain surface during release is the pinned action set plus rustup — nothing
  floating.

### Negative

- One conditional (`BOOTSTRAP_TOKEN` presence) lives in the workflow until the
  bootstrap completes; it is documented inline and in RELEASING.md with its removal
  condition (the secret's deletion makes it dead code that never runs).
  *Amendment (2026-06-10): the v0.1.0 bootstrap completed — Trusted Publishing is
  configured with "Trusted Publishing Only" enforced on all four crates, the
  `CARGO_REGISTRY_TOKEN` environment secret is deleted, and the token revoked
  (owner-confirmed). The conditional is removed from `release.yml`; the publish job
  is OIDC-only and this negative no longer applies.*
- The determinism check makes releases fail loudly if `cargo package` output is not
  reproducible across two jobs on the same runner image — a deliberate tripwire:
  silent non-determinism would invalidate the attestation's meaning.

## Alternatives Considered

### Keep a permanent registry token (the a2a-rust model)

Rejected: contradicts RELEASING.md's principle and the M1 DoD, and a long-lived
publish credential is the single highest-value secret a CI compromise could steal.

### Publish the bootstrap release from the owner's machine, no secret ever in GitHub

Viable and simpler in one sense, but rejected as the primary path: the first release
would then bypass the verify/test/attest/checksum chain entirely, and the GitHub
Release would carry artifacts the pipeline never produced. The environment-scoped,
expiring, crate-scoped secret is the smaller deviation, and it dies immediately after.

### Wait for crates.io "pending publisher" support for new crates

Rejected: not available as of the January 2026 development update
([register 2.14](../01-ecosystem-context.md)); the publish is wanted now. If it ships
later, nothing changes — the bootstrap path is already designed to be deleted.
