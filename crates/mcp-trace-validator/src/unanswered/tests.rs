// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

// The helpers take `json!` values by value so a call site reads as the record
// it builds; nothing here is hot.
#![allow(clippy::unwrap_used, clippy::needless_pass_by_value)]

use serde_json::json;

use super::find;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};
use crate::report::{LISTED_UNANSWERED, Party, TraceEnding, UnansweredRequests};

const C: &str = "client-to-server";
const S: &str = "server-to-client";

/// One stdio message event.
fn message(seq: u64, direction: &str, payload: serde_json::Value) -> String {
    json!({"seq": seq, "direction": direction, "transport": "stdio", "kind": "message", "payload": payload}).to_string()
}

/// One stdio lifecycle event.
fn lifecycle(seq: u64, direction: &str, event: &str) -> String {
    json!({"seq": seq, "direction": direction, "transport": "stdio", "kind": "lifecycle", "event": event}).to_string()
}

fn request(seq: u64, direction: &str, id: serde_json::Value, method: &str) -> String {
    message(
        seq,
        direction,
        json!({"jsonrpc": "2.0", "id": id, "method": method}),
    )
}

fn result(seq: u64, direction: &str, id: serde_json::Value) -> String {
    message(
        seq,
        direction,
        json!({"jsonrpc": "2.0", "id": id, "result": {}}),
    )
}

fn note(lines: &[String]) -> Option<UnansweredRequests> {
    let events = parse_trace(&lines.join("\n"), &Limits::default()).unwrap();
    find(&TraceContext::new(&events))
}

#[test]
fn every_request_answered_is_no_note() {
    let lines = [
        request(0, C, json!(1), "ping"),
        result(1, S, json!(1)),
        request(2, S, json!("s1"), "roots/list"),
        result(3, C, json!("s1")),
        lifecycle(4, S, "transport-close"),
    ];
    assert_eq!(note(&lines), None);
    assert_eq!(note(&[]), None);
}

#[test]
fn an_unanswered_request_is_listed_with_how_the_trace_ended() {
    let closed = note(&[
        request(0, C, json!(2), "tools/list"),
        lifecycle(1, S, "transport-close"),
    ])
    .unwrap();
    assert_eq!(closed.count, 1);
    assert_eq!(closed.ending, TraceEnding::TransportClose);
    assert_eq!(closed.requests.len(), 1);
    let listed = &closed.requests[0];
    assert_eq!(
        (
            listed.method.as_str(),
            &listed.id,
            listed.seq,
            listed.sender
        ),
        ("tools/list", &json!(2), 0, Party::Client)
    );
    assert_eq!(listed.describe(), "client \"tools/list\" id 2 at seq 0");
    assert_eq!(
        closed.summary(),
        "1 request had no response when the trace ended (the transport closed)"
    );

    let aborted = note(&[
        request(0, S, json!("s1"), "sampling/createMessage"),
        lifecycle(1, C, "transport-abort"),
    ])
    .unwrap();
    assert_eq!(aborted.ending, TraceEnding::TransportAbort);
    assert_eq!(aborted.requests[0].sender, Party::Server);
    assert_eq!(
        aborted.requests[0].describe(),
        "server \"sampling/createMessage\" id \"s1\" at seq 0"
    );

    // A trace whose last event is a message, or an HTTP event, simply stopped.
    let stopped = note(&[request(0, C, json!(1), "ping")]).unwrap();
    assert_eq!(stopped.ending, TraceEnding::Stopped);
    let http = json!({"seq": 1, "direction": S, "transport": "streamable-http", "kind": "http", "status": 202}).to_string();
    let accepted = note(&[request(0, C, json!(1), "ping"), http]).unwrap();
    assert_eq!(accepted.ending, TraceEnding::Stopped);
    // A close that is not the last event is not how the trace ended.
    let reopened = note(&[
        lifecycle(0, C, "transport-close"),
        request(1, C, json!(1), "ping"),
    ])
    .unwrap();
    assert_eq!(reopened.ending, TraceEnding::Stopped);
}

