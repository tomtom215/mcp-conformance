// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A baseline: the findings a server's author has accepted, so that a run can
//! fail on a finding that is *new* ([ADR-0025](https://github.com/tomtom215/mcp-conformance/blob/main/docs/plan/decisions/0025-a-baseline-for-validate.md)).
//!
//! A [`Baseline`] is a file of [`Entry`]s, each naming a finding by its
//! [`fingerprint`](crate::fingerprint) — the identity SARIF already reports —
//! and giving a reason, and optionally a last day. [`Baseline::apply`] marks the
//! findings the entries accept ([`Finding::baselined`](crate::report::Finding::baselined));
//! the report keeps every one of them, and its verdict stops counting them.
//! An entry that matches nothing is *stale*, and one whose day has passed is
//! *lapsed*; both are in the [`Notes`] it returns, because an acceptance that
//! outlives its cause is how a baseline turns into a place to hide.
//! [`record`] writes a baseline from what a run found.
//!
//! ```
//! use mcp_trace_validator::baseline::{self, Baseline, TraceReports};
//! use mcp_trace_validator::judge;
//! use mcp_trace_validator::report::Verdict;
//!
//! // A session whose second request reuses the first one's id (BASE-003).
//! let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}}
//! {"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"1"}}}}
//! {"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
//! let mut judgment = judge::judge(trace)?;
//! assert_eq!(judgment.verdict(), Verdict::Fail);
//!
//! // Accept what is there, then judge again: nothing is new.
//! let recorded = baseline::record(&[(Some("session.jsonl"), &judgment.reports)], None, None);
//! let mut traces = [TraceReports::new(Some("session.jsonl"), &mut judgment.reports)];
//! let applied = recorded.baseline.apply(&mut traces, None)?;
//! assert!(applied.notes.stale.is_empty());
//! assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Pass);
//! // The finding is still in the report, marked.
//! assert!(judgment.reports[0].baseline_tally().accepted > 0);
//! # Ok::<(), Box<dyn core::error::Error>>(())
//! ```

mod apply;
mod date;
mod file;
mod record;

pub use apply::{Applied, Notes, TraceReports};
pub use date::{Date, DateError};
pub use file::{Baseline, BaselineError, Entry, EntryProblem};
pub use record::{Recorded, record};

/// The JSON Schema (draft 2020-12) of a baseline file.
pub const JSON_SCHEMA: &str = include_str!("../schema/baseline.schema.json");

/// The `version` of the baseline file this build reads and writes.
pub const VERSION: u32 = 1;

/// The reason `--write-baseline` gives an entry no person has written one for.
///
/// It says what is true — a machine recorded the finding, nobody reviewed it —
/// rather than standing in for a judgment, and a run counts the accepted
/// findings that still carry it ([`BaselineTally::unreviewed`](crate::report::BaselineTally)).
pub const GENERATED_REASON: &str =
    "recorded by --write-baseline from the findings present at the time; not reviewed";

#[cfg(test)]
mod tests;
