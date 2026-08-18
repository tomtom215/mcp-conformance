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

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
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
fn is_log(event: &mcp_conformance_core::trace::TraceEvent, kind: &MessageKind<'_>) -> bool {
    event.direction == Direction::ServerToClient
        && matches!(kind, MessageKind::Notification { method } if *method == MESSAGE)
}

/// `LOG-008`: `notifications/message` only for a request that asked for it.
///
/// The level rides `_meta.io.modelcontextprotocol/logLevel` on the request, and a
/// log notification belongs to the response stream of the request that set it.
/// On a recording this is judged by the one thing that survives: if *no* request
/// in the session set a log level, no `notifications/message` may appear at all.
/// Attributing a notification to a particular request is not possible on stdio,
/// where one channel carries everything, so a session with at least one
/// level-setting request is not judged further.
pub(in crate::checks) fn level_requested(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let any_requested = context.messages().any(|(event, _, _)| {
        event.direction == Direction::ClientToServer
            && event.message_payload().is_some_and(|payload| {
                payload
                    .get("params")
                    .and_then(|params| params.get("_meta"))
                    .and_then(|meta| meta.get(LOG_LEVEL))
                    .is_some()
            })
    });
    for (event, kind, _) in context.messages() {
        if !is_log(event, kind) {
            continue;
        }
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
        }
    }
}

/// `LOG-009`: log notifications stay off a `subscriptions/listen` stream.
///
/// A `notifications/message` tagged with `io.modelcontextprotocol/subscriptionId`
/// is by definition travelling on a subscription's stream, which this clause
/// forbids: logging is request-scoped, and the subscription stream carries the
/// response to a different request entirely.
pub(in crate::checks) fn not_on_subscription(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        if !is_log(event, kind) {
            continue;
        }
        sink.examined();
        let tagged = event
            .message_payload()
            .and_then(|payload| payload.get("params"))
            .and_then(|params| params.get("_meta"))
            .and_then(|meta| meta.get("io.modelcontextprotocol/subscriptionId"))
            .is_some_and(|id| !id.is_null());
        if tagged {
            sink.push(
                Some(event.seq),
                "`notifications/message` carries a subscription id, so it is travelling on a \
                 `subscriptions/listen` stream; logging is request-scoped"
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
