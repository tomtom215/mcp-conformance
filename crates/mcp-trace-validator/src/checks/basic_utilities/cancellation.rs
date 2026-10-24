// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! basic/utilities/cancellation: what a `notifications/cancelled` may name,
//! from either party, and what its receiver owes it.
//!
//! At this revision either party issues requests and either may cancel its
//! own, so a cancellation's *direction* is part of every rule here: the ids it
//! may name are the ids its sender issued. Ids are compared as canonical JSON
//! text, so `"1"` and `1` are the different ids they are, and a request that
//! reuses an id is a new request — a cancellation is resolved against the
//! latest request its sender issued under that id.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::trace::{Direction, TraceEvent, TransportKind};
use serde_json::Value;

use super::super::FindingSink;
use super::{
    Party, answers, is_notification, notification_params, request_id, response_id, session_start,
};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The cancellation notification.
const CANCELLED: &str = "notifications/cancelled";

/// What a `notifications/cancelled` names.
enum Cancellation<'a> {
    /// The `params.requestId` it carries.
    Of(&'a Value),
    /// No request: `requestId` absent or null.
    OfNothing,
}

/// The cancellation `event` is, when it is one.
fn cancellation(event: &TraceEvent) -> Option<Cancellation<'_>> {
    if !is_notification(event, CANCELLED) {
        return None;
    }
    let named = notification_params(event, CANCELLED)
        .and_then(|params| params.get("requestId"))
        .filter(|id| !id.is_null());
    Some(named.map_or(Cancellation::OfNothing, Cancellation::Of))
}

/// `CANC-018`: a cancellation names a request its sender previously issued.
///
/// Judged only when the recording holds the session's `initialize` request: a
/// `2025-11-25` session begins with it (LIFE-001), so every request the session
/// issued afterwards is in the recording, and an id no request from the
/// cancelling party carried was never issued in that direction. Without the
/// handshake the capture may have begun mid-session and the request may
/// predate it, so nothing is judged. That witness is what the stateless
/// revision lacks, and why the same sentence (CANC-010) carries an exclusion
/// there.
///
/// The clause's second item — that the request is "believed to still be
/// in-progress" — is the sender's belief, and the page's own Timing
/// Considerations make a cancellation that crosses the answer on the wire
/// conforming; it is deliberately not judged. A cancellation naming no request
/// at all is reported: it references no request its sender issued.
pub(in crate::checks) fn references_issued_request(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let Some(start) = session_start(context) else {
        return;
    };
    // (sender, id text) of every request issued so far.
    let mut issued: BTreeSet<(Party, String)> = BTreeSet::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if let Some(id) = request_id(payload) {
            issued.insert((Party::of(event.direction), id));
            continue;
        }
        let Some(named) = cancellation(event) else {
            continue;
        };
        if event.seq < start {
            continue; // Before the session began: not this session's traffic.
        }
        // The subject is a cancellation sent in a session whose start the
        // recording holds.
        sink.examined();
        let sender = Party::of(event.direction);
        let party = sender.name();
        let Cancellation::Of(id) = named else {
            sink.push(
                Some(event.seq),
                format!(
                    "the {party}'s `{CANCELLED}` carries no `params.requestId`, so it \
                     references no request the {party} issued"
                ),
            );
            continue;
        };
        let text = to_canonical_string(id);
        if issued.contains(&(sender, text.clone())) {
            continue;
        }
        let aside = if issued.contains(&(sender.other(), text)) {
            format!(
                " (the {} issued a request with that id, and a cancellation may only name a \
                 request issued in its own direction)",
                sender.other().name()
            )
        } else {
            String::new()
        };
        sink.push(
            Some(event.seq),
            format!(
                "the {party} cancelled request {id}, which it never issued in this \
                 session{aside}; the session began with `initialize` at seq {start}, so every \
                 request it issued is in the recording"
            ),
        );
    }
}

/// `CANC-019`: the client does not cancel `initialize`.
///
/// Each client cancellation is resolved against the latest client request
/// carrying the id it names; one naming an id the recording never saw a client
/// request for cannot be shown to name `initialize`, and is not judged.
pub(in crate::checks) fn initialize_not_cancelled(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Id text → the method of the latest client request carrying it.
    let mut methods: BTreeMap<String, &str> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        if event.direction != Direction::ClientToServer {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if let (Some(id), Some(method)) = (
            request_id(payload),
            payload.get("method").and_then(Value::as_str),
        ) {
            methods.insert(id, method);
            continue;
        }
        let Some(Cancellation::Of(id)) = cancellation(event) else {
            continue;
        };
        let Some(method) = methods.get(&to_canonical_string(id)) else {
            continue;
        };
        // The subject is a client cancellation of a request the recording shows.
        sink.examined();
        if *method == "initialize" {
            sink.push(
                Some(event.seq),
                format!(
                    "the client cancelled its `initialize` request (id {id}); the `initialize` \
                     request must not be cancelled by clients"
                ),
            );
        }
    }
}

