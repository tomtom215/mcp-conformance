// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reference MCP server exercising every protocol capability — in progress.
//!
//! The M2 build-out is underway on rmcp (the official Rust SDK; ADR-0008
//! records the MSRV consequence). What ships today:
//!
//! - [`policy`] — the default-secure HTTP transport policy that closes the
//!   CVE-2026-42559 class (DNS rebinding via unvalidated `Host`/`Origin`
//!   headers) by construction. It predates the server deliberately: the
//!   listener cannot be wired up insecurely by accident.
//! - [`server::EverythingServer`] — the [`rmcp::ServerHandler`] at the core of
//!   the everything server, currently advertising and implementing the tool
//!   capability ([`tools`]); the remaining `2025-11-25` capabilities land
//!   module-by-module, each advertised only once implemented.
//!
//! The binary (`cli` feature, on by default) serves over stdio; streamable
//! HTTP — behind [`policy`] — is next.

pub mod policy;
pub mod server;
pub mod tools;

pub use server::EverythingServer;
