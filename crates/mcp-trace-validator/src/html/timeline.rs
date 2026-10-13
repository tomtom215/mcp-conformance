// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The message timeline: every event in `seq` order, what it was, which
//! request a response answers, and the findings that point at it.

use core::fmt::Write as _;
use std::collections::{BTreeMap, HashMap};

use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TraceEvent};
use serde_json::Value;

use super::Scope;
use super::clauses::{WIRE_SCHEMA, open_section, token};
use super::escape::Text;
use crate::report::{Finding, Outcome};

/// The most of a payload shown, in bytes of its pretty-printed JSON. A
/// recording can carry a 64 MiB tool result; the page shows the start of it
/// and says how much more the trace line holds, rather than becoming the size
/// of the recording several times over.
pub(super) const PAYLOAD_PREVIEW_BYTES: usize = 16 * 1024;

/// What a message is, read from its members.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Shape {
    /// A method and an id.
    Request { method: String, id: String },
    /// A method and no id.
    Notification { method: String },
    /// An id and a result.
    Result { id: String },
    /// An id and an error.
    Error { id: String, error: String },
    /// A JSON array of messages.
    Batch(usize),
    /// None of the above: a malformed message.
    Other,
}

/// `value` as one line of JSON, for ids and error codes.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// A method name as text: the string itself, or the JSON of whatever else a
/// malformed message put there.
fn method_text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| compact(value), ToOwned::to_owned)
}

pub(super) fn shape(payload: &Value) -> Shape {
    let Some(message) = payload.as_object() else {
        return payload
            .as_array()
            .map_or(Shape::Other, |batch| Shape::Batch(batch.len()));
    };
    let id = message.get("id").map(compact);
    match (message.get("method"), id) {
        (Some(method), Some(id)) => Shape::Request {
            method: method_text(method),
            id,
        },
        (Some(method), None) => Shape::Notification {
            method: method_text(method),
        },
        (None, Some(id)) => match message.get("error") {
            Some(error) => Shape::Error {
                id,
                error: error.get("code").map_or_else(
                    || "error".to_owned(),
                    |code| format!("error {}", compact(code)),
                ),
            },
            None if message.contains_key("result") => Shape::Result { id },
            None => Shape::Other,
        },
        (None, None) => Shape::Other,
    }
}

/// Requests paired with the responses that answer them.
#[derive(Debug, Default)]
pub(super) struct Pairs {
    /// A request's `seq` → the `seq` of its response.
    pub(super) answered_by: BTreeMap<u64, u64>,
    /// A response's `seq` → its request's `seq` and method.
    pub(super) answers: BTreeMap<u64, (u64, String)>,
}

/// Pairs each response with the request it answers: the latest unanswered
/// request with its id, sent the other way. A response to an id nobody asked
/// for, or a second answer to one, pairs with nothing — the validator's
/// findings say what is wrong with it; the timeline only draws what is there.
pub(super) fn pair(events: &[TraceEvent]) -> Pairs {
    let mut open: HashMap<(Direction, String), (u64, String)> = HashMap::new();
    let mut pairs = Pairs::default();
    for event in events {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        match shape(payload) {
            Shape::Request { method, id } => {
                open.insert((event.direction, id), (event.seq, method));
            }
            Shape::Result { id } | Shape::Error { id, .. } => {
                let asker = match event.direction {
                    Direction::ClientToServer => Direction::ServerToClient,
                    Direction::ServerToClient => Direction::ClientToServer,
                };
                if let Some((request, method)) = open.remove(&(asker, id)) {
                    pairs.answered_by.insert(request, event.seq);
                    pairs.answers.insert(event.seq, (request, method));
                }
            }
            Shape::Notification { .. } | Shape::Batch(_) | Shape::Other => {}
        }
    }
    pairs
}

/// A finding as its event shows it.
struct Mark<'a> {
    anchor: String,
    clause: &'a str,
    revision: Option<&'a str>,
    outcome: Outcome,
    detail: &'a str,
}

/// Every finding that names an event, by the event's `seq`, in report order.
fn marks<'a>(scope: &Scope<'a>) -> BTreeMap<u64, Vec<Mark<'a>>> {
    let mut marks: BTreeMap<u64, Vec<Mark<'a>>> = BTreeMap::new();
    let reports = scope.reports;
    for report in reports {
        let revision = scope.multi().then_some(report.revision.as_str());
        let mut add = |clause: &'a str, outcome: Outcome, findings: &'a [Finding]| {
            let anchor = scope.clause(&report.revision, clause);
            for (index, finding) in findings.iter().enumerate() {
                if let Some(seq) = finding.seq {
                    marks.entry(seq).or_default().push(Mark {
                        anchor: format!("{anchor}-{}", index + 1),
                        clause,
                        revision,
                        outcome,
                        detail: &finding.detail,
                    });
                }
            }
        };
        for row in &report.requirements {
            add(&row.id, row.outcome, &row.findings);
        }
        if let Some(schema) = &report.wire_schema {
            add(WIRE_SCHEMA, schema.outcome, &schema.findings);
        }
    }
    marks
}

