// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for a token's lifetime: progress after the answer, after a task's
//! terminal status, and a task's progress under another token.

use crate::checks::basic_utilities::progress::tests::{answer, ask, progress};
use crate::checks::basic_utilities::testkit::{
    findings_for, handshake, http, notify, request, result, run, trace,
};

const AFTER: &str = "progress.stops-after-completion";
const TASK_TOKEN: &str = "progress.task-token-unchanged";

/// A handshake whose server declares `tools` and task-augmented `tools/call`,
/// followed by `rest` (from seq 3).
fn tasks_session(rest: &[String]) -> String {
    let mut lines = handshake(
        "{}",
        r#"{"tools":{},"tasks":{"requests":{"tools":{"call":{}}}}}"#,
    );
    lines.extend_from_slice(rest);
    trace(&lines)
}

/// A task-augmented `tools/call` id `id` opting into progress with `token`
/// (JSON text) when given.
fn task_call(seq: u64, id: &str, token: Option<&str>) -> String {
    let meta = token.map_or_else(String::new, |token| {
        format!(r#","_meta":{{"progressToken":{token}}}"#)
    });
    request(
        seq,
        true,
        id,
        "tools/call",
        &format!(r#"{{"name":"deploy","arguments":{{}},"task":{{"ttl":60000}}{meta}}}"#),
    )
}

/// A `Task` object for `task_id` in `status`, as JSON text.
fn task_object(task_id: &str, status: &str) -> String {
    format!(
        r#"{{"taskId":"{task_id}","status":"{status}","createdAt":"2025-11-25T10:30:00Z","lastUpdatedAt":"2025-11-25T10:31:00Z","ttl":60000}}"#
    )
}

/// The `CreateTaskResult` answering `id` with a working task `task_id`.
fn created(seq: u64, id: &str, task_id: &str) -> String {
    result(
        seq,
        false,
        id,
        &format!(r#"{{"task":{}}}"#, task_object(task_id, "working")),
    )
}

/// The server's progress for task `task_id` under `token` (JSON text).
fn task_progress(seq: u64, task_id: &str, token: &str, value: &str) -> String {
    notify(
        seq,
        false,
        "notifications/progress",
        &format!(
            r#"{{"progressToken":{token},"progress":{value},"_meta":{{"io.modelcontextprotocol/related-task":{{"taskId":"{task_id}"}}}}}}"#
        ),
    )
}

/// A `tasks/get` (or `tasks/cancel`, with `cancel`) for `task_id` answered with
/// the task in `status`, at seq `seq` and `seq + 1` under id `id`.
fn polled(seq: u64, id: &str, task_id: &str, status: &str, cancel: bool) -> [String; 2] {
    let method = if cancel { "tasks/cancel" } else { "tasks/get" };
    [
        request(
            seq,
            true,
            id,
            method,
            &format!(r#"{{"taskId":"{task_id}"}}"#),
        ),
        result(seq + 1, false, id, &task_object(task_id, status)),
    ]
}

#[test]
fn progress_after_the_answer_is_reported_on_stdio_in_either_direction() {
    let late = tasks_session(&[
        ask(3, true, "2", Some(r#""t""#)),
        progress(4, false, r#""t""#, "1"),
        answer(5, false, "2"),
        progress(6, false, r#""t""#, "2"),
    ]);
    let (findings, subjects) = run(AFTER, &late);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("(seq 3) was answered at seq 5"),
        "{findings:?}"
    );
    assert_eq!(subjects, 2);

    let client_late = tasks_session(&[
        ask(3, false, r#""s1""#, Some(r#""r""#)),
        answer(4, true, r#""s1""#),
        progress(5, true, r#""r""#, "1"),
    ]);
    assert_eq!(findings_for(AFTER, &client_late).len(), 1);
}

#[test]
fn progress_before_the_answer_or_for_a_reused_token_is_not() {
    let reused = tasks_session(&[
        ask(3, true, "2", Some(r#""t""#)),
        progress(4, false, r#""t""#, "1"),
        answer(5, false, "2"),
        ask(6, true, "3", Some(r#""t""#)),
        progress(7, false, r#""t""#, "1"),
        // Never answered in the recording: still in progress.
        ask(8, true, "4", Some(r#""u""#)),
        progress(9, false, r#""u""#, "1"),
    ]);
    assert_eq!(run(AFTER, &reused), (Vec::<String>::new(), 3));
}

#[test]
fn unknown_tokens_and_http_recordings_are_not_judged() {
    // A token no recorded request carried: `token-issued-in-session`'s to decide.
    let unknown = tasks_session(&[answer(3, false, "2"), progress(4, false, r#""t""#, "1")]);
    assert_eq!(run(AFTER, &unknown), (Vec::<String>::new(), 0));
    // Over Streamable HTTP the two may ride different streams.
    let over_http = trace(&[
        http(
            0,
            true,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"export","_meta":{"progressToken":"t"}}}"#,
        ),
        http(
            1,
            false,
            r#"{"jsonrpc":"2.0","id":2,"result":{"content":[]}}"#,
        ),
        http(
            2,
            false,
            r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"t","progress":1}}"#,
        ),
    ]);
    assert_eq!(run(AFTER, &over_http), (Vec::<String>::new(), 0));
}

#[test]
fn a_tasks_token_outlives_the_create_task_result_until_a_terminal_status() {
    let mut lines = vec![
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        task_progress(5, "task-1", r#""d""#, "0.5"),
    ];
    lines.extend(polled(6, "3", "task-1", "working", false));
    lines.push(task_progress(8, "task-1", r#""d""#, "0.7"));
    lines.extend(polled(9, "4", "task-1", "input_required", false));
    lines.push(task_progress(11, "task-1", r#""d""#, "0.9"));
    assert_eq!(
        run(AFTER, &tasks_session(&lines)),
        (Vec::<String>::new(), 3)
    );

    // A task-augmented request answered with an ordinary result finished then.
    let plain = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        answer(4, false, "2"),
        task_progress(5, "task-1", r#""d""#, "1"),
    ]);
    assert_eq!(findings_for(AFTER, &plain).len(), 1);
}

#[test]
fn progress_after_the_task_ends_is_reported_whichever_message_ended_it() {
    for (status, cancel) in [("completed", false), ("failed", false), ("cancelled", true)] {
        let mut lines = vec![task_call(3, "2", Some(r#""d""#)), created(4, "2", "task-1")];
        lines.extend(polled(5, "3", "task-1", status, cancel));
        lines.push(task_progress(7, "task-1", r#""d""#, "1"));
        let findings = findings_for(AFTER, &tasks_session(&lines));
        assert_eq!(findings.len(), 1, "{status}: {findings:?}");
        assert!(
            findings[0].contains("reached a terminal status at seq 6"),
            "{findings:?}"
        );
    }
    // A status notification ends it too, and so does a `tasks/result` answer.
    let notified = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        notify(
            5,
            false,
            "notifications/tasks/status",
            &task_object("task-1", "completed"),
        ),
        task_progress(6, "task-1", r#""d""#, "1"),
    ]);
    assert_eq!(findings_for(AFTER, &notified).len(), 1);
    let fetched = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        request(5, true, "3", "tasks/result", r#"{"taskId":"task-1"}"#),
        result(6, false, "3", r#"{"content":[]}"#),
        task_progress(7, "task-1", r#""d""#, "1"),
    ]);
    assert_eq!(findings_for(AFTER, &fetched).len(), 1);
    // A non-terminal status notification, or a terminal one for another task,
    // ends nothing.
    let other = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        notify(
            5,
            false,
            "notifications/tasks/status",
            &task_object("task-1", "working"),
        ),
        notify(
            6,
            false,
            "notifications/tasks/status",
            &task_object("task-9", "completed"),
        ),
        task_progress(7, "task-1", r#""d""#, "1"),
    ]);
    assert_eq!(run(AFTER, &other), (Vec::<String>::new(), 1));
}

#[test]
fn a_tasks_progress_keeps_the_tokens_its_request_provided() {
    let kept = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        task_progress(5, "task-1", r#""d""#, "0.5"),
    ]);
    assert_eq!(run(TASK_TOKEN, &kept), (Vec::<String>::new(), 1));

    let changed = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        task_progress(5, "task-1", r#""e""#, "0.5"),
    ]);
    let findings = findings_for(TASK_TOKEN, &changed);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains(r#"carries token "e", not the token "d""#),
        "{findings:?}"
    );
    // The integer 7 is not the string "7".
    let retyped = tasks_session(&[
        task_call(3, "2", Some("7")),
        created(4, "2", "task-1"),
        task_progress(5, "task-1", r#""7""#, "0.5"),
    ]);
    assert_eq!(findings_for(TASK_TOKEN, &retyped).len(), 1);
}

#[test]
fn a_task_the_recording_never_saw_created_is_not_judged() {
    // No `_meta` tying the notification to a task: a plain notification.
    let untied = tasks_session(&[
        task_call(3, "2", Some(r#""d""#)),
        created(4, "2", "task-1"),
        progress(5, false, r#""e""#, "0.5"),
    ]);
    assert_eq!(run(TASK_TOKEN, &untied), (Vec::<String>::new(), 0));
    // A task id no `CreateTaskResult` in the recording named.
    let unknown = tasks_session(&[task_progress(3, "task-9", r#""e""#, "0.5")]);
    assert_eq!(run(TASK_TOKEN, &unknown), (Vec::<String>::new(), 0));
    // A task created for a request that carried no token leaves nothing to keep.
    let tokenless = tasks_session(&[
        task_call(3, "2", None),
        created(4, "2", "task-1"),
        task_progress(5, "task-1", r#""e""#, "0.5"),
    ]);
    assert_eq!(run(TASK_TOKEN, &tokenless), (Vec::<String>::new(), 0));
}
