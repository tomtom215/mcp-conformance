// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The tasks extension (`io.modelcontextprotocol/tasks`, specified in
//! `modelcontextprotocol/ext-tasks` at `2026-07-28`): a server may answer a
//! `tools/call` with a task handle (`resultType: "task"`), and the client
//! follows the task with `tasks/get`, `tasks/update` and `tasks/cancel`.
//!
//! The subjects are the extension's own traffic — task results, `tasks/*`
//! calls, and `subscriptions/listen` requests that ask for task notifications —
//! so a session that never used tasks leaves every row here unobserved.
//!
//! This module judges what a task says: the result that creates one, what
//! `tasks/get` returns, and the acknowledgements. [`methods`] judges the
//! protocol around it — who may use the extension's methods and streams, and
//! how a task is cancelled and routed.

use std::collections::BTreeMap;

use serde_json::Value;

use super::super::FindingSink;
use super::client_declares;
use crate::context::{Exchange, TraceContext};

mod methods;
#[cfg(test)]
mod tests;

pub(in crate::checks) use methods::{
    listen_requires_declaration, mcp_name_header, methods_require_declaration,
    no_cancelled_notification, no_progress_on_task_stream,
};

/// The extension's identifier, as capabilities name it.
pub(super) const TASKS: &str = "io.modelcontextprotocol/tasks";

/// The methods the extension defines on a task.
pub(super) const METHODS: [&str; 3] = ["tasks/get", "tasks/update", "tasks/cancel"];

/// A task's statuses (the `Task` interface's `status` member).
const STATUSES: [&str; 5] = [
    "working",
    "input_required",
    "completed",
    "cancelled",
    "failed",
];

pub(super) fn result_type(result: &Value) -> Option<&str> {
    result.get("resultType").and_then(Value::as_str)
}

/// A string member of `value`.
pub(super) fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
    value.get(key).and_then(Value::as_str)
}

/// Every successful `tasks/get` result, with the seq of the response.
fn get_results<'a>(context: &'a TraceContext<'a>) -> impl Iterator<Item = (u64, &'a Value)> + 'a {
    context
        .exchanges_for("tasks/get")
        .filter_map(|exchange| Some((exchange.response.seq, exchange.result?)))
}

/// `tasks/get` results whose `status` is `status`.
fn with_status<'a>(
    context: &'a TraceContext<'a>,
    status: &'a str,
) -> impl Iterator<Item = (u64, &'a Value)> + 'a {
    get_results(context).filter(move |(_, result)| text(result, "status") == Some(status))
}

/// `TASK-001`: a task result answers only a request that declared the extension.
///
/// Per request, "regardless of prior declarations": an earlier request's
/// declaration does not carry over, so each task result is held to the request
/// it answers.
pub(in crate::checks) fn create_requires_declaration(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges() {
        if exchange.result.and_then(result_type) != Some("task") {
            continue;
        }
        sink.examined();
        if !client_declares(exchange.params, TASKS) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "a `resultType: \"task\"` result answered a `{}` request whose `_meta` client \
                     capabilities do not declare `{TASKS}`",
                    exchange.method
                ),
            );
        }
    }
}

/// `TASK-006`: a `CreateTaskResult` is marked `resultType: "task"`.
///
/// A `tools/call` result is a `CreateTaskResult` when it carries a task's
/// `taskId` and no `content`, the member every `CallToolResult` has.
pub(in crate::checks) fn create_result_discriminator(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges_for("tools/call") {
        let Some(result) = exchange.result else {
            continue;
        };
        if text(result, "taskId").is_none() || result.get("content").is_some() {
            continue;
        }
        sink.examined();
        if result_type(result) != Some("task") {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`tools/call` was answered with a task (`taskId` {:?}) whose `resultType` is \
                     {}, not \"task\"",
                    text(result, "taskId").unwrap_or_default(),
                    result
                        .get("resultType")
                        .map_or_else(|| "absent".to_owned(), ToString::to_string)
                ),
            );
        }
    }
}

