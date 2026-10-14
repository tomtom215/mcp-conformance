// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `subscriptions/listen`: the long-lived notification stream that replaced
//! `resources/subscribe` and the HTTP GET endpoint.
//!
//! Every message belonging to a subscription carries the `subscriptions/listen`
//! request's own JSON-RPC id in `_meta.io.modelcontextprotocol/subscriptionId`,
//! which is what makes these checks possible on stdio, where every subscription
//! shares one channel. They correlate on that field — never on message order
//! alone, because the specification explicitly permits other subscriptions'
//! messages to interleave. Order decides only *which* subscription an id names:
//! the one opened by the latest request carrying it and not yet ended.
//!
//! Three of the page's seven clauses carry exclusions: two bind what the client
//! does with what it received (compare the filter, demultiplex the stream), and
//! one begins after a reconnection, which is a second recording.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::{Map, Value};

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The request that opens a subscription.
const LISTEN: &str = "subscriptions/listen";

/// The `_meta` key tying a message to its subscription.
const SUBSCRIPTION_ID: &str = "io.modelcontextprotocol/subscriptionId";

/// The acknowledgment that must open every subscription.
const ACKNOWLEDGED: &str = "notifications/subscriptions/acknowledged";

/// The notification that ends a subscription on stdio.
const CANCELLED: &str = "notifications/cancelled";

/// Each notification type, and the filter field that requests it.
const FILTERED: &[(&str, &str)] = &[
    ("notifications/tools/list_changed", "toolsListChanged"),
    ("notifications/prompts/list_changed", "promptsListChanged"),
    (
        "notifications/resources/list_changed",
        "resourcesListChanged",
    ),
];

/// The filter field listing the resource URIs whose updates were requested.
const RESOURCE_SUBSCRIPTIONS: &str = "resourceSubscriptions";

/// The notification type `RESOURCE_SUBSCRIPTIONS` governs.
const RESOURCE_UPDATED: &str = "notifications/resources/updated";

/// The tasks extension's notification, and the filter field that requests it
/// for the tasks it lists (`io.modelcontextprotocol/tasks`, ADR-0023). An
/// extension may add a filter field, and this one is how a client asks for a
/// task's status updates; without it a conforming tasks session read as a
/// server sending a type nobody requested.
const TASK_STATUS: &str = "notifications/tasks";

/// The filter field listing the tasks whose updates were requested.
const TASK_IDS: &str = "taskIds";

/// An open subscription: the request that opened it, and its filter.
#[derive(Debug, Clone, Copy)]
struct Subscription<'a> {
    /// The `subscriptions/listen` request's `seq`, which tells one opening of
    /// an id apart from a later request reusing it.
    seq: u64,
    /// The `notifications` filter, when the request carried one.
    filter: Option<&'a Map<String, Value>>,
}

/// A server message tagged with a subscription id, as `(seq, id text, method,
/// params)`.
///
/// Both notifications and the closing response are tagged, so the method is
/// `None` for the response — which is exactly what distinguishes it.
type Tagged<'a> = (u64, String, Option<&'a str>, &'a Value);

