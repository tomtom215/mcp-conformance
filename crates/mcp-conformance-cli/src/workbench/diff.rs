// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Two sessions side by side: what changed between, say, two SDK versions.
//!
//! **Messages** are aligned by what they are, in order: each message's key is
//! its direction, its method (a response takes the method of the request it
//! answers, by [`timeline::pair`] — the report's own pairing) and its id, and
//! the two key sequences are aligned by longest common subsequence. An aligned
//! pair is `same` when the payloads are equal JSON and `changed` when not; a
//! message with no partner is `only-a` or `only-b`. Alignment compares at most
//! [`ALIGN_LIMIT`] messages per side and says so when it stopped there.
//!
//! **Requirements** are compared per revision both sessions were judged at:
//! every clause whose outcome or number of findings differs.

use mcp_conformance_core::trace::{Direction, TraceEvent};
use mcp_trace_validator::report::Report;
use mcp_trace_validator::timeline::{self, Pairs, Shape};
use serde::Serialize;
use serde_json::Value;

use super::entry::Entry;

/// The most messages per side the alignment compares.
pub(crate) const ALIGN_LIMIT: usize = 2_000;

/// The most of a changed payload the diff carries, per side.
const PAYLOAD_LIMIT: usize = 4 * 1024;

/// One message as the diff shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Message {
    pub(crate) seq: u64,
    pub(crate) direction: &'static str,
    pub(crate) kind: &'static str,
    pub(crate) method: String,
    pub(crate) id: Option<String>,
}

/// One row of the aligned messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Row {
    pub(crate) status: &'static str,
    pub(crate) a: Option<Message>,
    pub(crate) b: Option<Message>,
    /// For a changed row, the start of each side's payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) payloads: Option<(String, String)>,
}

/// One clause whose reading differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Clause {
    pub(crate) revision: String,
    pub(crate) id: String,
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) a_findings: usize,
    pub(crate) b_findings: usize,
}

/// The whole comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct Diff {
    pub(crate) a: Value,
    pub(crate) b: Value,
    pub(crate) rows: Vec<Row>,
    /// `true` when a side had more than [`ALIGN_LIMIT`] messages.
    pub(crate) truncated: bool,
    pub(crate) clauses: Vec<Clause>,
    /// Revisions only one side was judged at.
    pub(crate) revisions_only_a: Vec<String>,
    pub(crate) revisions_only_b: Vec<String>,
}

fn message(event: &TraceEvent, payload: &Value, pairs: &Pairs) -> Message {
    let direction = match event.direction {
        Direction::ClientToServer => "c2s",
        Direction::ServerToClient => "s2c",
    };
    let answered = || {
        pairs
            .answers
            .get(&event.seq)
            .map(|(_, method)| method.clone())
            .unwrap_or_default()
    };
    let (kind, method, id) = match timeline::shape(payload) {
        Shape::Request { method, id } => ("request", method, Some(id)),
        Shape::Notification { method } => ("notification", method, None),
        Shape::Result { id } => ("result", answered(), Some(id)),
        Shape::Error { id, .. } => ("error", answered(), Some(id)),
        Shape::Batch(count) => ("batch", format!("batch of {count}"), None),
        _ => ("other", String::new(), None),
    };
    Message {
        seq: event.seq,
        direction,
        kind,
        method,
        id,
    }
}

/// The messages of `events`, with their payloads, at most `ALIGN_LIMIT`.
fn messages(events: &[TraceEvent]) -> (Vec<(Message, &Value)>, bool) {
    let pairs = timeline::pair(events);
    let all: Vec<(Message, &Value)> = events
        .iter()
        .filter_map(|event| {
            event
                .message_payload()
                .map(|payload| (message(event, payload, &pairs), payload))
        })
        .collect();
    let truncated = all.len() > ALIGN_LIMIT;
    (all.into_iter().take(ALIGN_LIMIT).collect(), truncated)
}

/// What two messages must share to be aligned.
fn key(message: &Message) -> (&str, &str, &str, Option<&str>) {
    let kind = if message.kind == "error" {
        "result"
    } else {
        message.kind
    };
    (
        message.direction,
        kind,
        &message.method,
        message.id.as_deref(),
    )
}