/// The timeline section.
pub(super) fn write(out: &mut String, scope: &Scope<'_>) {
    open_section(out, scope, "timeline", "Message timeline");
    let events = scope.trace.events;
    if events.is_empty() {
        out.push_str("<p class=\"empty\">The trace holds no events.</p>\n</section>\n");
        return;
    }
    let marks = marks(scope);
    let pairs = pair(events);
    let flagged = events.iter().filter(|e| marks.contains_key(&e.seq)).count();
    let flagged_text = match flagged {
        0 => "no finding names any of them".to_owned(),
        1 => "1 carries findings, each linked to its clause".to_owned(),
        _ => format!("{flagged} carry findings, each linked to its clause"),
    };
    let _ = writeln!(
        out,
        "<p class=\"lede\">{} {} in capture order; {flagged_text}.</p>",
        events.len(),
        if events.len() == 1 { "event" } else { "events" },
    );
    if flagged > 0 && flagged < events.len() {
        // Read by a CSS sibling selector: the switch works without a script.
        let id = scope.id("only-flagged");
        let _ = writeln!(
            out,
            "<input type=\"checkbox\" class=\"only-flagged\" id=\"{id}\">\
             <label class=\"switch\" for=\"{id}\">Only events with findings</label>"
        );
    }
    out.push_str("<ol class=\"timeline\">\n");
    for event in events {
        let found = marks.get(&event.seq).map_or(&[][..], Vec::as_slice);
        write_event(out, scope, event, &pairs, found);
    }
    out.push_str("</ol>\n</section>\n");
}

