// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A session as the session view draws it: every event read the way the
//! HTML report's timeline reads it ([`mcp_trace_validator::timeline`]), each
//! finding at its event, and the clauses to act on.
//!
//! Everything here is data. The front end turns each string into a text node,
//! never markup, so nothing a recording holds can become part of the page.

use std::collections::BTreeMap;

use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TraceEvent};
use mcp_trace_validator::report::{Outcome, Report};
use mcp_trace_validator::timeline::{self, Pairs, Shape};
use serde::Serialize;
use serde_json::{Value, json};

use super::entry::Entry;

/// The most events one response carries; the view asks for the next page.
pub(crate) const PAGE: usize = 2_000;

/// One finding, where the view lists it and where its event shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Finding {
    /// `f<n>`, unique within the session: the anchor both ends link to.
    pub(crate) key: String,
    pub(crate) requirement: String,
    pub(crate) revision: String,
    pub(crate) level: String,
    pub(crate) outcome: String,
    pub(crate) seq: Option<u64>,
    pub(crate) detail: String,
    pub(crate) check: String,
    pub(crate) quote: Option<String>,
    pub(crate) url: Option<String>,
}

fn word<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

/// Every finding of `reports`, in report order.
pub(crate) fn findings(reports: &[Report]) -> Vec<Finding> {
    let mut out = Vec::new();
    for report in reports {
        for row in &report.requirements {
            for finding in &row.findings {
                out.push(Finding {
                    key: format!("f{}", out.len() + 1),
                    requirement: row.id.clone(),
                    revision: report.revision.clone(),
                    level: row.level.clone(),
                    outcome: word(row.outcome),
                    seq: finding.seq,
                    detail: finding.detail.clone(),
                    check: finding.check.clone(),
                    quote: row.source.as_ref().map(|source| source.quote.clone()),
                    url: row.source.as_ref().map(|source| source.url.clone()),
                });
            }
        }
        if let Some(schema) = &report.wire_schema {
            for finding in &schema.findings {
                out.push(Finding {
                    key: format!("f{}", out.len() + 1),
                    requirement: "wire schema".to_owned(),
                    revision: report.revision.clone(),
                    level: "MUST".to_owned(),
                    outcome: word(schema.outcome),
                    seq: finding.seq,
                    detail: finding.detail.clone(),
                    check: finding.check.clone(),
                    quote: Some(schema.source.quote.clone()),
                    url: Some(schema.source.url.clone()),
                });
            }
        }
    }
    out
}

/// A message's reading, into `view`.
fn message_view(view: &mut Value, seq: u64, payload: &Value, pairs: &Pairs) {
    let (kind, method, id) = match timeline::shape(payload) {
        Shape::Request { method, id } => ("request", Some(method), Some(id)),
        Shape::Notification { method } => ("notification", Some(method), None),
        Shape::Result { id } => ("result", None, Some(id)),
        Shape::Error { id, error } => {
            view["error"] = json!(error);
            ("error", None, Some(id))
        }
        Shape::Batch(count) => {
            view["batch"] = json!(count);
            ("batch", None, None)
        }
        _ => ("malformed", None, None),
    };
    view["kind"] = json!(kind);
    view["method"] = json!(method.or_else(|| pairs.answers.get(&seq).map(|(_, m)| m.clone())));
    view["id"] = json!(id);
    view["answered_by"] = json!(pairs.answered_by.get(&seq));
    view["answers"] = json!(pairs.answers.get(&seq).map(|(seq, _)| seq));
    let pretty = serde_json::to_string_pretty(payload).unwrap_or_default();
    let (shown, rest) = timeline::preview(&pretty);
    view["payload"] = json!(shown);
    view["payload_more_bytes"] = json!(rest);
}

