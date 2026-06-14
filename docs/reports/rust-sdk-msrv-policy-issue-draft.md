<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# rust-sdk MSRV policy — ready-to-file issue (DRAFT)

**What this is.** A ready-to-file upstream *issue* (not a PR — engagement policy is
issue-first for anything non-trivial) proposing that
[`modelcontextprotocol/rust-sdk`](https://github.com/modelcontextprotocol/rust-sdk)
declare a Minimum Supported Rust Version. It ships none today, yet has a hard measured
floor of **1.88** (let-chains), which is invisible to Cargo's MSRV-aware resolver and so
silently breaks downstreams that pin older toolchains. Engagement
[backlog #3](../plan/07-ecosystem-engagement.md), [register 3.5](../plan/01-ecosystem-context.md);
our own [ADR-0008](../plan/decisions/0008-msrv-1.88.md) is the evidence trail.

**Status: DRAFT — not filed.** Outward-facing. **Do not open the issue without explicit
owner go-ahead.** Re-run the probes below at filing time and update the head SHA.

## Verified evidence (re-confirmed live 2026-06-14, `main` head `266f870`)

- **No MSRV is declared.** `rust-version` is absent from the workspace `Cargo.toml`, the
  `rmcp` crate manifest, and `rmcp-macros` (re-confirmed by fetching each from `main` at
  `266f870` on 2026-06-14; consistent with [register 3.5](../plan/01-ecosystem-context.md),
  first verified at tag `rmcp-v1.7.0`).
- **The measured floor is 1.88.** `cargo +1.85 check` fails with `E0658` (let-chains,
  stabilized in [Rust 1.88.0](https://blog.rust-lang.org/2025/06/26/Rust-1.88.0/)) across
  `rmcp` `=1.7.0, =1.6.0, =1.5.0, =1.4.0, =1.2.0, =1.0.0, =0.17.0`; independently,
  `rmcp-macros 1.7.0` → `darling = "0.23"` → `darling 0.23.0` itself declares
  `rust-version = "1.88.0"`. `cargo +1.88 check` passes with the
  `server`+`macros`+`transport-io`+`transport-streamable-http-server` feature set.
- **Consequence.** Because no `rust-version` is declared, Cargo's MSRV-aware resolver
  cannot protect downstreams, and a consumer on 1.85 gets an opaque `E0658` from a
  transitive macro crate rather than a clear "requires Rust 1.88" message. Downstreams
  (this project included) discover the floor by probing rather than reading it.

## Ready-to-file issue text

> **Title:** Declare an MSRV (`rust-version`) — the measured floor is 1.88 but nothing
> states it
>
> **Body:**
> rust-sdk declares no `rust-version` in any manifest, but the crates do not build below
> **Rust 1.88**: `cargo +1.85 check` fails with `E0658` for let-chains across every recent
> `rmcp` release (`=1.7.0` back through `=0.17.0`), and `rmcp-macros` pulls `darling 0.23`,
> which itself sets `rust-version = "1.88.0"`. `cargo +1.88 check` passes.
>
> Without a declared `rust-version`, Cargo's MSRV-aware resolver can't shield downstreams:
> a consumer pinned to an older stable gets an opaque `E0658` from a transitive proc-macro
> crate instead of a clear minimum-version error. It also leaves the supported floor
> undocumented for the Tier-1 "stable release & versioning documented" criterion
> (SEP-1730).
>
> **Proposal:**
> 1. Set `rust-version = "1.88"` in `[workspace.package]` (inherited by the published
>    crates).
> 2. Add a CI job that builds/checks on `1.88` so the declared floor stays honest (a job
>    that pins the toolchain and runs `cargo check`/`test` on the published feature set).
> 3. Document a bump policy: raise the MSRV only in a minor release, with a CHANGELOG
>    entry — the common "N-2 stable" / "bump-is-minor" convention.
>
> I've verified the floor independently while building conformance tooling against rmcp
> (happy to share the probe details / open a PR for items 1–2 if the policy is agreeable).
>
> Reproduce:
> ```sh
> for v in 1.7.0 1.6.0 1.5.0 1.4.0; do
>   cargo +1.85 add rmcp@=$v --features server,macros,transport-io 2>/dev/null
>   cargo +1.85 check   # fails: E0658 let-chains (stabilized 1.88.0)
> done
> cargo +1.88 check     # passes
> ```

## Filing checklist (owner action, after go-ahead)

- [ ] Re-run the probes and refresh the `main` head SHA in the evidence section.
- [ ] Check the tracker for an existing MSRV issue first (none found 2026-06-11 per
      [register 3.5](../plan/01-ecosystem-context.md); re-search at filing time).
- [ ] Open the issue on `modelcontextprotocol/rust-sdk`; on maintainer agreement, follow
      with the small PR for items 1–2 (the `rust-version` line + CI job).
- [ ] On resolution, update [register 3.5](../plan/01-ecosystem-context.md) and engagement
      [backlog #3](../plan/07-ecosystem-engagement.md).
