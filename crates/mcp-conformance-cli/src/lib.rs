// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-conformance` runs the official Model Context Protocol conformance suite
//! against a server or a client *through* `mcp-trace-capture`, and reports both
//! verdicts on the same traffic: the suite's checks, and `mcp-trace-validator`'s
//! clause-by-clause report of every session the suite drove
//! ([ADR-0021](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0021-a-workbench-people-keep-open.md)).
//!
//! It also watches a session live (`mcp-conformance watch`): recorded as
//! `mcp-trace-capture` records it, judged after each exchange, optionally
//! exported as OpenTelemetry spans.
//!
//! The binary is the product (`mcp-conformance test` and `watch`; the crate
//! README has the commands). This library is the small part it shares with the
//! repository's own gate, `cargo xtask conformance`, so the two cannot drift,
//! and the pure parts of `watch`, so they are tested and measured on their own:
//!
//! - [`suite`] — the suite's pinned version (the one place it is written), and
//!   reading its per-scenario results back;
//! - [`agreement`] — the policy that reconciles the validator's failures with
//!   the suite's verdict against a baseline of explained divergences;
//! - [`live`] — the incremental judge: a trace taken line by line, exchanges
//!   paired, and only new findings reported;
//! - [`otlp`] — exchanges and findings as OTLP/HTTP JSON spans under the
//!   OpenTelemetry semantic conventions for MCP;
//! - [`import`] — recordings made by other tools (`mcpsnoop` captures, MCP
//!   Inspector exports) as traces, with what each source does not record,
//!   which the workbench's importer and `cargo xtask mcpsnoop`'s conversion
//!   check share.
//!
//! With `default-features = false` it depends on the validator and the serde
//! family only.

#![forbid(unsafe_code)]

/// The crate README's examples, compiled and run as doctests so the README
/// cannot show code that no longer works.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

pub mod agreement;
pub mod import;
pub mod live;
pub mod otlp;
pub mod suite;
