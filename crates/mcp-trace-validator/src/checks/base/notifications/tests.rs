// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for `base.notification-unanswered`: the one response a notification
//! can draw that nothing else explains, and every guard that keeps a lawful
//! null-id error out of it.

#![allow(clippy::expect_used)]

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

const CHECK: &str = "base.notification-unanswered";

const CANCELLED: &str =
    r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":99}}"#;
const NULL_ERROR: &str = r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"x"}}"#;
const PING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
const PONG: &str = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;

fn line(seq: usize, transport: &str, direction: &str, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"{transport}","kind":"message","payload":{payload}}}"#
    )
}

/// A trace of `(sender, payload)` messages over `transport`, `c` for the
/// client and anything else for the server.
fn trace_over(transport: &str, messages: &[(&str, &str)]) -> String {
    messages
        .iter()
        .enumerate()
        .map(|(seq, (sender, payload))| {
            let direction = if *sender == "c" {
                "client-to-server"
            } else {
                "server-to-client"
            };
            line(seq, transport, direction, payload)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The finding seqs and subject count of the check over a stdio trace.
fn run(trace: &str) -> (Vec<Option<u64>>, u32) {
    let events = parse_trace(trace, &Limits::default()).expect("test trace parses");
    let context = TraceContext::new(&events);
    let outcome = checks::find(CHECK)
        .expect("check is registered")
        .run(&context);
    (
        outcome.findings.iter().map(|found| found.seq).collect(),
        outcome.subjects,
    )
}

fn stdio(messages: &[(&str, &str)]) -> (Vec<Option<u64>>, u32) {
    run(&trace_over("stdio", messages))
}

#[test]
fn a_null_id_response_right_after_a_notification_answers_it() {
    assert_eq!(
        stdio(&[("c", CANCELLED), ("s", NULL_ERROR)]),
        (vec![Some(1)], 1)
    );
    // An absent id, and a result rather than an error, are the same response.
    for response in [
        r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"x"}}"#,
        r#"{"jsonrpc":"2.0","id":null,"result":{}}"#,
        r#"{"jsonrpc":"2.0","result":{}}"#,
    ] {
        assert_eq!(
            stdio(&[("c", CANCELLED), ("s", response)]),
            (vec![Some(1)], 1),
            "{response}"
        );
    }
    // A notification with no params at all is as well-formed.
    let bare = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    assert_eq!(stdio(&[("c", bare), ("s", NULL_ERROR)]).0, [Some(1)]);
}

#[test]
fn the_finding_names_both_parties_and_the_notification() {
    let details = |trace: &str| -> Vec<String> {
        let events = parse_trace(trace, &Limits::default()).expect("test trace parses");
        let context = TraceContext::new(&events);
        let outcome = checks::find(CHECK)
            .expect("check is registered")
            .run(&context);
        outcome
            .findings
            .into_iter()
            .map(|found| found.detail)
            .collect()
    };
    let by_server = details(&trace_over("stdio", &[("c", CANCELLED), ("s", NULL_ERROR)]));
    assert_eq!(by_server.len(), 1);
    assert!(
        by_server[0].starts_with("the server sent a response with no id")
            && by_server[0].contains("the client's `notifications/cancelled` notification (seq 0)"),
        "{by_server:?}"
    );
    // The clause binds both parties: a client answering a server notification.
    let by_client = details(&trace_over("stdio", &[("s", CANCELLED), ("c", NULL_ERROR)]));
    assert_eq!(by_client.len(), 1);
    assert!(
        by_client[0].starts_with("the client sent a response with no id")
            && by_client[0].contains("the server's `notifications/cancelled` notification"),
        "{by_client:?}"
    );
}

#[test]
fn a_notification_left_unanswered_is_a_subject_and_no_finding() {
    assert_eq!(stdio(&[("c", CANCELLED)]), (vec![], 1));
    assert_eq!(
        stdio(&[("c", CANCELLED), ("c", PING), ("s", PONG)]),
        (vec![], 1)
    );
    // No notification, no subject — and a stray null-id error is not this
    // clause's.
    assert_eq!(stdio(&[("c", PING), ("s", PONG)]), (vec![], 0));
    assert_eq!(stdio(&[("s", NULL_ERROR)]), (vec![], 0));
}

#[test]
fn a_request_answered_by_id_does_not_hide_the_response() {
    // The request sent after the notification is answered by id later, so the
    // null-id response is not its answer.
    assert_eq!(
        stdio(&[
            ("c", CANCELLED),
            ("c", PING),
            ("s", NULL_ERROR),
            ("s", PONG)
        ]),
        (vec![Some(2)], 1)
    );
    // Answered before the notification: the same.
    assert_eq!(
        stdio(&[
            ("c", PING),
            ("s", PONG),
            ("c", CANCELLED),
            ("s", NULL_ERROR)
        ]),
        (vec![Some(3)], 1)
    );
}

#[test]
fn a_request_never_answered_by_id_may_be_what_the_response_answers() {
    assert_eq!(
        stdio(&[("c", CANCELLED), ("c", PING), ("s", NULL_ERROR)]),
        (vec![], 1)
    );
    // Sent before the notification, it spoils the notification as a subject.
    assert_eq!(
        stdio(&[("c", PING), ("c", CANCELLED), ("s", NULL_ERROR)]),
        (vec![], 0)
    );
}

#[test]
fn a_malformed_message_from_the_sender_ends_the_judgment() {
    for malformed in [
        // Not JSON: the recorder writes the line as a string.
        r#""{not json""#,
        // No `jsonrpc` member, and the wrong one.
        r#"{"method":"notifications/x"}"#,
        r#"{"jsonrpc":"1.0","method":"notifications/x"}"#,
        r#"{"jsonrpc":2,"method":"notifications/x"}"#,
        // `params` that is not an object, on a notification and on a request.
        r#"{"jsonrpc":"2.0","method":"notifications/x","params":5}"#,
        // An id that cannot be echoed.
        r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
        r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#,
        // Neither request, notification nor response.
        r#"{"jsonrpc":"2.0","id":4}"#,
    ] {
        assert_eq!(
            stdio(&[("c", malformed), ("c", CANCELLED), ("s", NULL_ERROR)]),
            (vec![], 0),
            "{malformed}"
        );
    }
    // A malformed notification is not itself a subject.
    let bad = r#"{"jsonrpc":"2.0","method":"notifications/x","params":[1]}"#;
    assert_eq!(stdio(&[("c", bad), ("s", NULL_ERROR)]), (vec![], 0));
    // An answered request whose params are not an object spoils too: the
    // null-id error may be a second word on it.
    let odd = r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":"x"}"#;
    assert_eq!(
        stdio(&[("c", odd), ("s", PONG), ("c", CANCELLED), ("s", NULL_ERROR)]),
        (vec![], 0)
    );
    // String and number ids are both readable, and a well-formed response
    // from the sender spoils nothing.
    let named = r#"{"jsonrpc":"2.0","id":"a","method":"ping","params":{}}"#;
    let named_answer = r#"{"jsonrpc":"2.0","id":"a","result":{}}"#;
    assert_eq!(
        stdio(&[
            ("c", named),
            ("s", named_answer),
            ("c", CANCELLED),
            ("s", NULL_ERROR)
        ]),
        (vec![Some(3)], 1)
    );
    // The peer's malformed messages do not spoil this party's notifications.
    assert_eq!(
        stdio(&[("s", r#""noise""#), ("c", CANCELLED), ("s", NULL_ERROR)]),
        (vec![Some(2)], 1)
    );
}

#[test]
fn only_the_receivers_next_message_is_attributed() {
    // The server said something else first: the window is closed.
    let log =
        r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":1}}"#;
    assert_eq!(
        stdio(&[("c", CANCELLED), ("s", log), ("s", NULL_ERROR)]),
        (vec![], 2)
    );
    // A response of the sender's own in the window, on either side of the
    // notification, could be what the null-id response reacts to.
    let ask = r#"{"jsonrpc":"2.0","id":"s1","method":"ping"}"#;
    let reply = r#"{"jsonrpc":"2.0","id":"s1","result":{}}"#;
    assert_eq!(
        stdio(&[
            ("s", ask),
            ("c", reply),
            ("c", CANCELLED),
            ("s", NULL_ERROR)
        ]),
        (vec![], 1)
    );
    assert_eq!(
        stdio(&[
            ("s", ask),
            ("c", CANCELLED),
            ("c", reply),
            ("s", NULL_ERROR)
        ]),
        (vec![], 1)
    );
    // Once the server has spoken, an earlier response of the client's no
    // longer stands between a new notification and its answer.
    assert_eq!(
        stdio(&[
            ("s", ask),
            ("c", reply),
            ("s", log),
            ("c", CANCELLED),
            ("s", NULL_ERROR)
        ]),
        (vec![Some(4)], 2)
    );
    // A response that names an id is the correlation checks' business.
    let unknown = r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32601,"message":"x"}}"#;
    assert_eq!(stdio(&[("c", CANCELLED), ("s", unknown)]), (vec![], 1));
    // One response answers one notification: a second null-id error follows
    // the first, not the notification.
    assert_eq!(
        stdio(&[("c", CANCELLED), ("s", NULL_ERROR), ("s", NULL_ERROR)]).0,
        [Some(1)]
    );
}

#[test]
fn streamable_http_is_left_to_the_transport_clause() {
    let trace = trace_over("streamable-http", &[("c", CANCELLED), ("s", NULL_ERROR)]);
    assert_eq!(run(&trace), (vec![], 0));
}