#[test]
fn one_of_three_answered_leaves_two_listed_in_seq_order() {
    let unanswered = note(&[
        request(0, C, json!(2), "tools/list"),
        result(1, S, json!(2)),
        request(2, C, json!(3), "ping"),
        request(3, C, json!(4), "tools/call"),
    ])
    .unwrap();
    assert_eq!(unanswered.count, 2);
    let seqs: Vec<u64> = unanswered
        .requests
        .iter()
        .map(|request| request.seq)
        .collect();
    assert_eq!(seqs, [2, 3]);
    assert_eq!(unanswered.unlisted(), 0);
    assert_eq!(
        unanswered.one_line(),
        "2 requests had no response when the trace ended (the recording simply stopped): \
         client \"ping\" id 3 at seq 2; client \"tools/call\" id 4 at seq 3"
    );
}

#[test]
fn an_answer_out_of_order_still_answers() {
    let lines = [
        request(0, C, json!(1), "tools/list"),
        request(1, C, json!(2), "ping"),
        result(2, S, json!(2)),
        result(3, S, json!(1)),
    ];
    assert_eq!(note(&lines), None);
    // An error answers too.
    let refused = [
        request(0, C, json!(1), "tools/list"),
        message(
            1,
            S,
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -32601, "message": "no"}}),
        ),
    ];
    assert_eq!(note(&refused), None);
}

#[test]
fn a_cancelled_request_is_not_listed() {
    let cancel = |seq, direction, id: serde_json::Value| {
        message(
            seq,
            direction,
            json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": id, "reason": "user"}}),
        )
    };
    // Cancelled by its sender, after it was sent: withdrawn.
    assert_eq!(
        note(&[
            request(0, C, json!(7), "tools/call"),
            cancel(1, C, json!(7))
        ]),
        None
    );
    // The id in canonical form: `7.0` names request 7.
    assert_eq!(
        note(&[
            request(0, C, json!(7), "tools/call"),
            cancel(1, C, json!(7.0))
        ]),
        None
    );
    // A cancellation from the other party names a request of its own, not this one.
    assert_eq!(
        note(&[
            request(0, C, json!(7), "tools/call"),
            cancel(1, S, json!(7))
        ])
        .unwrap()
        .count,
        1
    );
    // A cancellation before the request cannot withdraw it.
    assert_eq!(
        note(&[
            cancel(0, C, json!(7)),
            request(1, C, json!(7), "tools/call")
        ])
        .unwrap()
        .count,
        1
    );
    // A cancellation of another id leaves it pending.
    assert_eq!(
        note(&[
            request(0, C, json!(7), "tools/call"),
            cancel(1, C, json!(8))
        ])
        .unwrap()
        .count,
        1
    );
    // A cancellation without a `requestId` names nothing.
    let bare = message(
        1,
        C,
        json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {}}),
    );
    assert_eq!(
        note(&[request(0, C, json!(7), "tools/call"), bare])
            .unwrap()
            .count,
        1
    );
}

/// Only `notifications/cancelled` withdraws a request: another notification
/// whose params happen to carry a matching `requestId` does not.
#[test]
fn only_a_cancellation_withdraws_a_request() {
    let other = message(
        1,
        C,
        json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"requestId": 7}}),
    );
    assert_eq!(
        note(&[request(0, C, json!(7), "tools/call"), other])
            .unwrap()
            .count,
        1
    );
}

/// A Streamable HTTP record.
fn http(seq: u64, direction: &str, rest: serde_json::Value) -> String {
    let mut record = json!({"seq": seq, "direction": direction, "transport": "streamable-http"});
    for (key, value) in rest.as_object().unwrap() {
        record[key] = value.clone();
    }
    record.to_string()
}

