// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the tasks extension's checks: the edges the corpus pairs do not
//! pin — per-request declaration, snapshot repeats, acknowledgement order,
//! which listen stream a notification is on.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};

use super::super::testkit::{error, judge, notification, params, request, result};
use super::TASKS;

fn task(id: &str, status: &str) -> Value {
    json!({"taskId": id, "status": status, "createdAt": "2026-10-13T10:00:00Z",
           "lastUpdatedAt": "2026-10-13T10:00:00Z", "ttlMs": null})
}

fn with(mut value: Value, key: &str, member: Value) -> Value {
    value[key] = member;
    value
}

fn created(id: u64, task_id: &str) -> Vec<(&'static str, Value)> {
    vec![
        request(
            id,
            "tools/call",
            params(json!({"name": "deploy"}), &[TASKS]),
        ),
        result(
            id,
            with(task(task_id, "working"), "resultType", json!("task")),
        ),
    ]
}

#[test]
fn a_declaration_counts_on_the_request_it_rides_on_only() {
    // The first call declares, the second does not: "regardless of prior declarations".
    let mut steps = created(1, "t-1");
    steps.push(request(
        2,
        "tools/call",
        params(json!({"name": "deploy"}), &[]),
    ));
    steps.push(result(
        2,
        with(task("t-2", "working"), "resultType", json!("task")),
    ));
    let (subjects, findings) = judge("tasks.create-requires-declaration", &steps);
    assert_eq!(subjects, 2);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn a_call_tool_result_is_no_task_and_is_not_judged_as_one() {
    let steps = [
        request(1, "tools/call", params(json!({"name": "echo"}), &[TASKS])),
        result(
            1,
            json!({"resultType": "complete", "content": [], "taskId": "a field of the tool's"}),
        ),
    ];
    assert_eq!(
        judge("tasks.create-result-discriminator", &steps),
        (0, vec![])
    );
    assert_eq!(
        judge("tasks.create-requires-declaration", &steps),
        (0, vec![])
    );
}

#[test]
fn a_task_status_outside_the_five_is_named() {
    let mut steps = created(1, "t-1");
    steps.push(request(
        2,
        "tasks/get",
        params(json!({"taskId": "t-1"}), &[TASKS]),
    ));
    steps.push(result(
        2,
        with(task("t-1", "paused"), "resultType", json!("complete")),
    ));
    let (_, findings) = judge("tasks.task-fields", &steps);
    assert_eq!(findings.len(), 1);
    assert!(
        findings[0].contains("\"paused\" is not a task status"),
        "{findings:?}"
    );
}

#[test]
fn a_failed_task_needs_a_jsonrpc_shaped_error() {
    let mut steps = created(1, "t-1");
    let failed = with(
        task("t-1", "failed"),
        "error",
        json!({"code": "boom", "message": "x"}),
    );
    steps.push(request(
        2,
        "tasks/get",
        params(json!({"taskId": "t-1"}), &[TASKS]),
    ));
    steps.push(result(2, failed));
    let (subjects, findings) = judge("tasks.failed-carries-error", &steps);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
}

#[test]
fn the_same_request_again_under_its_key_is_the_snapshot_not_a_reuse() {
    let asks = |message: &str| {
        let ask =
            json!({"confirm": {"method": "elicitation/create", "params": {"message": message}}});
        with(
            with(task("t-1", "input_required"), "inputRequests", ask),
            "resultType",
            json!("complete"),
        )
    };
    let mut steps = created(1, "t-1");
    for (id, message) in [(2, "Proceed?"), (3, "Proceed?"), (4, "Which region?")] {
        steps.push(request(
            id,
            "tasks/get",
            params(json!({"taskId": "t-1"}), &[TASKS]),
        ));
        steps.push(result(id, asks(message)));
    }
    let (subjects, findings) = judge("tasks.input-request-keys-stable", &steps);
    assert_eq!(subjects, 2, "the two re-listings are the subjects");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("seq 3"),
        "names where the key was first used: {findings:?}"
    );
}

#[test]
fn an_answer_repeated_after_a_refused_update_is_not_a_repeat() {
    let update = |id| {
        request(
            id,
            "tasks/update",
            params(
                json!({"taskId": "t-1", "inputResponses": {"confirm": {"action": "accept"}}}),
                &[TASKS],
            ),
        )
    };
    let steps = [
        update(1),
        error(1, -32603),
        update(2),
        result(2, json!({"resultType": "complete"})),
    ];
    assert_eq!(judge("tasks.update-answers-once", &steps), (2, vec![]));
}

#[test]
fn cancelling_a_request_that_is_not_a_task_is_ordinary() {
    let mut steps = created(1, "t-1");
    steps.push(request(
        2,
        "tools/call",
        params(json!({"name": "slow"}), &[TASKS]),
    ));
    steps.push(notification(
        "client-to-server",
        "notifications/cancelled",
        json!({"requestId": 2}),
    ));
    assert_eq!(
        judge("tasks.no-cancelled-notification", &steps),
        (1, vec![])
    );
    // Before any task exists there is nothing a cancellation could misuse.
    let early = [notification(
        "client-to-server",
        "notifications/cancelled",
        json!({"requestId": 9}),
    )];
    assert_eq!(
        judge("tasks.no-cancelled-notification", &early),
        (0, vec![])
    );
}

#[test]
fn an_unanswered_listen_is_not_judged_and_a_refusal_passes() {
    let listen = request(
        1,
        "subscriptions/listen",
        params(json!({"notifications": {"taskIds": ["t-1"]}}), &[]),
    );
    assert_eq!(
        judge(
            "tasks.listen-requires-declaration",
            std::slice::from_ref(&listen)
        ),
        (0, vec![])
    );
    assert_eq!(
        judge(
            "tasks.listen-requires-declaration",
            &[listen, error(1, -32021)]
        ),
        (1, vec![])
    );
}

#[test]
fn only_a_task_listen_stream_binds_its_notifications() {
    let tagged = |method: &str, id: u64| {
        notification(
            "server-to-client",
            method,
            json!({"_meta": {"io.modelcontextprotocol/subscriptionId": id}}),
        )
    };
    let steps = [
        request(
            1,
            "subscriptions/listen",
            params(
                json!({"notifications": {"toolsListChanged": true}}),
                &[TASKS],
            ),
        ),
        request(
            2,
            "subscriptions/listen",
            params(json!({"notifications": {"taskIds": ["t-1"]}}), &[TASKS]),
        ),
        tagged("notifications/message", 1),
        tagged("notifications/tasks", 2),
        tagged("notifications/progress", 2),
    ];
    let (subjects, findings) = judge("tasks.no-progress-on-task-stream", &steps);
    assert_eq!(subjects, 2, "the two notifications on the task stream");
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("notifications/progress"),
        "{findings:?}"
    );
}

#[test]
fn a_declaring_clients_methods_are_served_and_a_refusal_is_the_rule_otherwise() {
    let get = |id, extensions: &[&str]| {
        request(
            id,
            "tasks/get",
            params(json!({"taskId": "t-1"}), extensions),
        )
    };
    let served = with(task("t-1", "working"), "resultType", json!("complete"));
    let steps = [
        get(1, &[TASKS]),
        result(1, served),
        get(2, &[]),
        error(2, -32021),
    ];
    assert_eq!(
        judge("tasks.methods-require-declaration", &steps),
        (1, vec![])
    );
}
