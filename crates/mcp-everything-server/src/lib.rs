// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reference MCP server exercising the protocol's capabilities at both
//! `2025-11-25` and `2026-07-28` (chosen per instance by
//! [`server::ServedRevision`]).
//!
//! Built on rmcp, the official Rust SDK (ADR-0008 records the MSRV
//! consequence). CI runs the pinned official conformance suite's
//! `2025-11-25` server scenarios against it via `cargo xtask conformance`,
//! and every check must pass; the crate README states the current scores.
//! What ships:
//!
//! - [`policy`] — the default-secure HTTP transport policy that closes the
//!   CVE-2026-42559 class (DNS rebinding via unvalidated `Host`/`Origin`
//!   headers) by construction: loopback-only allowlisting, fail-closed
//!   parsing, and outright denial of duplicate `Host`/`Origin` headers.
//!   The `http` module (feature `http`) enforces it with a 403 before any
//!   MCP processing.
//! - [`server::EverythingServer`] — the [`rmcp::ServerHandler`] implementing
//!   the suite's full server surface: every suite-defined tool ([`tools`],
//!   with sampling and elicitation in [`interactive`]), resources with
//!   templates and per-session-capped subscriptions ([`resources`]), prompts
//!   ([`prompts`]), completions, and logging-level filtering ([`logging`]) —
//!   plus `get-structured-content`, the TypeScript everything server's
//!   structured-output tool (`outputSchema` + `structuredContent`), which
//!   the suite does not exercise but the spec defines. Every listing is
//!   served in two pages ([`pagination`]): the suite's catalogue first, then
//!   icons, annotations, `resource_link` content, an audio prompt and
//!   `x-mcp-header` designations behind a `nextCursor`. The crate README
//!   records the one deliberate TypeScript-surface delta (async sampling —
//!   tasks, which `2026-07-28` moves to an extension) and why.
//! - The session trace tap (module `tap`, feature `tap`): each admitted HTTP
//!   session recorded as a validator-ready JSON Lines trace, capturing only
//!   the headers in its public `RECORDED_HEADERS` allowlist.
//!
//! The binary (`cli` feature, on by default) serves stdio or streamable HTTP.

pub mod fixtures;
#[cfg(feature = "http")]
pub mod http;
pub mod interactive;
pub mod logging;
pub mod notifying;
pub mod pagination;
pub mod policy;

/// Prefix of the one readiness line the HTTP binary prints to stderr.
///
/// The full line is `listening on <addr>`. Public because it is a
/// cross-process contract: orchestration (xtask's conformance task, the
/// binary tests) waits for this exact prefix before dialing — xtask cannot
/// depend on this crate, so its copy of the literal carries a pointer here
/// and the binary tests pin the coupling against the real executable.
pub const READINESS_LINE_PREFIX: &str = "listening on ";
pub mod prompts;
pub mod resources;
pub mod server;
pub mod subscriptions;
#[cfg(feature = "tap")]
pub mod tap;
pub mod tools;

pub use server::EverythingServer;
