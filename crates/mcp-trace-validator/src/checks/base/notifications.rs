// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `BASE-011` (`2025-11-25`) / `BASE-061` (`2026-07-28`): "The receiver MUST
//! NOT send a response" to a notification.
//!
//! A response to a notification has no id to echo, so the only form it can take
//! that the correlation checks do not already report is a response with a null
//! or absent id. `base.result-id-matches` and `base.error-id-matches` report a
//! response whose id names no outstanding request, whatever provoked it; they
//! leave a null-id *error* alone over stdio, because that is also the lawful
//! answer to a request whose id could not be read. This check judges the case
//! in which no such request exists.

use std::collections::HashSet;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, TransportKind};
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// What one party has sent, as far as the other's null-id responses go.
#[derive(Debug, Clone, Copy, Default)]
struct Sent<'a> {
    /// The party has sent something a null-id response may lawfully answer, or
    /// may be the only answer to: a message that is not well-formed JSON-RPC,
    /// or a request the trace never shows answered by id.
    spoiled: bool,
    /// The latest well-formed notification the party sent since the peer last
    /// sent anything: its `seq` and method.
    notification: Option<(u64, &'a str)>,
    /// Whether the party sent a response since the peer last sent anything.
    response: bool,
}

/// Whether a message is one no JSON-RPC receiver may call an invalid request:
/// `"jsonrpc": "2.0"`, a string or number id where it has one, and `params`
/// absent or an object.
fn well_formed(payload: &Value, kind: &MessageKind<'_>) -> bool {
    let params_ok = payload.get("params").is_none_or(Value::is_object);
    payload.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
        && match kind {
            MessageKind::Request { id, .. } => (id.is_string() || id.is_number()) && params_ok,
            MessageKind::Notification { .. } => params_ok,
            MessageKind::Result { .. } | MessageKind::Error { .. } => true,
            _ => false,
        }
}

/// The index of the party sending in `direction`, and of its peer.
const fn sides(direction: Direction) -> (usize, usize) {
    match direction {
        Direction::ClientToServer => (0, 1),
        Direction::ServerToClient => (1, 0),
    }
}

/// The names of the party sending in `direction`, and of its peer.
const fn parties(direction: Direction) -> (&'static str, &'static str) {
    match direction {
        Direction::ClientToServer => ("client", "server"),
        Direction::ServerToClient => ("server", "client"),
    }
}

/// `BASE-011` / `BASE-061`: no response to a notification, over stdio.
///
/// **Stimulus.** A notification that is well-formed JSON-RPC (`"jsonrpc":
/// "2.0"`, no id, `params` absent or an object) on a stdio stream, from a party
/// that up to then has sent nothing a null-id response could lawfully answer:
/// every message of its own was well-formed, and every request it sent is
/// answered *by id* somewhere in the trace (a request is answered once, so none
/// of them is what a null-id response answers). Each such notification is a
/// subject. The recorder writes a line that is not JSON into the trace as a
/// string payload, so a malformed message is visible here and ends the
/// judgment for its sender.
///
/// **Conforming.** The receiver sends nothing for it; whatever else it sends
/// carries a method or answers a request by id.
///
/// **Violation (fail).** The receiver's *next* message after the notification
/// is a result or error whose id is null or absent, and the sender sent no
/// response of its own in between (a reply to a response would be a different
/// defect). No request and no unreadable message exists for that response to
/// answer, so it answers the notification.
///
/// Streamable HTTP is not judged here: the answer to a notification sent by POST is
/// the transport's clause (`transport.accepted-input-status`, `202` with no
/// body), and that page lets a server that cannot accept the input put an
/// id-less JSON-RPC error in the body of an HTTP error status.
pub(in crate::checks) fn notification_unanswered(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let answered: HashSet<u64> = context
        .exchanges()
        .map(|exchange| exchange.request.seq)
        .collect();
    let mut sent = [Sent::default(); 2];
    for (event, kind, _) in context.messages() {
        if event.transport != TransportKind::Stdio {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let (own, peer) = sides(event.direction);
        if let MessageKind::Result { id } | MessageKind::Error { id, .. } = kind
            && id.is_none_or(Value::is_null)
            && let Sent {
                spoiled: false,
                notification: Some((seq, method)),
                response: false,
            } = sent[peer]
        {
            let (receiver, sender) = parties(event.direction);
            sink.push(
                Some(event.seq),
                format!(
                    "the {receiver} sent a response with no id as its next message after the \
                     {sender}'s `{method}` notification (seq {seq}); every request the {sender} \
                     had sent is answered by id and nothing it sent was malformed, so the \
                     response answers the notification"
                ),
            );
        }
        // Anything this party sends ends the peer's window: what the peer sent
        // before it is no longer what this party's next message follows.
        sent[peer].notification = None;
        sent[peer].response = false;
        let state = &mut sent[own];
        if !well_formed(payload, kind) {
            state.spoiled = true;
        }
        match kind {
            MessageKind::Request { .. } if !answered.contains(&event.seq) => state.spoiled = true,
            MessageKind::Result { .. } | MessageKind::Error { .. } => state.response = true,
            MessageKind::Notification { method } if !state.spoiled => {
                sink.examined();
                state.notification = Some((event.seq, method));
            }
            _ => {}
        }
    }
}