/// Every tagged server message, with the subscription its tag named when it
/// was sent.
///
/// A subscription is open from its `subscriptions/listen` request until the
/// response to it or a `notifications/cancelled` naming it — the client's ends
/// it on stdio, the server's tears it down. Once it has ended, a later request
/// may carry the same id, so a tag resolves against the subscription open *at
/// that point* in the trace, never against whichever request last carried the
/// id. A tag naming no open subscription — one opened before the recording
/// began, or already ended — resolves to nothing and is not judged.
fn resolved<'a>(context: &'a TraceContext<'_>) -> Vec<(Subscription<'a>, Tagged<'a>)> {
    let mut open: BTreeMap<String, Subscription<'a>> = BTreeMap::new();
    let mut resolved = Vec::new();
    for (event, kind, _) in context.messages() {
        // Resolved before the message can close anything: the closing response
        // and the server's teardown both belong to the subscription they end.
        if let Some(tagged) = tagged_message(event)
            && let Some(subscription) = open.get(&tagged.1)
        {
            resolved.push((*subscription, tagged));
        }
        let params = event
            .message_payload()
            .and_then(|payload| payload.get("params"));
        match (event.direction, kind) {
            (Direction::ClientToServer, MessageKind::Request { method: LISTEN, id })
                if !id.is_null() =>
            {
                let filter = params
                    .and_then(|params| params.get("notifications"))
                    .and_then(Value::as_object);
                let seq = event.seq;
                open.insert(id.to_string(), Subscription { seq, filter });
            }
            (
                Direction::ServerToClient,
                MessageKind::Result { id: Some(id) } | MessageKind::Error { id: Some(id), .. },
            ) => {
                open.remove(&id.to_string());
            }
            (_, MessageKind::Notification { method: CANCELLED }) => {
                if let Some(request) = params.and_then(|params| params.get("requestId")) {
                    open.remove(&request.to_string());
                }
            }
            _ => {}
        }
    }
    resolved
}

/// The subscription tag one server message carries, if any.
fn tagged_message(event: &TraceEvent) -> Option<Tagged<'_>> {
    if event.direction != Direction::ServerToClient {
        return None;
    }
    let payload = event.message_payload()?;
    let params = payload.get("params").or_else(|| payload.get("result"))?;
    let id = params.get("_meta")?.get(SUBSCRIPTION_ID)?;
    Some((
        event.seq,
        id.to_string(),
        payload.get("method").and_then(Value::as_str),
        params,
    ))
}

/// `SUBS-001`: only the notification types the filter asked for.
///
/// A filter field that is absent is a type not subscribed to — the page says so
/// outright ("Omitting a field is equivalent to not subscribing") — so a
/// subscription with no `notifications` object at all has requested nothing, and
/// every notification on it but the acknowledgment is unrequested.
pub(in crate::checks) fn only_requested_notifications(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (subscription, (seq, id, method, params)) in resolved(context) {
        let Some(method) = method else {
            continue;
        };
        if method == ACKNOWLEDGED || is_teardown(method, params, &id) {
            continue;
        }
        // The subject is a notification delivered on a subscription; the
        // acknowledgment is the stream's own opening and not filtered content.
        sink.examined();
        if let Some(reason) = unrequested(subscription.filter, method, params) {
            sink.push(
                Some(seq),
                format!("subscription {id} was sent `{method}`, which {reason}"),
            );
        }
    }
}

/// Whether a tagged message is the server ending subscription `id` on stdio.
///
/// basic/patterns/cancellation: "A server MUST send `notifications/cancelled`
/// referencing a `subscriptions/listen` request ID when it tears down that
/// subscription stream." That is the stream's own ending, like the
/// acknowledgment is its opening — not a notification *type* the filter
/// selects — so the filter cannot have failed to request it.
fn is_teardown(method: &str, params: &Value, id: &str) -> bool {
    method == CANCELLED
        && params
            .get("requestId")
            // `id` is the subscription id's canonical JSON text.
            .is_some_and(|request| serde_json::from_str::<Value>(id).ok().as_ref() == Some(request))
}

