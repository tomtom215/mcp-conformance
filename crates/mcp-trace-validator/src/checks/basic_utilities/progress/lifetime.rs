// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How long a progress token lives: until its request is answered — or, for a
//! task-augmented request, until the task it created reaches a terminal
//! status — and which token a task's progress keeps.
//!
//! One walk of the recording serves both rules. It learns, in trace order,
//! which request took each token up, when that request finished, and which
//! task a `CreateTaskResult` tied the token to; every progress notification is
//! then a [`Progress`] observation that each check judges on its own terms.
//!
//! **Tasks.** `basic/utilities/tasks` is out of the registry's scope at this
//! revision (experimental, and unscored by everything that scores conformance),
//! but the progress page states three MUSTs about task-augmented requests, and
//! a recording carries what they need: the request's `params.task`, the
//! `CreateTaskResult` that names the task, the `tasks/get`, `tasks/cancel` and
//! `tasks/result` answers and `notifications/tasks/status` that show it
//! reaching a terminal status, and the `io.modelcontextprotocol/related-task`
//! `_meta` that ties a later message to it. A task the recording never saw
//! created is unknown here, and nothing about it is judged.

use std::collections::BTreeMap;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::trace::{TraceEvent, TransportKind};
use serde_json::Value;

use super::super::super::FindingSink;
use super::super::{Party, answers, notification_params, request_id};
use super::{progress_params, requested_token};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The `_meta` key tying a message to a task.
const RELATED_TASK: &str = "io.modelcontextprotocol/related-task";

/// The statuses a task does not leave.
const TERMINAL: [&str; 3] = ["completed", "failed", "cancelled"];

/// How a token's request finished, and at which `seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finished {
    /// Its answer arrived: a result or error that was not a `CreateTaskResult`.
    Answered(u64),
    /// The task its `CreateTaskResult` named reached a terminal status.
    Terminal(u64),
}

/// The request that currently holds a token.
#[derive(Debug, Clone, Copy)]
struct Owner {
    /// The request's `seq`.
    request: u64,
    /// How and when it finished, once it has.
    finished: Option<Finished>,
}

/// A progress notification tied to a task by its `_meta`.
#[derive(Debug, Clone)]
struct Related {
    task_id: String,
    /// The task-augmented request's `seq` and the token it provided.
    request: u64,
    token: String,
}

/// One progress notification, with what the walk knew when it was sent.
#[derive(Debug)]
struct Progress<'a> {
    event: &'a TraceEvent,
    /// The notification's token, as canonical text; `None` when it has none.
    token: Option<String>,
    /// The request of the addressed party that holds the token, if any.
    owner: Option<Owner>,
    /// The task the notification names, when the recording saw it created.
    related: Option<Related>,
}

/// What the walk remembers about a request it has passed.
struct Request<'a> {
    sender: Party,
    method: &'a str,
    params: Option<&'a Value>,
    /// Its `_meta.progressToken`, as canonical text.
    token: Option<String>,
}

/// The walk's state: who holds each token, and which task each token's
/// request created.
#[derive(Default)]
struct Walk<'a> {
    /// Request seq → what it was.
    requests: BTreeMap<u64, Request<'a>>,
    /// (request's sender, token text) → the latest request that took it up.
    owners: BTreeMap<(Party, String), Owner>,
    /// (the task's receiver — the party that created it, task id) → the
    /// request it answered: its sender and token, and its seq.
    tasks: BTreeMap<(Party, String), ((Party, String), u64)>,
}

impl<'a> Walk<'a> {
    /// A request of either party: remember it, and let it take its token up.
    fn request(&mut self, event: &'a TraceEvent, payload: &'a Value, method: &'a str) {
        let token = requested_token(event).map(to_canonical_string);
        let sender = Party::of(event.direction);
        if let Some(token) = &token {
            self.owners.insert(
                (sender, token.clone()),
                Owner {
                    request: event.seq,
                    finished: None,
                },
            );
        }
        self.requests.insert(
            event.seq,
            Request {
                sender,
                method,
                params: payload.get("params"),
                token,
            },
        );
    }

