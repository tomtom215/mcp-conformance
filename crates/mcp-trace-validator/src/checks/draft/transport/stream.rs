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

/// `TRAN-060`: clients do not POST JSON-RPC responses.
pub(in crate::checks) fn client_no_responses(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        if event.direction != Direction::ClientToServer
            || event.transport != TransportKind::StreamableHttp
        {
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
                "client POSTed a JSON-RPC response, which 2026-07-28 forbids on this transport"
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
pub(in crate::checks) fn no_messages_after_cancellation(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let Some(closed_at) = cancellation_seq(context) else {
        return;
    };
    let mut outstanding: BTreeSet<String> = BTreeSet::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(id) = payload.get("id").filter(|id| !id.is_null()) else {
            continue;
        };
        if event.seq < closed_at {
            if payload.get("method").is_some() {
                outstanding.insert(id.to_string());
            } else {
                outstanding.remove(&id.to_string());
            }
        } else if event.direction == Direction::ServerToClient
            && outstanding.contains(&id.to_string())
        {
            sink.push(
                Some(event.seq),
                format!(
                    "server sent a further message for request id {id}, whose response \
                     stream closed at seq {closed_at}; a close is cancellation at this revision"
                ),
            );
        }
    }
}

/// The `seq` at which the recorded Streamable HTTP transport closed, if it did.
fn cancellation_seq(context: &TraceContext<'_>) -> Option<u64> {
    context.events().iter().find_map(|event| {
        let closed = matches!(
            event.body,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose | LifecycleEvent::TransportAbort
            }
        );
        (closed && event.transport == TransportKind::StreamableHttp).then_some(event.seq)
    })
}
