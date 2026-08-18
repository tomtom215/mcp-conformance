// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `2026-07-28` stdio binding's own clauses: cancellation by notification.
//!
//! stdio has no per-request stream to close, so where Streamable HTTP signals
//! cancellation by closing the response stream (TRAN-069/070, [`super::stream`]),
//! stdio signals it with `notifications/cancelled`. The two are the same
//! protocol rule reached by different evidence, which is exactly why they cannot
//! share a check: [`super::stream::no_messages_after_cancellation`] anchors on a
//! `transport-close` lifecycle event and would inspect nothing at all on a stdio
//! capture — a vacuous pass rather than a judgement.
//!
//! The rest of the stdio page reuses checks that already exist
//! (`transport.stdio-{server-output,client-input}-valid`,
//! `transport.client-no-responses`, `transport.no-independent-server-requests`,
//! `discover.dual-era-probe-first`) or carries exclusions: `stderr`, process
//! exit and stream closure are host-OS events the trace vocabulary does not
//! record.

use std::collections::BTreeMap;

use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

use super::super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The cancellation notification.
const CANCELLED: &str = "notifications/cancelled";

/// The progress notification, the other message that can be "for" a request.
const PROGRESS: &str = "notifications/progress";

/// A client notification's method, when it has one.
fn client_notification(event: &TraceEvent) -> Option<(&str, Option<&Value>)> {
    if event.direction != Direction::ClientToServer {
        return None;
    }
    let payload = event.message_payload()?;
    if payload.get("id").is_some_and(|id| !id.is_null()) {
        return None;
    }
    let method = payload.get("method")?.as_str()?;
    Some((method, payload.get("params")))
}

/// `TRAN-123`: a cancellation names the request it cancels.
///
/// Only the shape is judged — that `params.requestId` is there — because that is
/// what the clause states beyond the notification's existence. Whether the named
/// id was still in flight is deliberately not reported: a recording that begins
/// mid-session, or ends before the answer, would make a conforming client look
/// like it cancelled a request that never existed.
///
/// The other half of the clause, that a client wanting to cancel *sends* one of
/// these, has no falsifier: the intent to cancel produces no other message on
/// stdio, so a session with no cancellation is indistinguishable from one where
/// nothing was cancelled.
pub(in crate::checks) fn cancel_notification_references_request(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for event in context.events() {
        let Some((method, params)) = client_notification(event) else {
            continue;
        };
        if method != CANCELLED {
            continue;
        }
        sink.examined();
        let names_a_request = params
            .and_then(|params| params.get("requestId"))
            .is_some_and(|id| !id.is_null());
        if !names_a_request {
            sink.push(
                Some(event.seq),
                format!(
                    "`{CANCELLED}` carries no `params.requestId`, so it names no request \
                     to cancel"
                ),
            );
        }
    }
}

/// `TRAN-124`: nothing further is sent for a request after it is cancelled.
///
/// Two ways a later server message can be "for" the cancelled request, and both
/// are judged: a response correlated by JSON-RPC `id`, and a
/// `notifications/progress` correlated by the `progressToken` the request itself
/// carried in `_meta`. Stopping at the first would leave the clause's "any
/// further messages" covering only the answer the server had probably already
/// decided not to send.
///
/// Cancellations of ids the trace never opened are still honoured, because the
/// prohibition is on the id, not on this recording having witnessed its request.
pub(in crate::checks) fn no_messages_after_cancel_notification(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Request id (canonical text) → the progress token it opted into, if any.
    let mut tokens: BTreeMap<String, String> = BTreeMap::new();
    // Cancelled request id → the seq of the notification that cancelled it. An
    // id enters this map at the cancellation and is judged only for what comes
    // *after*, which is the ordering the clause states — expressed by when the
    // entry appears rather than by comparing sequence numbers later. A
    // cancellation is a client notification and the messages judged are
    // server-sent, so `seq > cancelled_at` and `seq >= cancelled_at` would be
    // the same rule, and no trace could tell them apart.
    let mut cancelled: BTreeMap<String, u64> = BTreeMap::new();
    for event in context.events() {
        if let Some((method, params)) = client_notification(event) {
            if method == CANCELLED
                && let Some(id) = params.and_then(|params| params.get("requestId"))
                && !id.is_null()
            {
                cancelled.entry(id.to_string()).or_insert(event.seq);
            }
            continue;
        }
        if event.direction == Direction::ClientToServer {
            record_progress_token(event, &mut tokens);
            continue;
        }
        if cancelled.is_empty() {
            continue; // Nothing has been cancelled yet, so nothing is forbidden.
        }
        // The subject is a server message sent while a cancellation stands: it
        // may or may not belong to the cancelled request, and the trace shows
        // which.
        sink.examined();
        report_if_cancelled(event, &tokens, &cancelled, sink);
    }
}

/// Remembers the `_meta.progressToken` a client request opted into.
fn record_progress_token(event: &TraceEvent, tokens: &mut BTreeMap<String, String>) {
    let Some(payload) = event.message_payload() else {
        return;
    };
    let (Some(id), Some(token)) = (
        payload.get("id").filter(|id| !id.is_null()),
        payload
            .get("params")
            .and_then(|params| params.get("_meta"))
            .and_then(|meta| meta.get("progressToken")),
    ) else {
        return;
    };
    tokens.insert(id.to_string(), token.to_string());
}

/// Reports a server message that belongs to an already-cancelled request.
fn report_if_cancelled(
    event: &TraceEvent,
    tokens: &BTreeMap<String, String>,
    cancelled: &BTreeMap<String, u64>,
    sink: &mut FindingSink,
) {
    let Some(payload) = event.message_payload() else {
        return;
    };
    // A response: no `method`, and a non-null `id` naming what it answers.
    let answered = payload
        .get("method")
        .is_none()
        .then(|| payload.get("id").filter(|id| !id.is_null()))
        .flatten()
        .map(ToString::to_string);
    let progressed = (payload.get("method").and_then(Value::as_str) == Some(PROGRESS))
        .then(|| payload.get("params")?.get("progressToken"))
        .flatten()
        .map(ToString::to_string)
        .and_then(|token| {
            tokens
                .iter()
                .find_map(|(id, opted)| (*opted == token).then(|| id.clone()))
        });
    for (id, what) in [(answered, "a response"), (progressed, "progress")] {
        let Some(id) = id else { continue };
        let Some(&cancelled_at) = cancelled.get(&id) else {
            continue;
        };
        sink.push(
            Some(event.seq),
            format!(
                "server sent {what} for request {id}, which the client cancelled at \
                 seq {cancelled_at}"
            ),
        );
    }
}
