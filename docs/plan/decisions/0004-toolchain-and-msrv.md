<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# ADR 0004: Toolchain Policy — Edition 2024, MSRV 1.85, No Third-Party Toolchain Actions

**Date:** 2026-06-09
**Status:** Accepted — MSRV clause superseded by [ADR-0008](0008-msrv-1.88.md); the
action inventory has since grown with release.yml and the conformance job, and every
action remains SHA-pinned (the policy, not the enumeration, is the decision)
(2026-06-10: rmcp's measured floor forced the bump this ADR anticipated); edition,
toolchain-action, and lint policy remain in force
**Author:** Tom F.

---

## Context

M0 requires the MSRV and edition "selected and recorded with rationale". Constraints:
Edition 2024 requires rustc ≥ 1.85; our current dependency floor (serde, `serde_json`,
clap 4, proptest) compiles on 1.85; rmcp — a dependency from M2 — declares no MSRV at
all ([register 3.5](../01-ecosystem-context.md)), so it cannot anchor the choice. CI
needs toolchains on three platforms; a2a-rust uses the `dtolnay/rust-toolchain` action
for this, which is one more third-party action in the supply chain.

## Decision

1. **Edition 2024** across the workspace — current edition, and its lint/idiom
   improvements are free quality.
2. **MSRV 1.85**, pinned in `[workspace.package] rust-version`: the Edition 2024 floor
   and the lowest bound we *actually verify* — CI runs clippy and the full test matrix
   on both stable and 1.85 on Linux/macOS/Windows. MSRV bumps are minor releases with
   a changelog entry. When rmcp lands (M2), its real compilation floor is validated
   against 1.85 and this ADR is superseded if a bump is forced.
3. **No third-party toolchain action in CI**: toolchains install via plain
   `rustup toolchain install` (preinstalled on GitHub runners). Remaining actions
   (checkout, rust-cache, cargo-deny-action) are pinned by commit SHA.
4. **Lint policy lives in `[workspace.lints]`** (single source for all crates):
   `unsafe_code = "forbid"`, `missing_docs = "deny"`, clippy `pedantic` + `nursery` at
   warn with CI escalating via `RUSTFLAGS="-D warnings"`, plus `unwrap_used` /
   `expect_used` (test modules opt out locally and visibly). `clippy.toml` carries the
   analyzer thresholds and `msrv = "1.85"` so suggestions never exceed the MSRV.

## Consequences

### Positive

- The MSRV claim is tested, not aspirational — both toolchains gate every PR on every
  platform.
- One fewer third-party action than the a2a-rust baseline; the remaining three are
  SHA-pinned.
- Lint configuration cannot drift between crates; a new crate inherits the full policy
  from one manifest table.

### Negative

- 1.85 is a conservative floor that may exclude newer std APIs
  (e.g. `u32::cast_signed`, hit during implementation) — worked around or waited for.
- Running clippy on two toolchains means satisfying two lint sets; occasionally a
  newer lint must be written around rather than configured per-toolchain.
- Plain `rustup` commands are slightly more verbose in workflows than the action they
  replace.

## Alternatives Considered

### Track latest stable as MSRV (the a2a-rust approach: 1.93)

Rejected: a conformance tool wants to run in *other projects'* CI, where toolchains
lag; every unnecessary MSRV month costs adopters. a2a-rust is an SDK with different
trade-offs.

### Edition 2021 with a lower MSRV (e.g. 1.75)

Rejected: gives up Edition 2024's defaults for compatibility nobody has asked for
yet; revisit only if a concrete adopter is blocked.

### Keep dtolnay/rust-toolchain (SHA-pinned)

Rejected: the action's value over two lines of `rustup` is minor, and the supply-chain
surface is real. Nothing prevents reinstating it by a superseding ADR if toolchain
installation grows complex.
