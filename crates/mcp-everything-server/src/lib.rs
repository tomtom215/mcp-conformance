// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reference MCP server exercising every `2025-11-25` protocol capability.
//!
//! Built on rmcp, the official Rust SDK (ADR-0008 records the MSRV
//! consequence). Milestone M2 is complete: 40/40 checks on the official
//! conformance suite's `2025-11-25` server scenarios, run in CI against the
//! pinned suite via `cargo xtask conformance`. What ships:
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
//!   ([`prompts`]), completions, and logging-level filtering ([`logging`]).
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
pub mod policy;
pub mod prompts;
pub mod resources;
pub mod server;
#[cfg(feature = "tap")]
pub mod tap;
pub mod tools;

pub use server::EverythingServer;
