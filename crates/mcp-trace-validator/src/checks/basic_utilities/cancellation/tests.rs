// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the cancellation rules.

use crate::checks::basic_utilities::testkit::{
    client, findings_for, http, notify, request, result, run, server, session, trace,
};

const ISSUED: &str = "cancellation.references-issued-request";
const INITIALIZE: &str = "cancellation.initialize-not-cancelled";
const TASK: &str = "cancellation.task-cancelled-by-request";
const ANSWERED: &str = "cancellation.no-response-after-cancellation";

/// A `notifications/cancelled` with `params` (JSON text), from the client when
/// `from_client`.
fn cancel(seq: u64, from_client: bool, params: &str) -> String {
    notify(seq, from_client, "notifications/cancelled", params)
}

/// A client `tools/call` id `id`, or a server `roots/list` when not
/// `from_client`.
fn call(seq: u64, from_client: bool, id: &str) -> String {
    if from_client {
        request(
            seq,
            true,
            id,
            "tools/call",
            r#"{"name":"slow","arguments":{}}"#,
        )
    } else {
        request(seq, false, id, "roots/list", "{}")
    }
}

#[test]
fn a_cancellation_names_a_request_its_sender_issued() {
    let own = session(&[call(3, true, "2"), cancel(4, true, r#"{"requestId":2}"#)]);
    assert_eq!(run(ISSUED, &own), (Vec::<String>::new(), 1));
    let servers_own = session(&[
        call(3, false, r#""s1""#),
        cancel(4, false, r#"{"requestId":"s1","reason":"withdrawn"}"#),
    ]);
    assert_eq!(run(ISSUED, &servers_own), (Vec::<String>::new(), 1));

    let unknown = session(&[cancel(3, true, r#"{"requestId":99,"reason":"stale"}"#)]);
    let findings = findings_for(ISSUED, &unknown);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("request 99, which it never issued") && findings[0].contains("seq 0"),
        "{findings:?}"
    );
}

#[test]
fn the_other_partys_request_and_a_nameless_cancellation_are_reported() {
    let crossed = session(&[
        call(3, false, r#""s1""#),
        cancel(4, true, r#"{"requestId":"s1"}"#),
    ]);
    let findings = findings_for(ISSUED, &crossed);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("the server issued a request with that id"),
        "{findings:?}"
    );
    // The string "2" is not the integer 2 the client issued.
    let retyped = session(&[call(3, true, "2"), cancel(4, true, r#"{"requestId":"2"}"#)]);
    assert_eq!(findings_for(ISSUED, &retyped).len(), 1);
    for params in ["{}", r#"{"requestId":null}"#, r#"{"reason":"x"}"#] {
        let findings = findings_for(ISSUED, &session(&[cancel(3, true, params)]));
        assert_eq!(findings.len(), 1, "{params}: {findings:?}");
        assert!(
            findings[0].contains("no `params.requestId`"),
            "{findings:?}"
        );
    }
}

#[test]
fn without_the_handshake_a_cancellations_provenance_is_not_judged() {
    let headless = trace(&[call(0, true, "2"), cancel(1, true, r#"{"requestId":99}"#)]);
    assert_eq!(run(ISSUED, &headless), (Vec::<String>::new(), 0));
    // A cancellation recorded before the handshake is not the session's.
    let before = trace(&[
        cancel(0, true, r#"{"requestId":99}"#),
        client(
            1,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        ),
    ]);
    assert_eq!(run(ISSUED, &before), (Vec::<String>::new(), 0));
    // Over Streamable HTTP the witness is the same handshake.
    let over_http = trace(&[
        http(
            0,
            true,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        ),
        http(
            1,
            true,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}"#,
        ),
    ]);
    assert_eq!(findings_for(ISSUED, &over_http).len(), 1);
}

#[test]
fn cancelling_initialize_is_reported_and_cancelling_anything_else_is_not() {
    let cancelled = trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        ),
        cancel(1, true, r#"{"requestId":1,"reason":"giving up"}"#),
    ]);
    let (findings, subjects) = run(INITIALIZE, &cancelled);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("`initialize` request (id 1)"),
        "{findings:?}"
    );
    assert_eq!(subjects, 1);

    let other = session(&[call(3, true, "2"), cancel(4, true, r#"{"requestId":2}"#)]);
    assert_eq!(run(INITIALIZE, &other), (Vec::<String>::new(), 1));
    // Id 1 reused for a later request: the cancellation names that one.
    let reused = session(&[call(3, true, "1"), cancel(4, true, r#"{"requestId":1}"#)]);
    assert_eq!(run(INITIALIZE, &reused), (Vec::<String>::new(), 1));
}

#[test]
fn unresolvable_and_server_cancellations_are_not_the_initialize_rules() {
    // An id the recording never saw a client request for.
    let unknown = session(&[
        cancel(3, true, r#"{"requestId":99}"#),
        cancel(4, true, "{}"),
    ]);
    assert_eq!(run(INITIALIZE, &unknown), (Vec::<String>::new(), 0));
    // The server cancelling id 1 names its own namespace, not the handshake.
    let servers = session(&[call(3, false, "1"), cancel(4, false, r#"{"requestId":1}"#)]);
    assert_eq!(run(INITIALIZE, &servers), (Vec::<String>::new(), 0));
}

/// A session whose server supports task-augmented `tools/call`, with `rest`
/// from seq 3.
fn tasks_session(rest: &[String]) -> String {
    let mut lines = crate::checks::basic_utilities::testkit::handshake(
        "{}",
        r#"{"tools":{},"tasks":{"requests":{"tools":{"call":{}}}}}"#,
    );
    lines.extend_from_slice(rest);
    trace(&lines)
}

/// A task-augmented `tools/call` id `id` answered with task `task_id`.
fn task(seq: u64, id: &str, task_id: &str) -> [String; 2] {
    [
        request(
            seq,
            true,
            id,
            "tools/call",
            r#"{"name":"deploy","arguments":{},"task":{"ttl":60000}}"#,
        ),
        result(
            seq + 1,
            false,
            id,
            &format!(
                r#"{{"task":{{"taskId":"{task_id}","status":"working","createdAt":"2025-11-25T10:30:00Z","lastUpdatedAt":"2025-11-25T10:30:00Z","ttl":60000}}}}"#
            ),
        ),
    ]
}

#[test]
fn a_task_is_not_cancelled_by_notification() {
    let mut lines = task(3, "2", "task-1").to_vec();
    lines.push(cancel(5, true, r#"{"requestId":2,"reason":"stop"}"#));
    let (findings, subjects) = run(TASK, &tasks_session(&lines));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains(r#"answered with task "task-1""#),
        "{findings:?}"
    );
    assert_eq!(subjects, 1);

    // Another request cancelled once a task exists: the proper notification.
    let mut lines = task(3, "2", "task-1").to_vec();
    lines.push(call(5, true, "3"));
    lines.push(cancel(6, true, r#"{"requestId":3}"#));
    assert_eq!(run(TASK, &tasks_session(&lines)), (Vec::<String>::new(), 1));
}

#[test]
fn before_any_task_exists_and_for_requests_merely_asking_nothing_is_judged() {
    // Asked for a task, answered as an ordinary call: cancel it by notification.
    let plain = tasks_session(&[
        request(
            3,
            true,
            "2",
            "tools/call",
            r#"{"name":"deploy","arguments":{},"task":{}}"#,
        ),
        result(4, false, "2", r#"{"content":[]}"#),
        cancel(5, true, r#"{"requestId":2}"#),
    ]);
    assert_eq!(run(TASK, &plain), (Vec::<String>::new(), 0));
    // The cancellation precedes the task's creation.
    let early = tasks_session(&[
        request(
            3,
            true,
            "2",
            "tools/call",
            r#"{"name":"deploy","arguments":{},"task":{}}"#,
        ),
        cancel(4, true, r#"{"requestId":2}"#),
        result(
            5,
            false,
            "2",
            r#"{"task":{"taskId":"t","status":"working","createdAt":"x","lastUpdatedAt":"x","ttl":null}}"#,
        ),
    ]);
    assert_eq!(run(TASK, &early), (Vec::<String>::new(), 0));
    // The server's tasks are the client's business, not the server's own.
    let mut lines = task(3, "2", "task-1").to_vec();
    lines.push(cancel(5, false, r#"{"requestId":2}"#));
    assert_eq!(run(TASK, &tasks_session(&lines)), (Vec::<String>::new(), 0));
}

#[test]
fn an_answer_after_the_cancellation_was_demonstrably_read_is_reported() {
    let knowing = session(&[
        call(3, true, "2"),
        cancel(4, true, r#"{"requestId":2}"#),
        call(5, true, "3"),
        result(6, false, "3", r#"{"content":[]}"#),
        result(7, false, "2", r#"{"content":[]}"#),
    ]);
    let (findings, subjects) = run(ANSWERED, &knowing);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("answered request 2, which the client cancelled at seq 4"),
        "{findings:?}"
    );
    assert_eq!(subjects, 2);

    // The client as receiver: the server withdrew a request of its own.
    let client_knowing = session(&[
        call(3, false, r#""s1""#),
        cancel(4, false, r#"{"requestId":"s1"}"#),
        request(5, false, r#""s2""#, "ping", "{}"),
        result(6, true, r#""s2""#, "{}"),
        result(7, true, r#""s1""#, r#"{"roots":[]}"#),
    ]);
    let findings = findings_for(ANSWERED, &client_knowing);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].starts_with("the client answered"),
        "{findings:?}"
    );
}

#[test]
fn the_race_a_reused_id_and_http_are_not_reported() {
    // The answer crosses the cancellation: nothing shows it was read.
    let race = session(&[
        call(3, true, "2"),
        cancel(4, true, r#"{"requestId":2}"#),
        result(5, false, "2", r#"{"content":[]}"#),
        call(6, true, "3"),
        result(7, false, "3", r#"{"content":[]}"#),
    ]);
    assert_eq!(run(ANSWERED, &race), (Vec::<String>::new(), 2));
    // Id 2 reused after its answer: the new request is not the cancelled one.
    let reused = session(&[
        call(3, true, "2"),
        cancel(4, true, r#"{"requestId":2}"#),
        result(5, false, "2", r#"{"content":[]}"#),
        call(6, true, "3"),
        result(7, false, "3", r#"{"content":[]}"#),
        call(8, true, "2"),
        result(9, false, "2", r#"{"content":[]}"#),
    ]);
    assert_eq!(findings_for(ANSWERED, &reused), Vec::<String>::new());
    // Over Streamable HTTP a later POST may be served first: no order to read.
    let over_http = trace(&[
        http(
            0,
            true,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"slow"}}"#,
        ),
        http(
            1,
            true,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":2}}"#,
        ),
        http(2, true, r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#),
        http(
            3,
            false,
            r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}"#,
        ),
        http(
            4,
            false,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[]}}"#,
        ),
    ]);
    assert_eq!(run(ANSWERED, &over_http), (Vec::<String>::new(), 0));
    // Nothing cancelled, and an answer to nothing the client sent: no subject.
    let quiet = session(&[
        call(3, true, "2"),
        server(4, r#"{"jsonrpc":"2.0","id":9,"result":{}}"#),
    ]);
    assert_eq!(run(ANSWERED, &quiet), (Vec::<String>::new(), 0));
}
