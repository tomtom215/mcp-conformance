<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright 2026 Tom F. (https://github.com/tomtom215) -->

# rmcp build-out hygiene findings — ready-to-file dossier

**What this is.** Two small, verified manifest/API-hygiene findings surfaced while
building [`mcp-everything-server`](../../crates/mcp-everything-server) on the official
Rust SDK ([`rmcp`](https://github.com/modelcontextprotocol/rust-sdk)) 1.7.0:
(1) three under-specified dependency floors that break `-Z minimal-versions` builds,
and (2) `Implementation::from_build_env()`, which is undocumented and reports rmcp's
own crate identity rather than the consuming application's. Filing is a maintainer
action; this document carries the verified mechanism, the reproduction, and the exact
text to post — the in-repo analogue of the
[#10](https://github.com/tomtom215/mcp-conformance/issues/10) dossier, kept as a
committed file. Tracked by engagement backlog item 9
([07-ecosystem-engagement.md](../plan/07-ecosystem-engagement.md)); the register record
is [3.9](../plan/01-ecosystem-context.md). **Pending owner authorization; nothing
posted.**

Per the [upstream-first policy](../plan/07-ecosystem-engagement.md) these are
**issue-first**: open the issue(s) describing the change, PR on maintainer interest.
They can go as one "hygiene findings from building on rmcp 1.7" issue with two
sections, or two tiny issues — the maintainer's preference decides.

**Strength, stated honestly up front.** The `from_build_env` finding and the
`tokio-util`/`tokio-stream` floors are exact (a named API at a known introduction
point). The `tracing` floor is an *empirical* building floor (the build fails below
0.1.41; the precise minimal was not bisected). `-Z minimal-versions` support is itself
**contentious upstream** — some maintainers consider honoring it out of scope — so the
floors half is a reasonable-but-decliney ask; the `from_build_env` half is the
higher-confidence, harder-to-argue-with piece.

---

## Finding 1 — under-specified dependency floors (`-Z minimal-versions`)

### Verified mechanism (2026-06-27)

rmcp 1.7.0's **published** manifest declares these floors (authoritative, from the
crates.io sparse index `https://index.crates.io/rm/cp/rmcp`, version `1.7.0`):

| Dependency | rmcp declares | Code needs | Why | Claim strength |
|------------|---------------|-----------|-----|----------------|
| `tokio-util` | `^0.7` (allows 0.7.0) | **≥0.7.9** | rmcp uses the `tokio_util::bytes` re-export, introduced in tokio-util **0.7.9** | exact |
| `tokio-stream` | `^0.1` (allows 0.1.0) | **≥0.1.1** | rmcp uses `tokio_stream::wrappers`, introduced in tokio-stream **0.1.1** | exact |
| `tracing` | `^0.1` (allows 0.1.0) | **≥0.1.41** | rmcp's `#[instrument]` functions fail to compile below 0.1.41 (empirical building floor; exact minimal not bisected) | empirical |

A consumer that resolves to the declared floor (which `-Z minimal-versions` does) gets a
tree that does not compile. `0.x` caret semantics make this concrete: `^0.7` means
`>=0.7.0, <0.8.0`, so 0.7.0 is a legal resolution, and 0.7.0 has no `tokio_util::bytes`
re-export.

A fourth, **separately owned** floor lives in the same tree: `sse-stream` 0.2.x (an
rmcp dependency) declares `http-body-util ^0.1` but imports `BodyDataStream`, added in
http-body-util **0.1.2**. That fix belongs to `sse-stream` (or to rmcp bumping its
`sse-stream` floor to a release that declares the tighter bound); it is noted here for
completeness but is not part of the rmcp manifest ask.

### Reproduce

```sh
# A project depending on rmcp 1.7.0 (server + macros + a transport), nightly:
cargo +nightly -Z minimal-versions update
cargo +nightly build
# Observe build failures resolving to tokio-util 0.7.0 (no `bytes` re-export),
# tokio-stream 0.1.0 (no `wrappers`), and tracing below 0.1.41.
```

This repository proves it continuously: the everything-server carries explicit
floor-shim declarations (`crates/mcp-everything-server/Cargo.toml`, the
"Minimal-versions floor shims" block) whose only job is to repair these upstream
bounds, and the scheduled `minimal-versions` CI job (`cargo xtask minimal-versions`,
`-Z direct-minimal-versions`) fails without them.

### Proposed fix

Tighten rmcp's declared floors to the versions it actually requires:
`tokio-util = "0.7.9"`, `tokio-stream = "0.1.1"`, and `tracing = "0.1.41"` (or the
bisected true minimum). One-line-each manifest change; no source change.

---

## Finding 2 — `Implementation::from_build_env()` reports rmcp's own identity

### Verified mechanism (2026-06-27, rmcp `main`, `crates/rmcp/src/model.rs:1057`)

```rust
pub fn from_build_env() -> Self {
    Implementation {
        name: env!("CARGO_CRATE_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        // ...
    }
}
```

`env!(..)` is a compile-time macro that reads the environment of **the crate being
compiled** — which, for this function body, is always `rmcp`. So `from_build_env()`
always yields `name: "rmcp"`, `version: <the rmcp version>`, regardless of the
application calling it. The name implies it reads *the caller's* build environment; it
cannot (a library function has no lexical access to its consumer's `CARGO_*`).

Two aggravating facts:

1. **It is undocumented** — no `///` comment, unlike every sibling on `impl
   Implementation` (`new`, `with_title`, `with_description`, …).
2. **It is the default** for `server_info`/`client_info` (model.rs:898/932/946 call
   `Implementation::from_build_env()` in the default param constructors), so a consumer
   that does not override it ships `serverInfo: {name: "rmcp", version: "1.7.0"}` — the
   SDK's identity in the protocol's server-identity field. This repo's everything-server
   avoids it explicitly (`crates/mcp-everything-server/src/server.rs`, the
   "Not `Implementation::from_build_env()`" comment).

### Proposed fix

- **Minimal (highest-confidence):** document the behavior — that it reports rmcp's own
  build environment, not the consumer's, and that applications wanting their own
  identity should call `Implementation::new(env!("CARGO_CRATE_NAME"),
  env!("CARGO_PKG_VERSION"))` **at their own call site** (where `env!` expands in their
  crate).
- **Complete (an API addition, maintainer's call):** provide a macro
  (e.g. `implementation_from_build_env!()`) that expands at the call site and captures
  the consumer's `CARGO_*`, so the default server/client identity is the application's,
  not the SDK's.

---

## Ready-to-file text

> **Title:** Hygiene from building on rmcp 1.7: three under-specified dependency
> floors, and `from_build_env()` reports rmcp's own identity
>
> **Body:**
> Two small findings from building a server on `rmcp = 1.7.0`. Happy to PR either.
>
> **1. Dependency floors are below what the code needs (`-Z minimal-versions`).**
> The published manifest declares `tokio-util = "0.7"`, `tokio-stream = "0.1"`, and
> `tracing = "0.1"`, but the code requires `tokio_util::bytes` (re-export added in
> tokio-util 0.7.9), `tokio_stream::wrappers` (added in tokio-stream 0.1.1), and a
> `tracing` no older than 0.1.41 (empirical building floor). A consumer resolving to
> the declared floors — e.g. under `-Z minimal-versions` — gets a tree that doesn't
> compile. Reproduce: `cargo +nightly -Z minimal-versions update && cargo +nightly
> build` on a project using rmcp 1.7 with server + a transport. Proposed: bump the
> three floors to `0.7.9` / `0.1.1` / `0.1.41`. (Separately: `sse-stream` 0.2.x
> under-declares `http-body-util` — it imports `BodyDataStream`, added in 0.1.2 — but
> that's a `sse-stream` manifest matter.)
>
> **2. `Implementation::from_build_env()` reports rmcp's identity, not the caller's.**
> It builds from `env!("CARGO_CRATE_NAME")` / `env!("CARGO_PKG_VERSION")`, which expand
> in rmcp's own compilation, so it always returns `{name: "rmcp", version: <rmcp
> version>}`. It is undocumented and is the default for `server_info`/`client_info`, so
> a server that doesn't override it advertises `serverInfo.name = "rmcp"`. Minimal fix:
> document that it reports rmcp's own build env and that consumers wanting their own
> identity should call `Implementation::new(env!("CARGO_CRATE_NAME"),
> env!("CARGO_PKG_VERSION"))` at their call site. Fuller fix (your call): a call-site
> macro that captures the consumer's `CARGO_*`.

## Reproducible method

```sh
# Floors: confirm the declared bounds at the head you are filing against.
curl -s "https://index.crates.io/rm/cp/rmcp" \
  | python3 -c "import sys,json;[print({d['name']:d['req'] for d in json.loads(l)['deps'] if d['name'] in ('tokio-util','tokio-stream','tracing')}) for l in sys.stdin if l.strip() and json.loads(l).get('vers')=='1.7.0']"

# from_build_env: confirm the body and that it is undocumented.
curl -s "https://raw.githubusercontent.com/modelcontextprotocol/rust-sdk/main/crates/rmcp/src/model.rs" \
  | sed -n '1057,1066p'
```

## Action

- [ ] Decide one issue (two sections) vs two issues, then file on
      `modelcontextprotocol/rust-sdk` (maintainer action; re-run the confirm
      one-liners against the head at filing time).
- [ ] On maintainer interest: PR the floor bumps (manifest-only) and/or the
      `from_build_env` docs note (and, if wanted, the call-site macro).
- [ ] When merged/declined: update register 3.9 and engagement backlog item 9; on a
      released rmcp that fixes the floors, drop the corresponding shims from
      `crates/mcp-everything-server/Cargo.toml`.
