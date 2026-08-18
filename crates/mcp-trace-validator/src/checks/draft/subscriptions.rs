// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `subscriptions/listen`: the long-lived notification stream that replaced
//! `resources/subscribe` and the HTTP GET endpoint.
//!
//! Every message belonging to a subscription carries the `subscriptions/listen`
//! request's own JSON-RPC id in `_meta.io.modelcontextprotocol/subscriptionId`,
//! which is what makes these checks possible on stdio, where every subscription
//! shares one channel. They correlate on that field and on nothing else — never
//! on message order alone, because the specification explicitly permits other
//! subscriptions' messages to interleave.
//!
//! Three of the page's seven clauses carry exclusions: two bind what the client
//! does with what it received (compare the filter, demultiplex the stream), and
//! one begins after a reconnection, which is a second recording.

use std::collections::{BTreeMap, BTreeSet};

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

/// An open subscription: the id its messages are tagged with, and its filter.
#[derive(Debug, Clone, Copy)]
struct Subscription<'a> {
    /// The `subscriptions/listen` request's id, in canonical text.
    seq: u64,
    /// The `notifications` filter, when the request carried one.
    filter: Option<&'a Map<String, Value>>,
}

/// Every `subscriptions/listen` request in the trace, keyed by its id text.
fn subscriptions<'a>(context: &'a TraceContext<'_>) -> BTreeMap<String, Subscription<'a>> {
    context
        .messages()
        .filter_map(|(event, _, _)| {
            if event.direction != Direction::ClientToServer {
                return None;
            }
            let payload = event.message_payload()?;
            if payload.get("method")?.as_str()? != LISTEN {
                return None;
            }
            let id = payload.get("id").filter(|id| !id.is_null())?;
            Some((
                id.to_string(),
                Subscription {
                    seq: event.seq,
                    filter: payload
                        .get("params")
                        .and_then(|params| params.get("notifications"))
                        .and_then(Value::as_object),
                },
            ))
        })
        .collect()
}

/// Server messages tagged with a subscription id, as `(seq, id text, method)`.
///
/// Both notifications and the closing response are tagged, so the method is
/// `None` for the response — which is exactly what distinguishes it.
fn tagged<'a>(context: &'a TraceContext<'_>) -> Vec<(u64, String, Option<&'a str>, &'a Value)> {
    context
        .messages()
        .filter_map(|(event, _, _)| tagged_message(event))
        .collect()
}

/// The subscription tag one server message carries, if any.
fn tagged_message(event: &TraceEvent) -> Option<(u64, String, Option<&str>, &Value)> {
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
    let subscriptions = subscriptions(context);
    for (seq, id, method, params) in tagged(context) {
        let (Some(method), Some(subscription)) = (method, subscriptions.get(&id)) else {
            continue;
        };
        if method == ACKNOWLEDGED {
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
pub(in crate::checks) fn acknowledgment_first(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let subscriptions = subscriptions(context);
    // One ordered pass, deciding each subscription at its first tagged message.
    // The alternative — filtering the tagged messages by `tag == id && seq >
    // listen.seq` — carried two comparisons a trace can never exercise: a
    // subscription's messages are server-sent and its `subscriptions/listen` is
    // client-sent, so no two share a `seq`, making `>` and `>=` the same rule.
    let mut open: BTreeMap<String, u64> = BTreeMap::new();
    let mut decided: BTreeSet<String> = BTreeSet::new();
    for (event, _, _) in context.messages() {
        if let Some((id, subscription)) = subscriptions
            .iter()
            .find(|(_, subscription)| subscription.seq == event.seq)
        {
            open.insert(id.clone(), subscription.seq);
            continue;
        }
        let Some((seq, id, method, _)) = tagged_message(event) else {
            continue;
        };
        if !open.contains_key(&id) || !decided.insert(id.clone()) {
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

/// `SUBS-005` and `SUBS-006`: a graceful closure's result is empty.
///
/// The clauses' leading obligation — *send* a response before closing — has no
/// falsifier, and the page says why in its own words: "a transport that closes
/// without it indicates an unexpected disconnect". A close with no response is
/// therefore not a missing response but a different ending, and no trace
/// separates the two. What is judged is the half that is on the wire when the
/// server does respond: the result must be empty, carrying nothing beyond
/// `resultType` and the `_meta` that names the subscription.
pub(in crate::checks) fn graceful_close_result_empty(
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
                    "the `{LISTEN}` response carries {}; a graceful closure's result is empty",
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