/// `CANC-020`: a task is cancelled with `tasks/cancel`, never by notification.
///
/// A request is a task's once the recording shows it answered with a
/// `CreateTaskResult` — a request that merely asked for a task may have been
/// processed as an ordinary one by a receiver without the capability, and
/// cancelling that by notification is proper. Only a cancellation sent after
/// some task of its sender's exists could misuse one, so those are the
/// subjects; the cancellation is resolved against the latest request its
/// sender issued under the id.
pub(in crate::checks) fn task_cancelled_by_request(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let answers = answers(context);
    // Request seq → (sender, id text).
    let mut requests: BTreeMap<u64, (Party, String)> = BTreeMap::new();
    // (sender, id text) → (latest request's seq, the task id it was answered with).
    let mut latest: BTreeMap<(Party, String), (u64, Option<String>)> = BTreeMap::new();
    // How many tasks each party's requests have created so far.
    let mut tasks: BTreeMap<Party, u32> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if let Some(request) = answers.get(&event.seq) {
            let created = payload
                .get("result")
                .and_then(|result| result.get("task")?.get("taskId")?.as_str());
            if let (Some(task_id), Some(key)) = (created, requests.get(request))
                && let Some(entry) = latest.get_mut(key)
                && entry.0 == *request
            {
                entry.1 = Some(task_id.to_owned());
                *tasks.entry(key.0).or_default() += 1;
            }
            continue;
        }
        let sender = Party::of(event.direction);
        if let Some(id) = request_id(payload) {
            requests.insert(event.seq, (sender, id.clone()));
            latest.insert((sender, id), (event.seq, None));
            continue;
        }
        let Some(named) = cancellation(event) else {
            continue;
        };
        if tasks.get(&sender).copied().unwrap_or_default() == 0 {
            continue; // No task of this party's exists yet: nothing to misuse.
        }
        // The subject is a cancellation sent once a task of its sender's exists.
        sink.examined();
        let Cancellation::Of(id) = named else {
            continue;
        };
        if let Some((_, Some(task_id))) = latest.get(&(sender, to_canonical_string(id))) {
            sink.push(
                Some(event.seq),
                format!(
                    "the {} cancelled request {id} by notification, and that request was \
                     answered with task {task_id:?}; a task is cancelled with `tasks/cancel`",
                    sender.name()
                ),
            );
        }
    }
}

/// `CANC-021` (the wire-visible item): no answer for a request once its
/// cancellation has been read.
///
/// A recording orders what crossed the channel, not what the receiver had
/// read: an answer written before the cancellation was read lands after it in
/// the trace, and the page names that race outright ("cancellation
/// notifications may arrive after request processing has completed").
/// Convicting every later answer would report both parties for a race the
/// specification expects. stdio delivers one party's messages in order, so
/// the receiver has demonstrably read a cancellation once it answers a request
/// the sender issued *after* it; from then on, an answer to the cancelled
/// request is one it sent knowing. Before that point the trace cannot tell the
/// two apart and nothing is reported. Streamable HTTP gives no such order (a
/// later POST may be served before an earlier one), so it is not judged.
///
/// Stopping work and freeing resources, the clause's other items, are internal
/// and leave no message. Both directions are judged: a server answering a
/// call the client cancelled, and a client answering a `sampling/createMessage`
/// or `roots/list` the server withdrew.
pub(in crate::checks) fn no_response_after_cancellation(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for cancelling in [Direction::ClientToServer, Direction::ServerToClient] {
        judge_receiver(context, cancelling, sink);
    }
}

/// The half of `CANC-021` in which `cancelling` sends the requests and the
/// cancellations, and the other party answers.
fn judge_receiver(context: &TraceContext<'_>, cancelling: Direction, sink: &mut FindingSink) {
    // Id text → the seq of the latest request the cancelling party sent under it.
    let mut sent_at: BTreeMap<String, u64> = BTreeMap::new();
    // Cancelled id → (seq of the cancellation, whether the receiver has
    // demonstrably read it).
    let mut cancelled: BTreeMap<String, (u64, bool)> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if event.direction == cancelling {
            if let Some(Cancellation::Of(id)) = cancellation(event) {
                cancelled
                    .entry(to_canonical_string(id))
                    .or_insert((event.seq, false));
            } else if let Some(id) = request_id(payload) {
                // A request reusing an id is a new request: a cancellation
                // recorded under that id belonged to the one before it.
                cancelled.remove(&id);
                sent_at.insert(id, event.seq);
            }
            continue;
        }
        if event.transport != TransportKind::Stdio || cancelled.is_empty() {
            continue;
        }
        let Some(id) = response_id(payload) else {
            continue;
        };
        let Some(&answered_at) = sent_at.get(&id) else {
            continue; // Answering nothing the cancelling party sent: BASE-004's.
        };
        // The subject is an answer the receiver sends while a cancellation
        // stands: it may or may not be for the cancelled request.
        sink.examined();
        if let Some(&(at, true)) = cancelled.get(&id) {
            sink.push(
                Some(event.seq),
                format!(
                    "the {} answered request {id}, which the {} cancelled at seq {at} — after \
                     answering a request sent later than that cancellation, so after reading it",
                    Party::of(event.direction).name(),
                    Party::of(cancelling).name()
                ),
            );
        }
        for (cancelled_at, read) in cancelled.values_mut() {
            *read |= *cancelled_at < answered_at;
        }
    }
}