/// Why `method` was not requested by `filter`, or `None` when it was.
fn unrequested(
    filter: Option<&Map<String, Value>>,
    method: &str,
    params: &Value,
) -> Option<String> {
    if let Some((_, field)) = FILTERED.iter().find(|(name, _)| *name == method) {
        let asked = filter
            .and_then(|filter| filter.get(*field))
            .is_some_and(|value| value.as_bool() == Some(true));
        return (!asked).then(|| format!("its filter did not set `{field}`"));
    }
    if method == RESOURCE_UPDATED {
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let listed = filter
            .and_then(|filter| filter.get(RESOURCE_SUBSCRIPTIONS))
            .and_then(Value::as_array)
            .is_some_and(|uris| uris.iter().any(|value| value.as_str() == Some(uri)));
        return (!listed)
            .then(|| format!("its `{RESOURCE_SUBSCRIPTIONS}` does not list the URI {uri:?}"));
    }
    if method == TASK_STATUS {
        let task = params
            .get("taskId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let listed = filter
            .and_then(|filter| filter.get(TASK_IDS))
            .and_then(Value::as_array)
            .is_some_and(|tasks| tasks.iter().any(|value| value.as_str() == Some(task)));
        return (!listed).then(|| format!("its `{TASK_IDS}` does not list the task {task:?}"));
    }
    Some("is not one of the notification types the filter can request".to_owned())
}

/// `SUBS-002`: the acknowledgment is a subscription's first message.
///
/// Judged per subscription id rather than per channel, which is the distinction
/// the page draws for stdio: another subscription's messages may interleave
/// ahead of this one's acknowledgment without breaking anything.
///
/// A subscription with no tagged message at all is not reported — a recording
/// that ends before the acknowledgment arrives is not evidence that it never
/// did. What is falsifiable is a *first* tagged message that is something else.
///
/// "First" is per opening, not per id: a client that reuses the id of a
/// subscription that has ended opens a new one, which owes its own
/// acknowledgment — so the decision is keyed by the `subscriptions/listen`
/// request's `seq`.
pub(in crate::checks) fn acknowledgment_first(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut decided: BTreeSet<u64> = BTreeSet::new();
    for (subscription, (seq, id, method, _)) in resolved(context) {
        if !decided.insert(subscription.seq) {
            continue;
        }
        // The subject is a subscription's *first* tagged message; a
        // subscription that produced none is undecided, not conforming.
        sink.examined();
        match method {
            Some(ACKNOWLEDGED) => {}
            Some(other) => sink.push(
                Some(seq),
                format!(
                    "subscription {id} opened with `{other}`; `{ACKNOWLEDGED}` must be its \
                     first message"
                ),
            ),
            None => sink.push(
                Some(seq),
                format!(
                    "subscription {id} was closed by its `{LISTEN}` response before any \
                     `{ACKNOWLEDGED}` was sent"
                ),
            ),
        }
    }
}

/// `SUBS-005` and `SUBS-006`: a graceful closure's result carries no
/// method-specific data.
///
/// The clauses' leading obligation — *send* a response before closing — has no
/// falsifier, and the page says why in its own words: "a transport that closes
/// without it indicates an unexpected disconnect". A close with no response is
/// therefore not a missing response but a different ending, and no trace
/// separates the two. What is judged is the half that is on the wire when the
/// server does respond: the result carries "no method-specific data beyond the
/// standard result fields and subscription metadata" — `resultType` and the
/// `_meta` that names the subscription, and nothing else.
///
/// **Re-reviewed 2026-08-20**, when the drift gate caught both quotes moving.
/// The page changed "the empty response" to "a successful response" and "an
/// empty result" to "a completion result", and added the sentence quoted above.
/// The obligation did not change: this check already permitted exactly
/// `resultType` and `_meta`, which the new prose now states outright instead of
/// leaving "empty" to be read. `resultType: "complete"` is *not* additionally
/// enforced — it appears only in the page's example, and a value shown in an
/// example with no RFC 2119 keyword is not a clause this registry enters
/// (03-conformance-strategy §What enters the registry, rule 3).
pub(in crate::checks) fn graceful_close_result_shape(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges_for(LISTEN) {
        let Some(result) = exchange.result.and_then(Value::as_object) else {
            continue;
        };
        sink.examined();
        let extra: Vec<&String> = result
            .keys()
            .filter(|key| *key != "resultType" && *key != "_meta")
            .collect();
        if !extra.is_empty() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the `{LISTEN}` response carries {}; a graceful closure's result \
                     carries no method-specific data beyond `resultType` and `_meta`",
                    extra
                        .iter()
                        .map(|key| format!("`{key}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }
}
