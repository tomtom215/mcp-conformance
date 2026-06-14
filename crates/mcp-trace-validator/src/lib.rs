// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Deterministic offline validation of recorded MCP protocol traces.
//!
//! The validator replays a trace (JSON Lines of
//! [`mcp_conformance_core::trace::TraceEvent`]) against a requirement registry and
//! produces a [`report::Report`] with one outcome per requirement and a precise
//! [`report::Finding`] for every violation: requirement ID, offending event `seq`, and
//! an actionable detail string.
//!
//! Three properties are load-bearing and tested, not aspirational:
//!
//! 1. **Determinism** — same trace bytes, same registry: byte-identical report. The
//!    engine touches no clock, no randomness, no environment.
//! 2. **No I/O in the engine** — [`engine::validate`] is a pure function over parsed
//!    events; reading files and rendering output happen at the CLI edge.
//! 3. **Honest accounting** — requirements the registry excludes are reported as
//!    excluded (never as passed), and registry entries referencing checks this build
//!    does not implement are reported as unsupported (never silently skipped).
//!
//! # Example
//!
//! ```
//! use mcp_conformance_core::requirement::Registry;
//! use mcp_trace_validator::{engine, reader};
//!
//! let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"x","version":"0"}}}}
//! {"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"y","version":"0"}}}}
//! {"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
//! "#;
//!
//! let registry = Registry::builtin_2025_11_25().expect("embedded registry is valid");
//! let events = reader::parse_trace(trace, &reader::Limits::default()).expect("valid trace");
//! let report = engine::validate(&registry, &events);
//! assert!(!report.has_errors(), "{report:#?}");
//! ```

pub mod checks;
pub mod context;
pub mod engine;
pub mod junit;
pub mod multi;
pub mod reader;
pub mod report;
