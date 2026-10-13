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
//! The one call that answers "is this recording conformant?", by the rules the
//! CLI applies — the revision the trace declares, a refusal for a contentless
//! trace — is [`judge::judge_with`] ([`judge::judge`] with the default
//! options). It reads any revision this build ships, so the same code judges a
//! `2025-11-25` session and a `2026-07-28` one by their own rules:
//!
//! ```
//! use mcp_trace_validator::judge::{self, Options};
//! use mcp_trace_validator::report::Verdict;
//! use mcp_trace_validator::sarif;
//!
//! let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"x","version":"0"}}}}
//! {"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"y","version":"0"}}}}
//! {"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
//! "#;
//!
//! let judgment = judge::judge_with(trace, &Options::default())?;
//! assert_eq!(judgment.reports[0].revision, "2025-11-25"); // as the trace declares
//! assert_eq!(judgment.verdict(), Verdict::Pass, "{:#?}", judgment.reports);
//!
//! // Everything a report format needs is in the judgment.
//! let artifact = sarif::Artifact { uri: Some("session.jsonl"), events: &judgment.events };
//! let log = sarif::render(&judgment.reports, artifact);
//! assert!(log.contains(r#""version": "2.1.0""#));
//! # Ok::<(), judge::JudgeError>(())
//! ```
//!
//! The steps it composes stay public for callers who need to vary one:
//! [`reader::parse_trace`], [`declared::select`] (which revision the trace
//! declares), and [`engine::validate`], which judges against whatever registry
//! it is handed — so a hand-picked registry judges a trace of another revision
//! by the wrong rules.

/// The crate README's examples, compiled and run as doctests so the README
/// cannot show code that no longer works.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

pub mod checks;
pub mod context;
pub mod declared;
pub mod engine;
pub mod judge;
pub mod junit;
pub mod multi;
pub mod reader;
pub mod report;
pub mod sarif;
pub mod sessions;
pub mod wire_schema;