fn write_event(
    out: &mut String,
    scope: &Scope<'_>,
    event: &TraceEvent,
    pairs: &Pairs,
    found: &[Mark<'_>],
) {
    let (direction, arrow) = match event.direction {
        Direction::ClientToServer => ("c2s", "client → server"),
        Direction::ServerToClient => ("s2c", "server → client"),
    };
    let flagged = if found.is_empty() { "" } else { " flagged" };
    let id = scope.event(event.seq);
    let _ = write!(
        out,
        "<li class=\"ev {direction}{flagged}\" id=\"{id}\"><div class=\"ev-line\">\
         <a class=\"ev-seq\" href=\"#{id}\" aria-label=\"event {seq}\">{seq}</a>\
         <span class=\"ev-dir\">{arrow}</span>",
        seq = event.seq
    );
    summary(out, scope, event, pairs);
    if let Some(ts) = &event.ts {
        let _ = write!(out, "<span class=\"ev-ts\">{}</span>", Text(ts));
    }
    out.push_str("</div>\n");
    if !found.is_empty() {
        out.push_str("<ul class=\"marks\">\n");
        for mark in found {
            let _ = write!(
                out,
                "<li class=\"o-{}\"><a href=\"#{}\">{}</a>",
                token(mark.outcome),
                mark.anchor,
                Text(if mark.clause == WIRE_SCHEMA {
                    "wire schema"
                } else {
                    mark.clause
                })
            );
            if let Some(revision) = mark.revision {
                let _ = write!(out, "<span class=\"pill rev\">{}</span>", Text(revision));
            }
            let _ = writeln!(
                out,
                "<span class=\"detail\">{}</span></li>",
                Text(mark.detail)
            );
        }
        out.push_str("</ul>\n");
    }
    body(out, event, !found.is_empty());
    out.push_str("</li>\n");
}

/// The event's kind, and for a message its method, id and pairing.
fn summary(out: &mut String, scope: &Scope<'_>, event: &TraceEvent, pairs: &Pairs) {
    match &event.body {
        EventBody::Message { payload } => {
            message_summary(out, scope, event.seq, payload, pairs);
        }
        EventBody::Http { method, status, .. } => {
            let _ = match (method, status) {
                (Some(method), _) => write!(
                    out,
                    "<span class=\"kind k-http\">HTTP request</span><code class=\"method\">{}</code>",
                    Text(method)
                ),
                (None, Some(status)) => write!(
                    out,
                    "<span class=\"kind k-http\">HTTP response</span><code class=\"method\">{status}</code>"
                ),
                (None, None) => write!(out, "<span class=\"kind k-http\">HTTP</span>"),
            };
        }
        EventBody::Lifecycle { event } => {
            let what = match event {
                LifecycleEvent::TransportOpen => "transport opened",
                LifecycleEvent::TransportClose => "transport closed",
                LifecycleEvent::TransportAbort => "transport aborted",
                _ => "transport event",
            };
            let _ = write!(out, "<span class=\"kind k-life\">{what}</span>");
        }
        _ => out.push_str("<span class=\"kind\">event</span>"),
    }
}

fn message_summary(out: &mut String, scope: &Scope<'_>, seq: u64, payload: &Value, pairs: &Pairs) {
    let link = |out: &mut String, text: &str, target: u64| {
        let _ = write!(
            out,
            "<span class=\"pair\">{text} <a href=\"#{}\">seq {target}</a></span>",
            scope.event(target)
        );
    };
    match shape(payload) {
        Shape::Request { method, id } => {
            let _ = write!(
                out,
                "<span class=\"kind k-request\">request</span><code class=\"method\">{}</code>\
                 <span class=\"id\">id <code>{}</code></span>",
                Text(&method),
                Text(&id)
            );
            match pairs.answered_by.get(&seq) {
                Some(response) => link(out, "answered at", *response),
                None => out.push_str("<span class=\"pair none\">no answer recorded</span>"),
            }
        }
        Shape::Notification { method } => {
            let _ = write!(
                out,
                "<span class=\"kind k-note\">notification</span><code class=\"method\">{}</code>",
                Text(&method)
            );
        }
        Shape::Result { id } => response(out, ("k-result", "result"), &id, seq, pairs, link),
        Shape::Error { id, error } => response(out, ("k-error", &error), &id, seq, pairs, link),
        Shape::Batch(count) => {
            let _ = write!(out, "<span class=\"kind k-other\">batch of {count}</span>");
        }
        Shape::Other => out.push_str("<span class=\"kind k-other\">malformed message</span>"),
    }
}

/// A response's kind, the method of the request it answers, its id, and a
/// link to that request.
fn response(
    out: &mut String,
    (class, kind): (&str, &str),
    id: &str,
    seq: u64,
    pairs: &Pairs,
    link: impl Fn(&mut String, &str, u64),
) {
    let _ = write!(out, "<span class=\"kind {class}\">{}</span>", Text(kind));
    let answered = pairs.answers.get(&seq);
    if let Some((_, method)) = answered {
        let _ = write!(out, "<code class=\"method\">{}</code>", Text(method));
    }
    let _ = write!(
        out,
        "<span class=\"id\">id <code>{}</code></span>",
        Text(id)
    );
    match answered {
        Some((request, _)) => link(out, "answers", *request),
        None => out.push_str("<span class=\"pair none\">answers no open request</span>"),
    }
}

/// The event's content, collapsed — open where a finding points.
fn body(out: &mut String, event: &TraceEvent, open: bool) {
    let open = if open { " open" } else { "" };
    match &event.body {
        EventBody::Message { payload } => {
            let pretty = serde_json::to_string_pretty(payload).unwrap_or_default();
            let (shown, rest) = preview(&pretty);
            let _ = write!(
                out,
                "<details class=\"payload\"{open}><summary>Payload</summary><pre><code>{}</code></pre>",
                Text(shown)
            );
            if rest > 0 {
                let _ = write!(
                    out,
                    "<p class=\"truncated\">… {rest} more bytes; the trace line holds the whole message.</p>"
                );
            }
            out.push_str("</details>\n");
        }
        EventBody::Http { headers, .. } if !headers.is_empty() => {
            let _ = write!(
                out,
                "<details class=\"payload\"{open}><summary>Headers <span class=\"count\">{}</span></summary>\
                 <dl class=\"headers\">",
                headers.len()
            );
            for (name, value) in headers {
                let _ = write!(out, "<dt>{}</dt><dd>{}</dd>", Text(name), Text(value));
            }
            out.push_str("</dl></details>\n");
        }
        _ => {}
    }
}

/// The start of `text` the page shows, and how many bytes it leaves out.
pub(super) fn preview(text: &str) -> (&str, usize) {
    if text.len() <= PAYLOAD_PREVIEW_BYTES {
        return (text, 0);
    }
    let mut end = PAYLOAD_PREVIEW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], text.len() - end)
}

#[cfg(test)]
mod tests;
