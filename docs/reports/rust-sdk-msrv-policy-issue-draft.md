<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# rust-sdk MSRV policy — issue draft (CORRECTED after empirical verification)

**What this is.** A ready-to-file upstream *issue* (issue-first per engagement policy)
proposing that [`modelcontextprotocol/rust-sdk`](https://github.com/modelcontextprotocol/rust-sdk)
declare a Minimum Supported Rust Version. It ships none today, yet its current release does
not build below **Rust 1.88**. Engagement [backlog #3](../plan/07-ecosystem-engagement.md),
[register 3.5](../plan/01-ecosystem-context.md); [ADR-0008](../plan/decisions/0008-msrv-1.88.md)
(§Correction) is the evidence trail.

**Status: DRAFT — NOT filed; value re-assessed as MARGINAL (owner decision needed).**
A 2026-06-15 first-hand empirical re-verification (below) **corrected two false claims** an
earlier draft of this file carried (a now-removed "E0658 / let-chains in rmcp's own source"
framing): the `<1.88` failure is cargo's **clear, named** MSRV pre-check on a *transitive*
dependency (`darling 0.23.0`), **not** an opaque `E0658`, and **not** let-chains in rmcp's
source. Because cargo already surfaces a clear error, the issue's value is Tier-1 MSRV
**documentation**, not rescuing users from silent breakage — a legitimate but minor
contribution. Re-run the probes at filing time and refresh the head SHA.

## Verified evidence (first-hand, 2026-06-15)

- **No MSRV is declared.** `rust-version` is absent from the workspace `Cargo.toml`, the
  `rmcp` crate manifest, and `rmcp-macros` (each fetched from `main` at head `266f870`,
  2026-06-15; consistent with [register 3.5](../plan/01-ecosystem-context.md)).
- **The floor of the current release is exactly 1.88 — set by a transitive dependency,
  not rmcp's own source.** With the `macros` feature, `rmcp-macros 1.7.0` →
  `darling = "0.23"` → `darling 0.23.0` (its only 0.23.x release, confirmed via the
  crates.io index) declares `rust-version = 1.88.0`. Measured against the **published**
  `rmcp =1.7.0` (a probe crate with `server`+`macros`+`transport-io`, one `Cargo.lock`
  resolved on 1.88 and shared across toolchains via `--locked`):

  | Toolchain | `cargo check --locked` | What it reports |
  |-----------|------------------------|-----------------|
  | 1.85.0 | **fails** (exit 101) | `error: rustc 1.85.0 is not supported by … darling@0.23.0 requires rustc 1.88.0` |
  | 1.87.0 | **fails** (exit 101) | `error: rustc 1.87.0 is not supported by … darling@0.23.0 requires rustc 1.88.0` |
  | 1.88.0 | **passes** | — |

- **The error is clear, not opaque.** Cargo's build-time MSRV check names the offending
  package (`darling`) and the required version (`1.88.0`) on both 1.85 and 1.87. There is
  no `E0658`, and rmcp's own source is never reached (the pre-check gates the build first).
  A grep of `crates/rmcp/src` and `crates/rmcp-macros/src` at `266f870` for `&&`-joined and
  `&& let` let-chains finds **none**.
- **Consequence (the honest, narrower one).** rmcp's supported floor is real (1.88) but
  undocumented. Declaring it would satisfy the Tier-1 "stable release & versioning
  documented" criterion (SEP-1730) and let cargo's MSRV-aware resolver pick a compatible
  rmcp for a downstream pinned below 1.88 — *rather than the de-facto behaviour today, where
  the floor is dictated by whichever `darling` the resolver happens to select.* It is **not**
  true (as an earlier draft claimed) that downstreams get an opaque error: they get a clear,
  named one.

## Ready-to-file issue text (corrected)

> **Title:** Declare an MSRV (`rust-version`) — the current release requires Rust 1.88
> (via `darling 0.23.0`), but nothing states it
>
> **Body:**
> rmcp declares no `rust-version` in any manifest, but the current release does not build
> below **Rust 1.88**. With the `macros` feature, `rmcp-macros 1.7.0` depends on
> `darling = "0.23"`, and `darling 0.23.0` (its only 0.23.x) declares
> `rust-version = 1.88.0`:
>
> ```sh
> # probe crate depending on rmcp =1.7.0 (features: server, macros, transport-io);
> # one Cargo.lock resolved on 1.88 and shared via --locked
> cargo +1.87.0 check --locked
> # error: rustc 1.87.0 is not supported by the following packages:
> #   darling@0.23.0 requires rustc 1.88.0
> cargo +1.88.0 check --locked   # passes
> ```
>
> Cargo reports this clearly today (it names darling and 1.88.0), so this isn't about
> opaque breakage — it's that rmcp's own supported floor is undocumented. Declaring it
> would (a) satisfy the Tier-1 "stable release & versioning documented" criterion
> (SEP-1730), and (b) let cargo's MSRV-aware resolver select a compatible rmcp for a
> downstream pinned below 1.88, instead of leaving the effective floor to whichever
> `darling` is resolved.
>
> **Proposal:**
> 1. Declare `rust-version` in `[workspace.package]` (inherited by the published crates).
>    The effective floor today is **1.88**, set by the `darling 0.23.0` dependency.
> 2. A CI job that builds/checks on the declared toolchain so the floor stays honest.
> 3. A documented bump policy (e.g. MSRV bumps are minor releases with a changelog entry).
>
> If you'd rather keep a lower MSRV, downgrading `darling` below 0.23 would lower the
> floor — happy to help either way (I verified the floor while building conformance
> tooling against rmcp).

## Reproduce (turnkey)

```sh
rustup toolchain install 1.87.0 1.88.0 --profile minimal
d=$(mktemp -d); cd "$d"; mkdir src; : > src/lib.rs
cat > Cargo.toml <<'TOML'
[workspace]
[package]
name = "rmcp-msrv-probe"
version = "0.0.0"
edition = "2021"
publish = false
[dependencies]
rmcp = { version = "=1.7.0", features = ["server", "macros", "transport-io"] }
TOML
cargo +1.88.0 generate-lockfile
cargo +1.88.0 check --locked   # passes
cargo +1.87.0 check --locked   # fails: rustc 1.87.0 is not supported by darling@0.23.0
```

## Filing checklist (owner action, after go-ahead)

- [ ] **Decide whether to file at all.** This is now a *minor* documentation request, not
      the "opaque breakage" pitch the earlier draft made. It is defensible as a small
      Tier-1 hygiene contribution; it is not a strong upstream-credibility play.
- [ ] Re-run the turnkey reproduce above and refresh the `main` head SHA in the evidence.
- [ ] Re-search the tracker for a duplicate (none as of 2026-06-15: a title+body search for
      `MSRV`, `rust-version`, and "minimum supported rust" returned only dependency-bump
      and chore PRs — notably #574 (the `darling 0.21 → 0.23` bump that *introduced* this
      floor) and #453 ("bump to rust 1.90") — no MSRV-policy issue).
- [ ] Open the issue on `modelcontextprotocol/rust-sdk`; on maintainer agreement, follow
      with a small PR for items 1–2 (the `rust-version` line + CI job).
- [ ] On resolution, update [register 3.5](../plan/01-ecosystem-context.md) and engagement
      [backlog #3](../plan/07-ecosystem-engagement.md).