    /// An answer: a request finished — unless it asked for a task and got one,
    /// in which case its token now lives as long as the task.
    fn answered(&mut self, event: &'a TraceEvent, payload: &Value, request: u64) {
        let Some(asked) = self.requests.get(&request) else {
            return;
        };
        let result = payload.get("result");
        let augmented = asked
            .params
            .is_some_and(|params| params.get("task").is_some_and(Value::is_object));
        let task_id = result
            .and_then(|result| result.get("task")?.get("taskId")?.as_str())
            .map(str::to_owned);
        let key = asked.token.clone().map(|token| (asked.sender, token));
        match (augmented, task_id) {
            (true, Some(task_id)) => {
                if let Some(key) = key {
                    self.tasks
                        .insert((Party::of(event.direction), task_id), (key, request));
                }
            }
            _ => {
                if let Some(owner) = key.and_then(|key| self.owners.get_mut(&key))
                    && owner.request == request
                {
                    owner.finished = Some(Finished::Answered(event.seq));
                }
            }
        }
        // The same answer may also be the one that shows a task ending.
        self.task_answer(event, asked.method, asked.params, result);
    }

    /// A `tasks/get` or `tasks/cancel` result carrying a terminal status, or a
    /// `tasks/result` answered at all: the task is over.
    fn task_answer(
        &mut self,
        event: &TraceEvent,
        method: &str,
        params: Option<&Value>,
        result: Option<&Value>,
    ) {
        let Some(result) = result else {
            return;
        };
        match method {
            "tasks/get" | "tasks/cancel" => self.terminal_task(event, result),
            "tasks/result" => {
                if let Some(task_id) = params.and_then(|params| params.get("taskId")?.as_str()) {
                    self.ended(Party::of(event.direction), task_id, event.seq);
                }
            }
            _ => {}
        }
    }

    /// A `Task` object sent by `event`'s party — a `tasks/get` or
    /// `tasks/cancel` result, or `notifications/tasks/status` params — whose
    /// status is terminal.
    fn terminal_task(&mut self, event: &TraceEvent, task: &Value) {
        let (Some(task_id), Some(status)) = (
            task.get("taskId").and_then(Value::as_str),
            task.get("status").and_then(Value::as_str),
        ) else {
            return;
        };
        if TERMINAL.contains(&status) {
            self.ended(Party::of(event.direction), task_id, event.seq);
        }
    }

    /// The task `receiver` created as `task_id` reached a terminal status at `at`.
    fn ended(&mut self, receiver: Party, task_id: &str, at: u64) {
        let Some((key, request)) = self.tasks.get(&(receiver, task_id.to_owned())) else {
            return;
        };
        if let Some(owner) = self.owners.get_mut(key)
            && owner.request == *request
            && owner.finished.is_none()
        {
            owner.finished = Some(Finished::Terminal(at));
        }
    }

