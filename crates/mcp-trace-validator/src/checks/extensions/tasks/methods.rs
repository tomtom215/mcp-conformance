// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The protocol around a task: which client may use the extension's methods
//! and its listen streams, how a task is cancelled, what its stream carries,
//! and how its requests are routed over Streamable HTTP.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::super::FindingSink;
use super::super::super::stateless::transport::{Match, compare, framing};
use super::super::{client_declares, error_code};
use super::{METHODS, TASKS, method_exchanges, result_type, text};
use crate::context::TraceContext;

/// `MissingRequiredClientCapabilityError`'s code at this revision.
const MISSING_CAPABILITY: i64 = -32021;

/// The `_meta` key tying a message to its subscription.
const SUBSCRIPTION_ID: &str = "io.modelcontextprotocol/subscriptionId";

/// `TASK-029`: a task is cancelled with `tasks/cancel`, never with
/// `notifications/cancelled` naming the request the task answered.
pub(in crate::checks) fn no_cancelled_notification(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // The ids of requests answered with a task, as each answer arrives.
    let tasked: BTreeMap<u64, &Value> = context
        .exchanges()
        .filter(|exchange| exchange.result.and_then(result_type) == Some("task"))
        .filter_map(|exchange| {
            let id = exchange.request.message_payload()?.get("id")?;
            Some((exchange.response.seq, id))
        })
        .collect();
    let Some(first) = tasked.keys().next().copied() else {
        return;
    };
    for (event, kind, _) in context.messages() {
        let MessageKind::Notification { method } = kind else {
            continue;
        };
        if *method != "notifications/cancelled"
            || event.direction != Direction::ClientToServer
            || event.seq < first
        {
            continue;
        }
        sink.examined();
        let named = event
            .message_payload()
            .and_then(|payload| payload.get("params")?.get("requestId"));
        let cancels_task = tasked.range(..event.seq).any(|(_, id)| Some(*id) == named);
        if cancels_task {
            sink.push(
                Some(event.seq),
                format!(
                    "`notifications/cancelled` names request {}, which a task answered; a task \
                     is cancelled with `tasks/cancel`",
                    named.map(ToString::to_string).unwrap_or_default()
                ),
            );
        }
    }
}

/// The `subscriptions/listen` requests that ask for task notifications, by the
/// seq of the request.
fn task_listens<'a>(context: &'a TraceContext<'a>) -> Vec<(u64, &'a Value, Option<&'a Value>)> {
    context
        .messages()
        .filter_map(|(event, kind, _)| {
            let MessageKind::Request { method, id } = kind else {
                return None;
            };
            if *method != "subscriptions/listen" || event.direction != Direction::ClientToServer {
                return None;
            }
            let params = event.message_payload()?.get("params");
            let asks = params
                .and_then(|params| params.get("notifications")?.get("taskIds")?.as_array())
                .is_some_and(|ids| !ids.is_empty());
            asks.then_some((event.seq, *id, params))
        })
        .collect()
}

/// `TASK-033`, `TASK-038`: a listen request asking for task notifications from
/// a client that did not declare the extension is refused with `-32021`.
///
/// Judged once the server answers it: an error response is the refusal, and a
/// result or an acknowledgement on the subscription is acceptance.
pub(in crate::checks) fn listen_requires_declaration(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, id, params) in task_listens(context) {
        if client_declares(params, TASKS) {
            continue;
        }
        let answer = context
            .exchanges_for("subscriptions/listen")
            .find(|exchange| exchange.request.seq == seq);
        let acknowledged = context.messages().any(|(event, kind, _)| {
            matches!(kind, MessageKind::Notification { .. })
                && event.seq > seq
                && event.direction == Direction::ServerToClient
                && subscription_of(event) == Some(id)
        });
        let refused = answer
            .as_ref()
            .and_then(|exchange| error_code(exchange.response));
        if answer.is_none() && !acknowledged {
            continue;
        }
        sink.examined();
        if refused != Some(MISSING_CAPABILITY) {
            sink.push(
                Some(answer.map_or(seq, |exchange| exchange.response.seq)),
                format!(
                    "`subscriptions/listen` asked for task notifications without declaring \
                     `{TASKS}` and was not refused with `{MISSING_CAPABILITY}`"
                ),
            );
        }
    }
}

/// The subscription a server message belongs to.
fn subscription_of(event: &mcp_conformance_core::trace::TraceEvent) -> Option<&Value> {
    event
        .message_payload()?
        .get("params")?
        .get("_meta")?
        .get(SUBSCRIPTION_ID)
}

/// `TASK-034`: a task's listen stream carries no progress or log notifications.
pub(in crate::checks) fn no_progress_on_task_stream(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let listens = task_listens(context);
    if listens.is_empty() {
        return;
    }
    // The subscription an id names is the one opened by the latest listen
    // request carrying it, so each id maps to whether that request asked for tasks.
    let all_listens: Vec<(u64, &Value)> = context
        .messages()
        .filter_map(|(event, kind, _)| match kind {
            MessageKind::Request { method, id } if *method == "subscriptions/listen" => {
                Some((event.seq, *id))
            }
            _ => None,
        })
        .collect();
    let task_seqs: BTreeSet<u64> = listens.iter().map(|(seq, _, _)| *seq).collect();
    for (event, kind, _) in context.messages() {
        let MessageKind::Notification { method } = kind else {
            continue;
        };
        if event.direction != Direction::ServerToClient {
            continue;
        }
        let Some(subscription) = subscription_of(event) else {
            continue;
        };
        let opened = all_listens
            .iter()
            .filter(|(seq, id)| *seq < event.seq && *id == subscription)
            .map(|(seq, _)| *seq)
            .next_back();
        if !opened.is_some_and(|seq| task_seqs.contains(&seq)) {
            continue;
        }
        sink.examined();
        if matches!(*method, "notifications/progress" | "notifications/message") {
            sink.push(
                Some(event.seq),
                format!("`{method}` was sent on a task's `subscriptions/listen` stream"),
            );
        }
    }
}

/// `TASK-035`: over Streamable HTTP, a `tasks/*` request's `Mcp-Name` header
/// carries its `taskId` (plainly or in the Base64 sentinel form).
pub(in crate::checks) fn mcp_name_header(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for post in framing::Framing::new(context).posts() {
        let Some(method) = text(post.payload, "method").filter(|method| METHODS.contains(method))
        else {
            continue;
        };
        if post.payload.get("id").is_none_or(Value::is_null) {
            continue;
        }
        let Some(task) = post
            .payload
            .get("params")
            .and_then(|params| text(params, "taskId"))
        else {
            continue;
        };
        sink.examined();
        match post.headers.get("mcp-name") {
            None => sink.push(
                Some(post.seq),
                format!("the POST for `{method}` carries no `Mcp-Name` header for task {task:?}"),
            ),
            Some(sent) if compare(sent, task) == Match::Mismatch => sink.push(
                Some(post.seq),
                format!("`Mcp-Name` is {sent:?} but the `{method}` request's `taskId` is {task:?}"),
            ),
            Some(_) => {}
        }
    }
}

/// `TASK-039`: a client that did not declare the extension has its `tasks/*`
/// requests refused with `-32021`.
pub(in crate::checks) fn methods_require_declaration(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in method_exchanges(context) {
        if client_declares(exchange.params, TASKS) {
            continue;
        }
        sink.examined();
        if error_code(exchange.response) != Some(MISSING_CAPABILITY) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`{}` from a client that did not declare `{TASKS}` was answered without the \
                     `{MISSING_CAPABILITY}` error",
                    exchange.method
                ),
            );
        }
    }
}
