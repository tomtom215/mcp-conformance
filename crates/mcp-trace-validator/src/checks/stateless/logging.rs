// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Logging at `2026-07-28`, where the level rides each request's `_meta`.
//!
//! `logging/setLevel` is gone: a client asks for log messages by putting
//! `io.modelcontextprotocol/logLevel` in a request's `_meta`, and the server may
//! answer with `notifications/message` on that request's response stream and
//! nowhere else. The feature is deprecated as of this revision (SEP-2577), which
//! changes nothing about how its remaining clauses are judged.
//!
//! The capability declaration for logging lives with the other four in
//! [`super::capabilities`], because all five state one rule per feature page and
//! all five need this revision's declaration surface.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The `_meta` key a request sets to opt into log messages.
const LOG_LEVEL: &str = "io.modelcontextprotocol/logLevel";

/// The log notification itself.
const MESSAGE: &str = "notifications/message";

/// Whether `event` is a server `notifications/message`.
fn is_log(event: &TraceEvent, kind: &MessageKind<'_>) -> bool {
    event.direction == Direction::ServerToClient
        && matches!(kind, MessageKind::Notification { method } if *method == MESSAGE)
}

/// `LOG-008`: `notifications/message` only for a request that asked for it.
///
/// The level rides `_meta.io.modelcontextprotocol/logLevel` on the request, and a
/// log notification belongs to the request that set it — but carries nothing
/// naming that request. Two readings are sound, and both are judged:
///
/// - If *no* request in the session set a level, no `notifications/message` may
///   appear at all.
/// - If exactly one client request is in flight when the notification arrives
///   (sent, not yet answered; a `subscriptions/listen` stream does not count, as
///   LOG-009 forbids logs on it), the notification can only be that request's,
///   so that request must have set a level. This reaches a session in which
///   *some* request asked for logs and the server logged against another one,
///   which the first reading alone passed. With several requests in flight the
///   notification is attributable to none, and is not judged further.
pub(in crate::checks) fn level_requested(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let any_requested = context.messages().any(|(event, _, _)| {
        event.direction == Direction::ClientToServer
            && event.message_payload().is_some_and(requests_logs)
    });
    for Log {
        event, in_flight, ..
    } in logs(context)
    {
        // The subject is an emitted log notification: a session in which the
        // server never logged puts nothing to this test, whether or not a
        // request asked for logs.
        sink.examined();
        if !any_requested {
            sink.push(
                Some(event.seq),
                "server emitted `notifications/message` though no request in this session \
                 carried `io.modelcontextprotocol/logLevel`"
                    .to_owned(),
            );
        } else if in_flight.len() == 1
            && let Some((request_seq, false)) = in_flight.values().next().copied()
        {
            sink.push(
                Some(event.seq),
                format!(
                    "server emitted `notifications/message` while the only request in flight \
                     (seq {request_seq}) carried no `io.modelcontextprotocol/logLevel`"
                ),
            );
        }
    }
}

/// Whether a client message's `_meta` opts into log messages.
fn requests_logs(payload: &Value) -> bool {
    payload
        .get("params")
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get(LOG_LEVEL))
        .is_some()
}

/// A server log notification, and what the client had in flight when it arrived.
struct Log<'a> {
    /// The `notifications/message` event.
    event: &'a TraceEvent,
    /// Client requests in flight, `subscriptions/listen` aside: id text →
    /// (seq, whether it set a level).
    in_flight: BTreeMap<String, (u64, bool)>,
    /// Whether a later response answers a request the recording does not
    /// carry — one sent before the recording began, which may have been in
    /// flight here unseen.
    unrecorded_request_answered: bool,
}

/// Every server log notification in the trace, in order.
fn logs<'a>(context: &'a TraceContext<'_>) -> Vec<Log<'a>> {
    let mut logs: Vec<Log<'a>> = Vec::new();
    let mut in_flight: BTreeMap<String, (u64, bool)> = BTreeMap::new();
    // Every id a recorded client request carried, answered or not.
    let mut recorded: BTreeSet<String> = BTreeSet::new();
    for (event, kind, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if event.direction == Direction::ClientToServer {
            if let MessageKind::Request { method, id } = kind {
                recorded.insert(id.to_string());
                if *method != "subscriptions/listen" {
                    in_flight.insert(id.to_string(), (event.seq, requests_logs(payload)));
                }
            }
            continue;
        }
        if let MessageKind::Result { id: Some(id) } | MessageKind::Error { id: Some(id), .. } = kind
        {
            in_flight.remove(&id.to_string());
            if !recorded.contains(&id.to_string()) {
                for log in &mut logs {
                    log.unrecorded_request_answered = true;
                }
            }
            continue;
        }
        if is_log(event, kind) {
            logs.push(Log {
                event,
                in_flight: in_flight.clone(),
                unrecorded_request_answered: false,
            });
        }
    }
    logs
}

/// `LOG-009`: log notifications stay on the response stream of the request
/// that set the level.
///
/// Two deliveries are attributable from a trace, and both are judged:
///
/// - A `notifications/message` tagged with `io.modelcontextprotocol/subscriptionId`
///   is by definition travelling on a subscription's stream, which the clause
///   names: the subscription stream carries the response to a different request
///   entirely.
/// - A `notifications/message` that arrives while no request is in flight
///   (`subscriptions/listen` aside, which the first case covers) is on no
///   request's response stream at all — every request it could belong to has
///   already been answered — so it is on "any stream other than the one
///   carrying the response to the request that set the log level". A response
///   later in the trace to a request the recording never carried withdraws the
///   judgment: that request was in flight, unseen, and the log may be its.
pub(in crate::checks) fn not_on_subscription(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for log in logs(context) {
        sink.examined();
        let tagged = log
            .event
            .message_payload()
            .and_then(|payload| payload.get("params"))
            .and_then(|params| params.get("_meta"))
            .and_then(|meta| meta.get("io.modelcontextprotocol/subscriptionId"))
            .is_some_and(|id| !id.is_null());
        if tagged {
            sink.push(
                Some(log.event.seq),
                "`notifications/message` carries a subscription id, so it is travelling on a \
                 `subscriptions/listen` stream; logging is request-scoped"
                    .to_owned(),
            );
        } else if log.in_flight.is_empty() && !log.unrecorded_request_answered {
            sink.push(
                Some(log.event.seq),
                "`notifications/message` arrived with no request but a `subscriptions/listen` \
                 in flight, so it is on no request's response stream; logging is request-scoped"
                    .to_owned(),
            );
        }
    }
}

/// `LOG-010`: an unrecognized log level draws `-32602`.
///
/// The levels are RFC 5424's eight, which the page lists as the complete set.
pub(in crate::checks) fn invalid_level_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    const LEVELS: &[&str] = &[
        "debug",
        "info",
        "notice",
        "warning",
        "error",
        "critical",
        "alert",
        "emergency",
    ];
    for exchange in context.exchanges() {
        let Some(level) = exchange
            .params
            .and_then(|params| params.get("_meta"))
            .and_then(|meta| meta.get(LOG_LEVEL))
        else {
            continue;
        };
        if level.as_str().is_some_and(|name| LEVELS.contains(&name)) {
            continue;
        }
        // The subject is a request that named a level outside the eight; a
        // session that only ever named valid ones leaves this clause untested.
        sink.examined();
        let code = exchange
            .response
            .message_payload()
            .and_then(|payload| payload.get("error"))
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        if code != Some(-32602) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the request declared log level {level}, which is not one of the eight \
                     RFC 5424 levels, and drew {} rather than -32602",
                    code.map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
                ),
            );
        }
    }
}
