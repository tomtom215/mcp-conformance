// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `client/roots`: what a `roots/list` answer may contain, whether the server
//! checked the capability before asking, and the client's error when it has no
//! roots to give.

use std::collections::BTreeSet;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};
use serde_json::Value;

use super::super::FindingSink;
use super::asks::{answers_for, asks_for};
use super::elicitation::declared_where_answered;
use crate::context::TraceContext;

/// The roots method.
const ROOTS: &str = "roots/list";

/// The notification a client sends when its roots change.
const LIST_CHANGED: &str = "notifications/roots/list_changed";

/// "Clients that support roots MUST declare the `roots` capability".
pub(in crate::checks) fn client_declares_capability(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_where_answered(context, sink, ROOTS, "roots");
}

/// Whether `uri` is a `file://` URI; the scheme compared without case, as RFC
/// 3986 §3.1 compares schemes.
fn is_file_uri(uri: &str) -> bool {
    uri.get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
}

/// "`uri`: Unique identifier for the root. This MUST be a `file://` URI in the
/// current specification."
pub(in crate::checks) fn uri_file_scheme(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for answer in answers_for(context, ROOTS) {
        let roots = answer
            .result
            .and_then(|result| result.get("roots"))
            .and_then(Value::as_array);
        for (index, root) in roots.into_iter().flatten().enumerate() {
            sink.examined();
            let uri = root.get("uri").and_then(Value::as_str);
            if !uri.is_some_and(is_file_uri) {
                sink.push(
                    Some(answer.seq),
                    format!(
                        "the client's answer to {} lists `roots[{index}]` with {}, not a \
                         `file://` URI",
                        answer.ask.label(),
                        uri.map_or_else(|| "no string `uri`".to_owned(), |uri| format!("{uri:?}"))
                    ),
                );
            }
        }
    }
}

/// "Servers SHOULD: Check for roots capability before usage" — its
/// wire-visible failure is asking a client that did not declare `roots`.
pub(in crate::checks) fn capability_checked(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, ROOTS) {
        let Some(declared) = ask.declared.declares(&["roots"]) else {
            continue;
        };
        sink.examined();
        if !declared {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} asks for roots, but {} does not declare `roots`",
                    ask.label(),
                    ask.declaration()
                ),
            );
        }
    }
}

/// `2025-11-25` "Clients SHOULD return standard JSON-RPC errors for common
/// failure cases: Client does not support roots: `-32601` (Method not found)".
///
/// A client that did not declare `roots` is taken at its word: answering such a
/// request with roots would be [`client_declares_capability`]'s finding, and
/// any other error is not the one the row names.
pub(in crate::checks) fn unsupported_rejected(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for answer in answers_for(context, ROOTS) {
        if answer.ask.is_mrtr() || answer.ask.declared.declares(&["roots"]) != Some(false) {
            continue;
        }
        sink.examined();
        if answer.error_code != Some(-32601) {
            sink.push(
                Some(answer.seq),
                format!(
                    "the client, which did not declare `roots`, answered {} with {} rather than \
                     the `-32601` error",
                    answer.ask.label(),
                    answer
                        .error_code
                        .map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
                ),
            );
        }
    }
}

/// The set of root URIs a `roots/list` result lists; `None` unless `roots` is
/// an array whose every entry has a string `uri`.
fn root_uris(result: &Value) -> Option<BTreeSet<&str>> {
    result
        .get("roots")?
        .as_array()?
        .iter()
        .map(|root| root.get("uri").and_then(Value::as_str))
        .collect()
}

/// The `seq` at which the stdio session the client initialized ended: the first
/// lifecycle event after its `initialize` request, when that is a stdio
/// `transport-close` or `transport-abort`. `None` when the recording shows no
/// end, or shows the transport reopening first (another process, and so
/// another session, whose roots are its own).
fn session_end(context: &TraceContext<'_>) -> Option<u64> {
    let (initialize, _) = context.initialize().request?;
    context
        .events()
        .iter()
        .filter(|event| event.seq > initialize)
        .find_map(|event| match event.body {
            EventBody::Lifecycle { event: moment } => Some((event, moment)),
            _ => None,
        })
        .filter(|(event, moment)| {
            event.transport == TransportKind::Stdio
                && matches!(
                    moment,
                    LifecycleEvent::TransportClose | LifecycleEvent::TransportAbort
                )
        })
        .map(|(event, _)| event.seq)
}

/// `2025-11-25` "When roots change, clients that support `listChanged` MUST
/// send a notification".
///
/// That the roots changed happens inside the client, but its own `roots/list`
/// results say what they were: two results listing different sets of root
/// URIs are the client's statement that they changed.
///
/// **Stimulus.** Over stdio, a client whose `initialize` request declared
/// `roots.listChanged`, two successive `roots/list` results (each a `roots`
/// array whose entries all carry a string `uri`; an error or an unreadable
/// result between them is passed over), and a `transport-close` or
/// `transport-abort` after the second with no other lifecycle event since
/// `initialize` — the session is over, so no notification is still to come. Each such pair is a subject. A recording
/// that stops while the transport is open is not judged: the notification may
/// follow the second result. Nor is Streamable HTTP, where a recording carries
/// no event that ends the session.
///
/// **Conforming.** The two sets are equal, or the client sent
/// `notifications/roots/list_changed` somewhere in the trace. Any one
/// notification excuses every pair: a notification sent before the first
/// result may announce a change that result did not yet reflect, and one
/// notification may cover several changes.
///
/// **Violation (fail).** The sets differ (by URI; order and names are not
/// compared) and the client sent no `notifications/roots/list_changed` in the
/// whole trace.
pub(in crate::checks) fn list_changed_notified(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let notified = context.messages().any(|(event, kind, _)| {
        event.direction == Direction::ClientToServer
            && matches!(kind, MessageKind::Notification { method } if *method == LIST_CHANGED)
    });
    let Some(end) = session_end(context) else {
        return;
    };
    let listings: Vec<(u64, BTreeSet<&str>)> = answers_for(context, ROOTS)
        .into_iter()
        .filter(|answer| {
            !answer.ask.is_mrtr()
                && answer.declared.declares(&["roots", "listChanged"]) == Some(true)
        })
        .filter_map(|answer| Some((answer.seq, root_uris(answer.result?)?)))
        .collect();
    for pair in listings.windows(2) {
        let ((first_seq, first), (second_seq, second)) = (&pair[0], &pair[1]);
        if end < *second_seq {
            continue;
        }
        sink.examined();
        if first != second && !notified {
            sink.push(
                Some(*second_seq),
                format!(
                    "the client declared `roots.listChanged` and answered `roots/list` with a \
                     different set of roots than at seq {first_seq}, but sent no \
                     `{LIST_CHANGED}` anywhere in the session, which ended at seq {end}"
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
