// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The message timeline: every event in `seq` order, what it was, which
//! request a response answers, and the findings that point at it.

use core::fmt::Write as _;
use std::collections::BTreeMap;

use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TraceEvent};
use serde_json::Value;

use super::Scope;
use super::clauses::{WIRE_SCHEMA, open_section, party_pill, token};
use super::escape::Text;
use crate::report::{Finding, Outcome, Party};
use crate::timeline::{Pairs, Shape, pair, preview, shape};

/// A finding as its event shows it.
struct Mark<'a> {
    anchor: String,
    clause: &'a str,
    revision: Option<&'a str>,
    outcome: Outcome,
    finding: &'a Finding,
    /// The party view of the report the finding came from.
    view: Option<Party>,
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
                        finding,
                        view: report.party_view,
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
        for row in report
            .extensions
            .iter()
            .flat_map(|extension| &extension.requirements)
        {
            add(&row.id, row.outcome, &row.findings);
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
            party_pill(out, mark.finding, mark.view);
            let _ = writeln!(
                out,
                "<span class=\"detail\">{}</span></li>",
                Text(&mark.finding.detail)
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