/// One event, read.
fn event_view(event: &TraceEvent, pairs: &Pairs, marks: &[&Finding]) -> Value {
    let direction = match event.direction {
        Direction::ClientToServer => "c2s",
        Direction::ServerToClient => "s2c",
    };
    let mut view = json!({
        "seq": event.seq,
        "direction": direction,
        "ts": event.ts,
        "marks": marks.iter().map(|mark| json!({
            "key": mark.key, "requirement": mark.requirement, "outcome": mark.outcome,
            "detail": mark.detail,
        })).collect::<Vec<_>>(),
    });
    match &event.body {
        EventBody::Message { payload } => message_view(&mut view, event.seq, payload, pairs),
        EventBody::Http {
            method,
            status,
            headers,
        } => {
            view["kind"] = json!("http");
            view["http_method"] = json!(method);
            view["status"] = json!(status);
            view["headers"] = json!(headers);
        }
        EventBody::Lifecycle { event } => {
            view["kind"] = json!("lifecycle");
            view["lifecycle"] = json!(match event {
                LifecycleEvent::TransportOpen => "transport opened",
                LifecycleEvent::TransportClose => "transport closed",
                LifecycleEvent::TransportAbort => "transport aborted",
                _ => "transport event",
            });
        }
        _ => view["kind"] = json!("event"),
    }
    view
}

/// Latency per method: for each paired request, the time to its answer in
/// milliseconds, when both events carry timestamps the module can read.
fn latency(events: &[TraceEvent], pairs: &Pairs) -> BTreeMap<u64, u64> {
    let by_seq: BTreeMap<u64, &TraceEvent> = events.iter().map(|e| (e.seq, e)).collect();
    let millis = |event: &TraceEvent| event.ts.as_deref().and_then(ts_millis);
    pairs
        .answered_by
        .iter()
        .filter_map(|(request, response)| {
            let start = millis(by_seq.get(request)?)?;
            let end = millis(by_seq.get(response)?)?;
            Some((*request, u64::try_from(end - start).ok()?))
        })
        .collect()
}

/// Milliseconds within a day of `HH:MM:SS[.fff]` in an RFC 3339 time, plus
/// whole days — enough to subtract two times of one session.
fn ts_millis(ts: &str) -> Option<i64> {
    let day = super::clock::age_in_days("1970-01-01", ts.get(..10)?)?;
    let time = ts.get(11..)?;
    let number = |range: core::ops::Range<usize>| time.get(range)?.parse::<i64>().ok();
    let (hours, minutes, seconds) = (number(0..2)?, number(3..5)?, number(6..8)?);
    let fraction: String = time
        .get(8..)
        .unwrap_or_default()
        .strip_prefix('.')
        .unwrap_or_default()
        .chars()
        .take_while(char::is_ascii_digit)
        .chain(core::iter::repeat('0'))
        .take(3)
        .collect();
    Some(
        ((day * 24 + hours) * 60 + minutes) * 60_000
            + seconds * 1000
            + fraction.parse::<i64>().ok()?,
    )
}

/// The session view: the entry, its findings, and one page of its events
/// from `from`.
pub(crate) fn session(entry: &Entry, events: &[TraceEvent], from: usize) -> Value {
    let pairs = timeline::pair(events);
    let found = findings(&entry.reports);
    let mut by_seq: BTreeMap<u64, Vec<&Finding>> = BTreeMap::new();
    for finding in &found {
        if let Some(seq) = finding.seq {
            by_seq.entry(seq).or_default().push(finding);
        }
    }
    let latency = latency(events, &pairs);
    let page: Vec<Value> = events
        .iter()
        .skip(from)
        .take(PAGE)
        .map(|event| {
            let marks = by_seq.get(&event.seq).map_or(&[][..], Vec::as_slice);
            let mut view = event_view(event, &pairs, marks);
            if let Some(ms) = latency.get(&event.seq) {
                view["latency_ms"] = json!(ms);
            }
            view
        })
        .collect();
    let attention: usize = entry
        .reports
        .iter()
        .map(|report| {
            report
                .requirements
                .iter()
                .filter(|row| {
                    matches!(
                        row.outcome,
                        Outcome::Fail | Outcome::Warn | Outcome::Unsupported
                    )
                })
                .count()
        })
        .sum();
    json!({
        "summary": entry.summary(),
        "totals": entry.reports.iter().map(|r| json!({"revision": r.revision, "verdict": word(r.verdict()), "totals": r.totals})).collect::<Vec<_>>(),
        "attention": attention,
        "findings": found,
        "events": page,
        "from": from,
        "total_events": events.len(),
        "next": (from + PAGE < events.len()).then_some(from + PAGE),
    })
}

#[cfg(test)]
mod tests;
