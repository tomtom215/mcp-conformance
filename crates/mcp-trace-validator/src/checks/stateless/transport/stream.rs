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
        sink.examined();
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
        sink.examined();
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
        if !is_sse {
            continue; // Only an event stream can be buffered by a proxy.
        }
        sink.examined();
        if headers.get("x-accel-buffering").map(String::as_str) != Some("no") {
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
/// signal (TRAN-069), so the recorded form of that signal is an orderly
/// `transport-close` on Streamable HTTP. Two readings this check used to make
/// are gone, because both convicted servers of cancellations that never
/// happened:
///
/// - **A close names no request.** The lifecycle event carries no id, and a
///   stream is one request's. It is attributed only when exactly one request is
///   in flight; with several, which stream ended is unknown and nothing is
///   judged. Read as cancelling *every* request in flight, one closed stream
///   failed the server for answering all the others.
/// - **An abort is not a cancellation.** `transport-abort` records a transport
///   that *failed* — the capture proxy writes it when the upstream server could
///   not be reached or broke mid-response, and answers the client 502 itself.
///   That is not the client closing its stream, and the server is not told of
///   any cancellation by it.
///
/// One pass over the events, cancelling at the close rather than comparing
/// sequence numbers against it: the close is a lifecycle event, so no message
/// can share its `seq`, and `<` versus `<=` would be untestable.
pub(in crate::checks) fn no_messages_after_cancellation(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let mut outstanding: BTreeSet<String> = BTreeSet::new();
    // Cancelled request id → the seq of the close that cancelled it.
    let mut cancelled: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for event in context.events() {
        if is_cancellation(event) {
            if outstanding.len() == 1
                && let Some(id) = outstanding.pop_first()
            {
                cancelled.insert(id, event.seq);
            }
            continue;
        }
        if !cancelled.is_empty() {
            report_after_close(event, &cancelled, sink);
        }
        track_outstanding(event, &mut outstanding, &mut cancelled);
    }
}

/// Whether `event` is the recorded form of a response stream closing.
fn is_cancellation(event: &mcp_conformance_core::trace::TraceEvent) -> bool {
    let closed = matches!(
        event.body,
        EventBody::Lifecycle {
            event: LifecycleEvent::TransportClose
        }
    );
    closed && event.transport == TransportKind::StreamableHttp
}

/// Opens an id on a request and closes it on the answer, so `outstanding` holds
/// exactly the ids in flight.
fn track_outstanding(
    event: &mcp_conformance_core::trace::TraceEvent,
    outstanding: &mut BTreeSet<String>,
    cancelled: &mut std::collections::BTreeMap<String, u64>,
) {
    let Some(payload) = event.message_payload() else {
        return;
    };
    let Some(id) = payload.get("id").filter(|id| !id.is_null()) else {
        return;
    };
    if payload.get("method").is_some() {
        // A new request reusing a cancelled id is a new request.
        cancelled.remove(&id.to_string());
        outstanding.insert(id.to_string());
    } else {
        outstanding.remove(&id.to_string());
    }
}

/// Reports a server message for a request whose stream was closed.
fn report_after_close(
    event: &mcp_conformance_core::trace::TraceEvent,
    cancelled: &std::collections::BTreeMap<String, u64>,
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
    // The subject is a server message carrying an id *after* the close: before
    // one, nothing is forbidden, and a session with no close is untested.
    sink.examined();
    if let Some(closed_at) = cancelled.get(&id.to_string()) {
        sink.push(
            Some(event.seq),
            format!(
                "server sent a further message for request id {id}, whose response \
                 stream closed at seq {closed_at}; a close is cancellation at this revision"
            ),
        );
    }
}