/// `TASK-007`: `resultType: "task"` marks a `CreateTaskResult` and nothing else —
/// a result so marked carries the task's `taskId` and `status`.
pub(in crate::checks) fn task_result_type_only_on_create(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges() {
        let Some(result) = exchange
            .result
            .filter(|result| result_type(result) == Some("task"))
        else {
            continue;
        };
        sink.examined();
        let missing: Vec<&str> = ["taskId", "status"]
            .into_iter()
            .filter(|key| text(result, key).is_none())
            .collect();
        if !missing.is_empty() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the `{}` result is marked `resultType: \"task\"` but is no `CreateTaskResult`: \
                     it lacks {}",
                    exchange.method,
                    quoted(&missing)
                ),
            );
        }
    }
}

/// `TASK-015`: a `tasks/get` result is a `Task` — its id, a known status, both
/// timestamps and a `ttlMs` that is a number or `null`.
pub(in crate::checks) fn task_fields(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, result) in get_results(context) {
        sink.examined();
        let mut wrong = Vec::new();
        for key in ["taskId", "createdAt", "lastUpdatedAt"] {
            if text(result, key).is_none() {
                wrong.push(format!("`{key}` is not a string"));
            }
        }
        match text(result, "status") {
            Some(status) if STATUSES.contains(&status) => {}
            Some(status) => wrong.push(format!("`status` {status:?} is not a task status")),
            None => wrong.push("`status` is not a string".to_owned()),
        }
        if !result
            .get("ttlMs")
            .is_some_and(|ttl| ttl.is_null() || ttl.is_number())
        {
            wrong.push("`ttlMs` is neither a number nor null".to_owned());
        }
        if !wrong.is_empty() {
            sink.push(
                Some(seq),
                format!(
                    "the `tasks/get` result is not a `Task`: {}",
                    wrong.join("; ")
                ),
            );
        }
    }
}

/// `TASK-015`: an `input_required` task carries its `inputRequests` map.
pub(in crate::checks) fn input_required_carries_requests(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    payload_present(context, sink, "input_required", "inputRequests");
}

/// `TASK-015`, `TASK-044`, `TASK-046`: a `completed` task carries its `result`.
pub(in crate::checks) fn completed_carries_result(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    payload_present(context, sink, "completed", "result");
}

/// The status-specific member a `tasks/get` result of `status` must carry: an
/// object, present.
fn payload_present(context: &TraceContext<'_>, sink: &mut FindingSink, status: &str, key: &str) {
    for (seq, result) in with_status(context, status) {
        sink.examined();
        if !result.get(key).is_some_and(Value::is_object) {
            sink.push(
                Some(seq),
                format!("a `tasks/get` result with status `{status}` carries no `{key}` object"),
            );
        }
    }
}

/// `TASK-015`, `TASK-042`, `TASK-045`: a `failed` task carries the JSON-RPC
/// error — an object with an integer `code` and a string `message`.
pub(in crate::checks) fn failed_carries_error(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, result) in with_status(context, "failed") {
        sink.examined();
        let error = result.get("error");
        let is_jsonrpc_error = error.is_some_and(|error| {
            error.get("code").is_some_and(Value::is_i64)
                && error.get("message").is_some_and(Value::is_string)
        });
        if !is_jsonrpc_error {
            sink.push(
                Some(seq),
                "a `failed` task carries no JSON-RPC error: `error` must be an object with an \
                 integer `code` and a string `message`"
                    .to_owned(),
            );
        }
    }
}

/// `TASK-008`, `TASK-043`, `TASK-044`: `failed` is for JSON-RPC errors only, so
/// a `failed` task carries no `result` — a tool result, even with `isError`,
/// completes the task.
pub(in crate::checks) fn failed_not_tool_result(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, result) in with_status(context, "failed") {
        sink.examined();
        if result.get("result").is_some() {
            sink.push(
                Some(seq),
                "a `failed` task carries a `result`: a result, even one with `isError: true`, \
                 completes the task; `failed` is for JSON-RPC errors"
                    .to_owned(),
            );
        }
    }
}

/// `TASK-041`: a `failed` task says why in `statusMessage`.
pub(in crate::checks) fn failed_status_message(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, result) in with_status(context, "failed") {
        sink.examined();
        if text(result, "statusMessage").is_none() {
            sink.push(
                Some(seq),
                "a `failed` task carries no `statusMessage` with diagnostic information".to_owned(),
            );
        }
    }
}

