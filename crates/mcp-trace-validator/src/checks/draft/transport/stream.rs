// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `2026-07-28` response-stream clauses: what may travel on the stream a
//! POST opened, and for how long.
//!
//! They judge the server's side of a stream — message direction, shape and
//! ordering, and the response headers that open it — never a request header and
//! never the body/header agreement a POST claims. That is why they sit apart
//! from the request-header clauses in [`super::headers`] and the rejection
//! clauses in [`super::validation`].

use std::collections::BTreeSet;

use serde_json::Value;

use super::super::super::FindingSink;
use crate::context::TraceContext;
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};

#[cfg(test)]
mod tests;

/// `TRAN-060` and `TRAN-119`: clients do not send JSON-RPC responses.
///
/// Judged on every binding, not just Streamable HTTP. Both binding pages state
/// the rule — "The client **MUST NOT** write JSON-RPC _responses_" on stdio,
/// and the POST form on HTTP — and it is one rule, because the revision removed
/// server-initiated requests outright: there is nothing on any transport for a
/// client response to answer. The earlier HTTP-only filter would have made this
/// silently vacuous for the stdio clause, reporting `pass` on a trace it never
/// inspected.
pub(in crate::checks) fn client_no_responses(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        if event.direction != Direction::ClientToServer {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let is_response = payload.get("id").is_some()
            && payload.get("method").is_none()
            && (payload.get("result").is_some() || payload.get("error").is_some());
        if is_response {
            sink.push(
                Some(event.seq),
                "client sent a JSON-RPC response; 2026-07-28 removed server-initiated \
                 requests, so there is nothing for one to answer"
                    .to_owned(),
            );
        }
    }
}

/// `TRAN-066`: the server sends no independent requests on a response stream.
///
/// Server-initiated requests are gone at this revision: what a server needs from
/// a client it asks for through MRTR, inside the result of the client's own
/// request. A server message carrying both `method` and a non-null `id` is
/// therefore a request it had no way to issue.
pub(in crate::checks) fn no_independent_server_requests(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, _, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if let Some(method) = payload.get("method").and_then(Value::as_str)
            && payload.get("id").is_some_and(|id| !id.is_null())
        {
            sink.push(
                Some(event.seq),
                format!(
                    "server sent an independent request `{method}`; 2026-07-28 replaces \
                     server-initiated requests with MRTR input requests"
                ),
            );
        }
    }
}

/// `TRAN-068`: an SSE response carries `X-Accel-Buffering: no`.
pub(in crate::checks) fn accel_buffering_header(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for event in context.events() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        let EventBody::Http { headers, .. } = &event.body else {
            continue;
        };
        let is_sse = headers
            .get("content-type")
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if is_sse && headers.get("x-accel-buffering").map(String::as_str) != Some("no") {
            sink.push(
                Some(event.seq),
                "SSE response does not carry `X-Accel-Buffering: no`".to_owned(),
            );
        }
    }
}

/// `TRAN-070`: nothing further is sent for a request whose stream was closed.
///
/// The revision makes closing a request's SSE response stream the cancellation
/// signal (TRAN-069), so the recorded form of that signal is a transport close
/// or abort on Streamable HTTP. Judged only against ids still outstanding when
/// it happened: a message answering a request the server had already completed
/// is a different defect, and not one this clause reaches.
/// One pass over the events, flipping at the close rather than comparing
/// sequence numbers against it. A `seq` comparison would be untestable here:
/// the close is a lifecycle event, so no *message* can ever share its `seq`,
/// and `<` versus `<=` would be a distinction no trace could exhibit.
pub(in crate::checks) fn no_messages_after_cancellation(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let mut outstanding: BTreeSet<String> = BTreeSet::new();
    let mut closed_at: Option<u64> = None;
    for event in context.events() {
        if let Some(closed_at) = closed_at {
            report_after_close(event, &outstanding, closed_at, sink);
        } else if is_cancellation(event) {
            closed_at = Some(event.seq);
        } else {
            track_outstanding(event, &mut outstanding);
        }
    }
}

/// Whether `event` is the recorded form of a response stream closing.
fn is_cancellation(event: &mcp_conformance_core::trace::TraceEvent) -> bool {
    let closed = matches!(
        event.body,
        EventBody::Lifecycle {
            event: LifecycleEvent::TransportClose | LifecycleEvent::TransportAbort
        }
    );
    closed && event.transport == TransportKind::StreamableHttp
}

/// Opens an id on a request and closes it on the answer, so `outstanding` holds
/// exactly the ids in flight.
fn track_outstanding(
    event: &mcp_conformance_core::trace::TraceEvent,
    outstanding: &mut BTreeSet<String>,
) {
    let Some(payload) = event.message_payload() else {
        return;
    };
    let Some(id) = payload.get("id").filter(|id| !id.is_null()) else {
        return;
    };
    if payload.get("method").is_some() {
        outstanding.insert(id.to_string());
    } else {
        outstanding.remove(&id.to_string());
    }
}

/// Reports a server message for an id that was still in flight at the close.
fn report_after_close(
    event: &mcp_conformance_core::trace::TraceEvent,
    outstanding: &BTreeSet<String>,
    closed_at: u64,
    sink: &mut FindingSink,
) {
    if event.direction != Direction::ServerToClient {
        return;
    }
    let Some(id) = event
        .message_payload()
        .and_then(|payload| payload.get("id"))
        .filter(|id| !id.is_null())
    else {
        return;
    };
    if outstanding.contains(&id.to_string()) {
        sink.push(
            Some(event.seq),
            format!(
                "server sent a further message for request id {id}, whose response \
                 stream closed at seq {closed_at}; a close is cancellation at this revision"
            ),
        );
    }
}