#[test]
fn http_refusals_and_streamed_answers_are_read_as_the_context_reads_them() {
    let post = |seq| {
        http(
            seq,
            C,
            json!({"kind": "http", "method": "POST", "headers": {"accept": "application/json, text/event-stream"}}),
        )
    };
    let ask = |seq, id: serde_json::Value| {
        http(
            seq,
            C,
            json!({"kind": "message", "payload": {"jsonrpc": "2.0", "id": id, "method": "tools/list"}}),
        )
    };
    // A request a protected resource refused with 401 was never read; the
    // retry is the request, and it is answered.
    let refused_then_retried = [
        post(0),
        ask(1, json!(1)),
        http(
            2,
            S,
            json!({"kind": "http", "status": 401, "headers": {"www-authenticate": "Bearer resource_metadata=\"https://s/.well-known/oauth-protected-resource\""}}),
        ),
        post(3),
        ask(4, json!(1)),
        http(
            5,
            S,
            json!({"kind": "http", "status": 200, "headers": {"content-type": "text/event-stream"}}),
        ),
        // Streamed over SSE: the recorder writes the message like any other.
        http(
            6,
            S,
            json!({"kind": "message", "payload": {"jsonrpc": "2.0", "id": 1, "result": {"tools": []}}}),
        ),
    ];
    assert_eq!(note(&refused_then_retried), None);

    // A request a server meets with 202 and nothing else was not answered.
    let accepted_only = [
        post(0),
        ask(1, json!(1)),
        http(2, S, json!({"kind": "http", "status": 202})),
    ];
    let unanswered = note(&accepted_only).unwrap();
    assert_eq!(unanswered.count, 1);
    assert_eq!(unanswered.requests[0].seq, 1);
    assert_eq!(unanswered.ending, TraceEnding::Stopped);
}

#[test]
fn more_than_the_listed_number_are_counted_not_listed() {
    let extra = 2;
    let lines: Vec<String> = (0..LISTED_UNANSWERED + extra)
        .map(|n| request(n as u64, C, json!(n), "ping"))
        .collect();
    let unanswered = note(&lines).unwrap();
    assert_eq!(unanswered.count as usize, LISTED_UNANSWERED + extra);
    assert_eq!(unanswered.requests.len(), LISTED_UNANSWERED);
    assert_eq!(unanswered.unlisted() as usize, extra);
    assert!(unanswered.one_line().ends_with("; and 2 more"));
    assert_eq!(
        unanswered.requests.last().unwrap().seq,
        u64::try_from(LISTED_UNANSWERED - 1).unwrap()
    );
    // Exactly the listed number: nothing left over, nothing counted.
    let exact: Vec<String> = (0..LISTED_UNANSWERED)
        .map(|n| request(n as u64, C, json!(n), "ping"))
        .collect();
    let unanswered = note(&exact).unwrap();
    assert_eq!(unanswered.unlisted(), 0);
    assert!(!unanswered.one_line().contains(" more"));
}

#[test]
fn the_endings_and_the_note_render_as_documented() {
    assert_eq!(
        TraceEnding::TransportClose.describe(),
        "the transport closed"
    );
    assert_eq!(
        TraceEnding::TransportAbort.describe(),
        "the transport aborted"
    );
    assert_eq!(
        TraceEnding::Stopped.describe(),
        "the recording simply stopped"
    );
    assert_eq!(
        serde_json::to_value(TraceEnding::TransportAbort).unwrap(),
        "transport-abort"
    );
    let unanswered = note(&[
        request(0, C, json!("a"), "ping"),
        lifecycle(1, S, "transport-close"),
    ])
    .unwrap();
    let json = serde_json::to_value(&unanswered).unwrap();
    assert_eq!(
        json,
        json!({"count": 1, "ending": "transport-close",
               "requests": [{"method": "ping", "id": "a", "seq": 0, "sender": "client"}]})
    );
    let back: UnansweredRequests = serde_json::from_value(json).unwrap();
    assert_eq!(back, unanswered);
}