/// `TASK-017`, `TASK-028`, `TASK-032`: the three methods' results are standard
/// results, `resultType: "complete"`.
pub(in crate::checks) fn results_complete(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for exchange in method_exchanges(context) {
        let Some(result) = exchange.result else {
            continue;
        };
        sink.examined();
        if result_type(result) != Some("complete") {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the `{}` result's `resultType` is {}, not \"complete\"",
                    exchange.method,
                    result
                        .get("resultType")
                        .map_or_else(|| "absent".to_owned(), ToString::to_string)
                ),
            );
        }
    }
}

/// `TASK-024`, `TASK-030`: `tasks/update` and `tasks/cancel` are acknowledged
/// with an empty result — nothing beyond `resultType` and `_meta`.
pub(in crate::checks) fn ack_empty(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for exchange in method_exchanges(context).filter(|exchange| exchange.method != "tasks/get") {
        let Some(result) = exchange.result.and_then(Value::as_object) else {
            continue;
        };
        sink.examined();
        let extra: Vec<&str> = result
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "resultType" && *key != "_meta")
            .collect();
        if !extra.is_empty() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "the `{}` acknowledgement is not empty: it carries {}",
                    exchange.method,
                    quoted(&extra)
                ),
            );
        }
    }
}

/// `TASK-022`, `TASK-023`: over a task's lifetime one `inputRequests` key names
/// one request.
///
/// A key repeated with the same request is the documented snapshot — every poll
/// lists what is still outstanding — and so is a key that reappears after the
/// client answered it, since an acknowledgement may precede the task's
/// observable status. A key repeated with a *different* request is the
/// violation, and the subject is every key a task lists again.
pub(in crate::checks) fn input_request_keys_stable(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let mut seen: BTreeMap<(String, String), (u64, &Value)> = BTreeMap::new();
    for exchange in context.exchanges_for("tasks/get") {
        let Some(result) = exchange.result else {
            continue;
        };
        let task = exchange
            .params
            .and_then(|params| text(params, "taskId"))
            .or_else(|| text(result, "taskId"))
            .unwrap_or_default();
        let Some(requests) = result.get("inputRequests").and_then(Value::as_object) else {
            continue;
        };
        for (key, request) in requests {
            let seq = exchange.response.seq;
            match seen.get(&(task.to_owned(), key.clone())) {
                None => {
                    seen.insert((task.to_owned(), key.clone()), (seq, request));
                }
                Some((first, earlier)) => {
                    sink.examined();
                    if *earlier != request {
                        sink.push(
                            Some(seq),
                            format!(
                                "task {task:?} reuses `inputRequests` key {key:?} for a different \
                                 request than the one it named at seq {first}"
                            ),
                        );
                    }
                }
            }
        }
    }
}

/// `TASK-026`: a client answers each input request once — no `tasks/update`
/// carries a key an earlier, acknowledged one for the same task already did.
pub(in crate::checks) fn update_answers_once(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut updates: Vec<Exchange<'_>> = context.exchanges_for("tasks/update").collect();
    updates.sort_by_key(|exchange| exchange.request.seq);
    let mut answered: BTreeMap<(String, String), u64> = BTreeMap::new();
    for exchange in updates {
        let Some(params) = exchange.params else {
            continue;
        };
        let task = text(params, "taskId").unwrap_or_default().to_owned();
        let Some(responses) = params.get("inputResponses").and_then(Value::as_object) else {
            continue;
        };
        for key in responses.keys() {
            sink.examined();
            if let Some(first) = answered.get(&(task.clone(), key.clone())) {
                sink.push(
                    Some(exchange.request.seq),
                    format!(
                        "`tasks/update` answers input request {key:?} of task {task:?} again; the \
                         update at seq {first} answered it and was acknowledged"
                    ),
                );
            }
        }
        if exchange.result.is_some() {
            for key in responses.keys() {
                answered
                    .entry((task.clone(), key.clone()))
                    .or_insert(exchange.request.seq);
            }
        }
    }
}

/// Every answered `tasks/get`, `tasks/update` and `tasks/cancel`.
pub(super) fn method_exchanges<'a>(
    context: &'a TraceContext<'a>,
) -> impl Iterator<Item = Exchange<'a>> + 'a {
    context
        .exchanges()
        .filter(|exchange| METHODS.contains(&exchange.method))
}

/// `keys` as `` `a`, `b` ``.
fn quoted(keys: &[&str]) -> String {
    keys.iter()
        .map(|key| format!("`{key}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