/// The start of `payload`, pretty-printed: the whole characters that fit in
/// [`PAYLOAD_LIMIT`] bytes.
fn excerpt(payload: &Value) -> String {
    let pretty = serde_json::to_string_pretty(payload).unwrap_or_default();
    let end = pretty
        .char_indices()
        .map(|(at, character)| at + character.len_utf8())
        .take_while(|&end| end <= PAYLOAD_LIMIT)
        .last()
        .unwrap_or(0);
    pretty[..end].to_owned()
}

/// Aligns two message sequences by longest common subsequence of their keys.
// The textbook names (a, b, n, m, i, j) keep the dynamic programme checkable
// against any account of the algorithm.
#[allow(clippy::many_single_char_names)]
fn align(a: &[(Message, &Value)], b: &[(Message, &Value)]) -> Vec<Row> {
    let (n, m) = (a.len(), b.len());
    // lengths[i][j]: the LCS of a[i..] and b[j..], row-major.
    let mut lengths = vec![0_u16; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lengths[at(i, j)] = if key(&a[i].0) == key(&b[j].0) {
                lengths[at(i + 1, j + 1)] + 1
            } else {
                lengths[at(i + 1, j)].max(lengths[at(i, j + 1)])
            };
        }
    }
    let mut rows = Vec::with_capacity(n.max(m));
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && key(&a[i].0) == key(&b[j].0) {
            let same = a[i].1 == b[j].1;
            rows.push(Row {
                status: if same { "same" } else { "changed" },
                a: Some(a[i].0.clone()),
                b: Some(b[j].0.clone()),
                payloads: (!same).then(|| (excerpt(a[i].1), excerpt(b[j].1))),
            });
            i += 1;
            j += 1;
        } else if j < m && (i == n || lengths[at(i, j + 1)] >= lengths[at(i + 1, j)]) {
            rows.push(Row {
                status: "only-b",
                a: None,
                b: Some(b[j].0.clone()),
                payloads: None,
            });
            j += 1;
        } else {
            rows.push(Row {
                status: "only-a",
                a: Some(a[i].0.clone()),
                b: None,
                payloads: None,
            });
            i += 1;
        }
    }
    rows
}

/// Every clause whose outcome or finding count differs, per revision both
/// sides were judged at.
fn clauses(a: &[Report], b: &[Report]) -> Vec<Clause> {
    let outcome = |row: &mcp_trace_validator::report::RequirementReport| {
        serde_json::to_value(row.outcome)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_default()
    };
    let mut differences = Vec::new();
    for left in a {
        let Some(right) = b.iter().find(|report| report.revision == left.revision) else {
            continue;
        };
        for row in &left.requirements {
            let other = right.requirements.iter().find(|other| other.id == row.id);
            let (b_outcome, b_findings) = other.map_or_else(
                || ("absent".to_owned(), 0),
                |o| (outcome(o), o.findings.len()),
            );
            let a_outcome = outcome(row);
            if a_outcome != b_outcome || row.findings.len() != b_findings {
                differences.push(Clause {
                    revision: left.revision.clone(),
                    id: row.id.clone(),
                    a: a_outcome,
                    b: b_outcome,
                    a_findings: row.findings.len(),
                    b_findings,
                });
            }
        }
    }
    differences
}

fn only(a: &[Report], b: &[Report]) -> Vec<String> {
    a.iter()
        .filter(|report| !b.iter().any(|other| other.revision == report.revision))
        .map(|report| report.revision.clone())
        .collect()
}

/// Compares entry `a` (with its events) with entry `b`.
pub(crate) fn diff(a: (&Entry, &[TraceEvent]), b: (&Entry, &[TraceEvent])) -> Diff {
    let (left, left_truncated) = messages(a.1);
    let (right, right_truncated) = messages(b.1);
    Diff {
        a: a.0.summary(),
        b: b.0.summary(),
        rows: align(&left, &right),
        truncated: left_truncated || right_truncated,
        clauses: clauses(&a.0.reports, &b.0.reports),
        revisions_only_a: only(&a.0.reports, &b.0.reports),
        revisions_only_b: only(&b.0.reports, &a.0.reports),
    }
}

#[cfg(test)]
mod tests;
