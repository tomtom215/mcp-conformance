<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0014: Pin the Stable Toolchain in `rust-toolchain.toml`, Not in the Workflows

**Date:** 2026-08-24
**Status:** Accepted (extends [ADR-0004](0004-toolchain-and-msrv.md) clause 3, which
stands: still no third-party toolchain action)
**Author:** Tom F.

---

## Context

Every gate in this workspace runs at `-D warnings` with clippy's `pedantic` and
`nursery` groups enabled ([ADR-0004](0004-toolchain-and-msrv.md) clause 4). That makes
the compiler an **input** to the gate rather than a neutral tool: a Rust release adds
lints, and the build turns red on code nobody touched — on whichever pull request
happens to be open, whose author did not cause it.

That is precisely the shape
[04-engineering-standards.md](../04-engineering-standards.md) already refuses for the
official-suite pins: *a gate whose input moves underneath it is not a gate*. Both suite
versions are pinned exactly, and `cargo xtask suite-currency` exists so a pin cannot
fall behind unnoticed. Until 2026-08-24 the Rust toolchain was the one gate input still
floating: `ci.yml` installed `stable`, and contributors ran whatever `rustup` had given
them.

The cost was measured, not hypothetical. CI was red for **thirteen consecutive runs** on
`claude/mcp-conformance-maintenance-5517q3`. Two of the three causes were lints that did
not exist in the workspace's local toolchain: `1.98.0` added
`clippy::unused_async_trait_impl` (sixteen sites across the rmcp handler and transport
impls), and `clippy::manual_is_variant_and` began flagging `.ok().is_some_and(..)` on a
`Result` — verified directly against both toolchains on a minimal reproduction: `1.94.1`
raises neither, `1.98.0` raises both. (Which intermediate release introduced each was
not established; the pair of endpoints is what this decision rests on.) The local
toolchain was `1.94.1`. `cargo xtask ci` passed locally every time,
because a local gate **structurally cannot** run lints from a toolchain it does not
have. The gate was not lying; it was answering a different question than CI asked.

## Decision

1. **The toolchain is pinned exactly, in `rust-toolchain.toml` at the workspace root** —
   `channel = "1.98.0"`, `profile = "minimal"`, `components = ["clippy", "rustfmt"]`.

2. **Pinned there rather than in the workflows.** `rustup` honours the file for every
   `cargo` invocation inside the tree, so a checkout and CI compile with the same
   compiler and evaluate the same lints *by construction*. A workflow-only pin would
   have fixed the thirteen red runs and left the actual defect — a local gate that
   cannot see what CI enforces — exactly as it was.

3. **CI reads the pin rather than installing a channel.** Every job whose toolchain is
   the pinned one runs `rustup show active-toolchain`, which installs from the file.
   ADR-0004 clause 3 is unchanged: plain `rustup`, no third-party action.

4. **Legs that need a different toolchain say so where it outranks a directory
   override.** A `rust-toolchain.toml` beats `rustup default`, so the MSRV matrices set
   `RUSTUP_TOOLCHAIN`, and the unstable-flag legs use `cargo +nightly`. `cargo xtask
   ci`'s MSRV clippy leg uses an explicit `+1.88.0`. The MSRV
   ([ADR-0008](0008-msrv-1.88.md), `[workspace.package].rust-version`) is a different
   number for a different purpose — the oldest toolchain the crates *compile* on — and
   this decision does not touch it.

5. **`release.yml`'s `verify-install` job deliberately keeps bare `stable`.** It
   installs the published crate from crates.io, outside the workspace, simulating a
   user — and a user has whatever `stable` currently is. Pinning there would test a
   fiction.

6. **Two gates hold the pin from both sides.**
   - `cargo xtask toolchain-pin` (offline, runs inside `cargo xtask ci`): every
     workflow's toolchain reference agrees with the file, and no job reintroduces a bare
     `stable`. Failures name the file and line.
   - `cargo xtask toolchain-currency` (network, weekly, in the `claims-expire` job under
     [ADR-0010](0010-deferral-ledger-and-scheduled-reverification.md)): fails when
     crates.io's channel data shows a newer stable than the pin, so the pin cannot rot
     silently.

7. **Bumping is a deliberate change**, the same procedure as a suite pin: raise
   `channel`, run `cargo xtask ci`, and fix or record what the new lints say in the same
   commit. Holding the pin deliberately is a legitimate outcome, recorded where the pin
   lives.

## Consequences

### Positive

- The local gate and CI now answer the same question. `cargo xtask ci` passing is
  evidence about CI, which it was not before.
- A Rust release can no longer turn an unrelated pull request red. Lint churn arrives as
  a scheduled, attributable commit instead of as someone else's problem.
- The bump is reviewable: one file, one line, and a diff showing exactly which lints the
  new toolchain added and how each was answered.
- A contributor gets the right toolchain on first build rather than a surprise on push.

### Negative

- **New lints stop arriving for free.** This project treats clippy's newest suggestions
  as real quality signal, and pinning defers them until someone bumps. The weekly
  currency gate is the whole defence against that deferral becoming indefinite, and it
  is a weaker forcing function than a red build: an issue can be closed without acting.
- **A second toolchain is installed on first use.** A contributor already on a different
  stable silently downloads and stores another one — several hundred megabytes (the
  pinned minimal toolchain measured 734 MB on this machine, including one added
  cross-target). On a metered connection or a small CI cache that is a real cost.
- **The MSRV legs now depend on `RUSTUP_TOOLCHAIN` outranking the file** — a subtler
  mechanism than the `rustup default` it replaced, and one a future editor could undo
  without noticing. `toolchain-pin` covers the bare-`stable` case; it does not prove the
  override precedence itself, which is upstream `rustup` behaviour this repository
  cannot test.
- **The pin and the MSRV can drift apart in meaning.** Two version numbers now describe
  "the toolchain", and a reader must know which is which. Both files carry the
  distinction in a comment; nothing enforces that they stay explained.

## Alternatives Considered

### Pin in the workflows only (replace `stable` with `1.98.0` in each job)

Rejected: it fixes the red runs and preserves the defect that caused them. The local
gate would still run whatever the contributor has, still pass, and still say nothing
about CI. Thirteen red runs happened *with* every local gate green; a fix that leaves
that true is not a fix.

### Keep tracking `stable` and absorb the churn

Rejected on measured cost. It is defensible for a project whose CI is a smoke test; it
is not defensible when `-D warnings` over `pedantic` + `nursery` means every release is
a potential build break, landing on whichever pull request is open. The repository
already rejected this reasoning for the suite pins, and the same argument applies
verbatim.

### `dtolnay/rust-toolchain` with an explicit version input

Rejected twice over: it reinstates the third-party action
[ADR-0004](0004-toolchain-and-msrv.md) clause 3 removed from the supply chain, and being
a CI-only mechanism it would still leave local builds unpinned.

### Pin, but skip the currency gate

Rejected: an unmonitored pin is how a project ends up three toolchains behind and
discovers it during an urgent release. [ADR-0010](0010-deferral-ledger-and-scheduled-reverification.md)'s
premise is that a claim nothing re-checks is a claim that rots, and "1.98.0 is current"
is exactly such a claim.