    /// A progress notification, as the walk sees it now.
    fn observe(&self, event: &'a TraceEvent, params: Option<&Value>) -> Progress<'a> {
        let token = params
            .and_then(|params| params.get("progressToken"))
            .map(to_canonical_string);
        let owner = token
            .as_ref()
            .and_then(|token| {
                self.owners
                    .get(&(Party::addressed_by(event.direction), token.clone()))
            })
            .copied();
        let related = params
            .and_then(|params| {
                params
                    .get("_meta")?
                    .get(RELATED_TASK)?
                    .get("taskId")?
                    .as_str()
            })
            .and_then(|task_id| {
                let ((_, token), request) = self
                    .tasks
                    .get(&(Party::of(event.direction), task_id.to_owned()))?;
                Some(Related {
                    task_id: task_id.to_owned(),
                    request: *request,
                    token: token.clone(),
                })
            });
        Progress {
            event,
            token,
            owner,
            related,
        }
    }
}

/// Every progress notification in the recording, each with what had happened
/// to its token by then.
fn observations<'a>(context: &TraceContext<'a>) -> Vec<Progress<'a>> {
    let answers = answers(context);
    let mut walk = Walk::default();
    let mut seen = Vec::new();
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if let Some(request) = answers.get(&event.seq) {
            walk.answered(event, payload, *request);
            continue;
        }
        if request_id(payload).is_some()
            && let Some(method) = payload.get("method").and_then(Value::as_str)
        {
            walk.request(event, payload, method);
            continue;
        }
        if let Some(params) = progress_params(event) {
            seen.push(walk.observe(event, Some(params)));
        } else if let Some(task) = notification_params(event, "notifications/tasks/status") {
            walk.terminal_task(event, task);
        }
    }
    seen
}

/// `PROG-013` (the "in-progress operation" half), `PROG-016` and `PROG-019`:
/// no progress for a request already finished.
///
/// Judged on stdio only, where one ordered channel carries both the
/// notification and the answer, so a notification recorded after the answer
/// was written after it. Streamable HTTP can carry them on different streams,
/// whose relative order in a recording is the order they were captured, not
/// sent — the ambiguity `stateless::progress` records for the same rule.
///
/// A request that asked for a task (`params.task`) and was answered with a
/// `CreateTaskResult` is not finished by that answer: the page keeps its token
/// valid "throughout the task's lifetime", and the request finishes when the
/// recording shows the task reaching a terminal status. A token no recorded
/// request carried is not reported here — it is `token-issued-in-session`'s,
/// which knows whether the recording can say so.
pub(in crate::checks) fn stops_after_completion(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for progress in observations(context) {
        if progress.event.transport != TransportKind::Stdio {
            continue;
        }
        let Some(owner) = progress.owner else {
            continue;
        };
        // The subject is a notification whose token the recording saw a
        // request take up, on a transport whose order is the sender's.
        sink.examined();
        let token = progress.token.unwrap_or_default();
        match owner.finished {
            None => {}
            Some(Finished::Answered(at)) => sink.push(
                Some(progress.event.seq),
                format!(
                    "progress for token {token} after its request (seq {}) was answered at seq \
                     {at}; progress notifications must stop after completion",
                    owner.request
                ),
            ),
            Some(Finished::Terminal(at)) => sink.push(
                Some(progress.event.seq),
                format!(
                    "progress for token {token} after the task its request (seq {}) created \
                     reached a terminal status at seq {at}; progress notifications for tasks \
                     must stop then",
                    owner.request
                ),
            ),
        }
    }
}

/// `PROG-014` and `PROG-015`: a task's progress keeps the token its request
/// provided.
///
/// A notification is a task's when its `_meta` names the task
/// (`io.modelcontextprotocol/related-task`), and the task is one the recording
/// saw a `CreateTaskResult` create for a request that carried a token. Its
/// token must then be that request's; any other is a token the task's
/// requestor never asked progress under. A task created for a request that
/// carried no token leaves nothing to keep, and is not judged here.
pub(in crate::checks) fn task_token_unchanged(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for progress in observations(context) {
        let Some(related) = progress.related else {
            continue;
        };
        // The subject is a progress notification tied to a task the recording
        // saw created against a token.
        sink.examined();
        if progress.token.as_deref() != Some(related.token.as_str()) {
            sink.push(
                Some(progress.event.seq),
                format!(
                    "the {}'s progress for task {:?} carries token {}, not the token {} its \
                     task-augmented request (seq {}) provided; a task's progress must keep that \
                     token throughout the task's lifetime",
                    Party::of(progress.event.direction).name(),
                    related.task_id,
                    progress.token.as_deref().unwrap_or("none"),
                    related.token,
                    related.request
                ),
            );
        }
    }
}
