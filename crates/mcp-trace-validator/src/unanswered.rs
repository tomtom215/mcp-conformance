// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which requests a trace ended without answering
//! ([`UnansweredRequests`], the report's note).
//!
//! A request is answered when the context pairs a response with it
//! ([`TraceContext::exchanges`]): by id from the other party, after the
//! request, or — for a null-id answer — by the HTTP framing. So an answer that
//! arrives later than another request's, out of order, still answers; an
//! answer streamed over SSE is a message event like any other and pairs the
//! same way; and a request the HTTP layer refused before reading it (`401`,
//! `403`) is not a message the protocol saw, and is not listed. An HTTP `202`
//! with no body answers a notification, which has no id and is never a
//! request; a *request* a server meets with `202` and nothing else is listed,
//! because nothing answered it.
//!
//! A request its sender cancelled (`notifications/cancelled` naming its id,
//! sent after it) is not listed either: the sender withdrew the question.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a private module; this follows the rustc
// lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::BTreeSet;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TraceEvent};
use serde_json::Value;

use crate::context::TraceContext;
use crate::report::{LISTED_UNANSWERED, Party, TraceEnding, UnansweredRequest, UnansweredRequests};

/// The requests `context` ends without answering, or `None` when every
/// request was answered or cancelled.
pub(crate) fn find(context: &TraceContext<'_>) -> Option<UnansweredRequests> {
    let answered: BTreeSet<u64> = context
        .exchanges()
        .map(|exchange| exchange.request.seq)
        .collect();
    let cancellations = cancellations(context);
    let mut unanswered = Vec::new();
    for (event, kind, _) in context.messages() {
        let MessageKind::Request { method, id } = kind else {
            continue;
        };
        if answered.contains(&event.seq) || is_cancelled(event, id, &cancellations) {
            continue;
        }
        unanswered.push(UnansweredRequest {
            method: (*method).to_owned(),
            id: (*id).clone(),
            seq: event.seq,
            sender: Party::sender(event.direction),
        });
    }
    if unanswered.is_empty() {
        return None;
    }
    let count = u32::try_from(unanswered.len()).unwrap_or(u32::MAX);
    unanswered.truncate(LISTED_UNANSWERED);
    Some(UnansweredRequests {
        count,
        ending: ending(context.events()),
        requests: unanswered,
    })
}

/// Every `notifications/cancelled`: who sent it, the id it names (canonical),
/// and its `seq`.
fn cancellations(context: &TraceContext<'_>) -> Vec<(Direction, String, u64)> {
    context
        .messages()
        .filter_map(|(event, kind, _)| match kind {
            MessageKind::Notification { method } if *method == "notifications/cancelled" => {
                let id = event.message_payload()?.get("params")?.get("requestId")?;
                Some((event.direction, to_canonical_string(id), event.seq))
            }
            _ => None,
        })
        .collect()
}

/// Whether the request at `event`, with `id`, was cancelled by its sender
/// after it was sent.
fn is_cancelled(
    event: &TraceEvent,
    id: &Value,
    cancellations: &[(Direction, String, u64)],
) -> bool {
    let id = to_canonical_string(id);
    cancellations
        .iter()
        .any(|(by, named, at)| *by == event.direction && *named == id && *at > event.seq)
}

/// How the trace ended: what its last event is.
fn ending(events: &[TraceEvent]) -> TraceEnding {
    match events.last().map(|event| &event.body) {
        Some(EventBody::Lifecycle {
            event: LifecycleEvent::TransportClose,
        }) => TraceEnding::TransportClose,
        Some(EventBody::Lifecycle {
            event: LifecycleEvent::TransportAbort,
        }) => TraceEnding::TransportAbort,
        _ => TraceEnding::Stopped,
    }
}

#[cfg(test)]
mod tests;
