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

/// A notification tagged with subscription `id`.
fn tagged(direction: &'static str, method: &str, id: u64) -> (&'static str, Value) {
    notification(
        direction,
        method,
        json!({"_meta": {"io.modelcontextprotocol/subscriptionId": id}}),
    )
}

#[test]
fn only_a_client_cancellation_after_a_task_is_judged() {
    let cancel = |direction| {
        notification(
            direction,
            "notifications/cancelled",
            json!({"requestId": 1}),
        )
    };
    let mut steps = vec![
        request(1, "tools/call", params(json!({"name": "deploy"}), &[TASKS])),
        // Before the task exists.
        cancel("client-to-server"),
        result(1, with(task("t-1", "working"), "resultType", json!("task"))),
        // Another notification, and the server's own cancellation.
        notification(
            "client-to-server",
            "notifications/progress",
            json!({"requestId": 1}),
        ),
        cancel("server-to-client"),
    ];
    assert_eq!(
        judge("tasks.no-cancelled-notification", &steps),
        (0, vec![])
    );
    steps.push(cancel("client-to-server"));
    assert_eq!(
        judge("tasks.no-cancelled-notification", &steps),
        (
            1,
            vec![
                "`notifications/cancelled` names request 1, which a task answered; a task is \
                 cancelled with `tasks/cancel`"
                    .to_owned()
            ]
        )
    );
}

#[test]
fn only_a_server_notification_on_the_subscription_after_a_listen_acknowledges_it() {
    let listen = request(
        1,
        "subscriptions/listen",
        params(json!({"notifications": {"taskIds": ["t-1"]}}), &[]),
    );
    let server_request = (
        "server-to-client",
        json!({"jsonrpc": "2.0", "id": "s1", "method": "ping",
               "params": {"_meta": {"io.modelcontextprotocol/subscriptionId": 1}}}),
    );
    let initialized = notification("client-to-server", "notifications/initialized", json!({}));
    for steps in [
        // Tagged, but before the listen.
        vec![
            initialized.clone(),
            tagged("server-to-client", "notifications/tasks", 1),
            listen.clone(),
        ],
        vec![
            listen.clone(),
            tagged("client-to-server", "notifications/tasks", 1),
        ],
        vec![
            listen.clone(),
            tagged("server-to-client", "notifications/tasks", 2),
        ],
        vec![listen.clone(), initialized.clone()],
        vec![listen.clone(), server_request],
    ] {
        assert_eq!(
            judge("tasks.listen-requires-declaration", &steps),
            (0, vec![]),
            "{steps:?}"
        );
    }
    assert_eq!(
        judge(
            "tasks.listen-requires-declaration",
            &[listen, tagged("server-to-client", "notifications/tasks", 1)]
        ),
        (
            1,
            vec![format!(
                "`subscriptions/listen` asked for task notifications without declaring `{TASKS}` \
                 and was not refused with `-32021`"
            )]
        )
    );
}

#[test]
fn a_listen_request_from_the_server_is_no_task_listen() {
    let steps = [
        (
            "server-to-client",
            json!({"jsonrpc": "2.0", "id": 1, "method": "subscriptions/listen",
                   "params": {"notifications": {"taskIds": ["t-1"]}}}),
        ),
        (
            "client-to-server",
            json!({"jsonrpc": "2.0", "id": 1, "result": {}}),
        ),
    ];
    assert_eq!(
        judge("tasks.listen-requires-declaration", &steps),
        (0, vec![])
    );
}

#[test]
fn the_latest_listen_carrying_an_id_decides_its_stream() {
    let listen = |id, notifications: Value| {
        request(
            id,
            "subscriptions/listen",
            params(json!({ "notifications": notifications }), &[TASKS]),
        )
    };
    let steps = [
        listen(2, json!({"taskIds": ["t-1"]})),
        listen(1, json!({"toolsListChanged": true})),
        // Not a listen: it opens no subscription 2.
        request(2, "tools/call", params(json!({"name": "x"}), &[TASKS])),
        // The client's own notification is not on the stream.
        tagged("client-to-server", "notifications/progress", 2),
        tagged("server-to-client", "notifications/progress", 2),
        // Subscription 2 reopened without tasks.
        listen(2, json!({"toolsListChanged": true})),
        tagged("server-to-client", "notifications/progress", 2),
    ];
    assert_eq!(
        judge("tasks.no-progress-on-task-stream", &steps),
        (
            1,
            vec![
                "`notifications/progress` was sent on a task's `subscriptions/listen` stream"
                    .to_owned()
            ]
        )
    );
}

/// What `check` reports on a Streamable HTTP trace of `(headers, payload)` POSTs.
fn judge_posts(check: &str, posts: &[(Value, Value)]) -> (u32, Vec<String>) {
    let mut lines = Vec::new();
    for (headers, payload) in posts {
        let seq = lines.len();
        lines.push(json!({"seq": seq, "direction": "client-to-server",
            "transport": "streamable-http", "kind": "http", "method": "POST", "headers": headers}));
        lines.push(json!({"seq": seq + 1, "direction": "client-to-server",
            "transport": "streamable-http", "kind": "message", "payload": payload}));
    }
    let document: Vec<String> = lines.iter().map(ToString::to_string).collect();
    let events =
        crate::reader::parse_trace(&document.join("\n"), &crate::reader::Limits::default())
            .unwrap();
    let outcome = crate::checks::find(check)
        .unwrap()
        .run(&crate::context::TraceContext::new(&events));
    (
        outcome.subjects,
        outcome
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect(),
    )
}

#[test]
fn an_mcp_name_naming_another_task_is_a_mismatch() {
    let post = |id: u64, name: &str| {
        (
            json!({"content-type": "application/json", "mcp-method": "tasks/get", "mcp-name": name}),
            json!({"jsonrpc": "2.0", "id": id, "method": "tasks/get", "params": {"taskId": "t-1"}}),
        )
    };
    assert_eq!(
        judge_posts("tasks.mcp-name-header", &[post(1, "t-1"), post(2, "t-2")]),
        (
            2,
            vec![
                "`Mcp-Name` is \"t-2\" but the `tasks/get` request's `taskId` is \"t-1\""
                    .to_owned()
            ]
        )
    );
}
