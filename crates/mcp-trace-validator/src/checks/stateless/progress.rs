// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! basic/patterns/progress: the token a client opts in with, and the
//! notifications a server sends against it.
//!
//! A client asks for progress by putting `progressToken` in a request's
//! `_meta`; the server answers with `notifications/progress` carrying that
//! token. Both sides of that exchange are on the wire, so the page's MUSTs are
//! judged here — the token's type and uniqueness on the client's side, the
//! value's growth and the notifications' lifetime on the server's. Tokens are
//! compared as canonical JSON text, so the string `"1"` and the integer `1` are
//! different tokens, as they are different JSON values.

use std::collections::BTreeMap;

use mcp_conformance_core::trace::{Direction, TraceEvent, TransportKind};
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The progress notification.
const PROGRESS: &str = "notifications/progress";

/// A client request's `_meta.progressToken`, when it is a request carrying one.
fn requested_token(event: &TraceEvent) -> Option<&Value> {
    if event.direction != Direction::ClientToServer {
        return None;
    }
    let payload = event.message_payload()?;
    payload.get("method")?;
    payload.get("id").filter(|id| !id.is_null())?;
    payload.get("params")?.get("_meta")?.get("progressToken")
}

/// A server progress notification's `params`, when the event is one.
fn progress_params(event: &TraceEvent) -> Option<&Value> {
    if event.direction != Direction::ServerToClient {
        return None;
    }
    let payload = event.message_payload()?;
    if payload.get("method")?.as_str()? != PROGRESS {
        return None;
    }
    payload.get("params")
}

/// For each client request the trace saw answered, the `seq` of its answer.
fn answered_at(context: &TraceContext<'_>) -> BTreeMap<u64, u64> {
    context
        .exchanges()
        .filter(|exchange| exchange.request.direction == Direction::ClientToServer)
        .map(|exchange| (exchange.request.seq, exchange.response.seq))
        .collect()
}

/// `PROG-001`: a progress token is a string or an integer.
pub(in crate::checks) fn token_type(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        let Some(token) = requested_token(event) else {
            continue;
        };
        sink.examined();
        if !(token.is_string() || token.is_i64() || token.is_u64()) {
            sink.push(
                Some(event.seq),
                format!("request `_meta.progressToken` is {token}, not a string or integer"),
            );
        }
    }
}

/// `PROG-002`: no two active requests share a progress token.
///
/// "Active" is read the way BASE-045 reads an in-flight id: from the request
/// until its answer. A request the recording never sees answered stays active
/// to the end, because nothing shows it finished.
pub(in crate::checks) fn token_unique_in_flight(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let answered = answered_at(context);
    // Active requests carrying a token, as (token text, request seq).
    let mut active: Vec<(String, u64)> = Vec::new();
    for (event, _, _) in context.messages() {
        let Some(token) = requested_token(event) else {
            continue;
        };
        sink.examined();
        active.retain(|(_, request)| {
            answered
                .get(request)
                .is_none_or(|answer| *answer > event.seq)
        });
        let token = token.to_string();
        if let Some((_, earlier)) = active.iter().find(|(held, _)| *held == token) {
            sink.push(
                Some(event.seq),
                format!(
                    "progress token {token} is already held by the request at seq {earlier}, \
                     which has not been answered; tokens must be unique across active requests"
                ),
            );
        }
        active.push((token, event.seq));
    }
}

/// `PROG-003`: each notification's `progress` exceeds the last for its token.
///
/// The sequence restarts when a new request takes the token up: a token may be
/// reused once its request is done, and the new operation's progress owes
/// nothing to the old one's.
pub(in crate::checks) fn value_increases(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // Token text → the last `progress` value sent against it.
    let mut last: BTreeMap<String, f64> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        if let Some(token) = requested_token(event) {
            last.remove(&token.to_string());
            continue;
        }
        let Some(params) = progress_params(event) else {
            continue;
        };
        let (Some(token), Some(progress)) = (
            params.get("progressToken"),
            params.get("progress").and_then(Value::as_f64),
        ) else {
            continue;
        };
        let token = token.to_string();
        if let Some(previous) = last.insert(token.clone(), progress) {
            // The subject is a notification with a predecessor to exceed.
            sink.examined();
            if progress <= previous {
                sink.push(
                    Some(event.seq),
                    format!(
                        "`progress` for token {token} went from {previous} to {progress}; it \
                         must increase with each notification"
                    ),
                );
            }
        }
    }
}

/// `PROG-005` and `PROG-008`: no progress for a request already answered.
///
/// Judged on stdio only, where one ordered channel carries both the notification
/// and the answer, so a notification recorded after the answer was written after
/// it. Streamable HTTP can carry them on different streams, whose relative
/// order in a recording is the order they were captured, not sent — the same
/// ambiguity the framing module refuses to resolve by guess.
///
/// A token no recorded request carried is not reported: the session may have
/// begun before the capture did.
pub(in crate::checks) fn no_progress_after_completion(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let answered = answered_at(context);
    // Token text → the latest request that took it up.
    let mut owner: BTreeMap<String, u64> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        if let Some(token) = requested_token(event) {
            owner.insert(token.to_string(), event.seq);
            continue;
        }
        if event.transport != TransportKind::Stdio {
            continue;
        }
        let Some(token) = progress_params(event).and_then(|params| params.get("progressToken"))
        else {
            continue;
        };
        let token = token.to_string();
        let Some(request) = owner.get(&token) else {
            continue;
        };
        sink.examined();
        if let Some(answer) = answered.get(request).filter(|answer| **answer < event.seq) {
            sink.push(
                Some(event.seq),
                format!(
                    "progress for token {token} after its request (seq {request}) was answered \
                     at seq {answer}; progress notifications must stop after completion"
                ),
            );
        }
    }
}
