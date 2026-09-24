// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the area's shared helper: which HTTP status a message rode under.

use super::http_status_for;
use super::testkit::{client, post, server, status, trace};
use crate::context::TraceContext;

/// A two-exchange HTTP session: each response's `http` event precedes its
/// message, which is the order `mcp_everything_server::tap` records.
fn two_exchanges() -> Vec<String> {
    vec![
        post(0, r#"{"accept":"application/json, text/event-stream"}"#),
        client(1, r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#),
        status(2, 200),
        server(
            3,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
        status(4, 400),
        server(
            5,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"x"}}"#,
        ),
    ]
}

#[test]
fn a_message_rides_the_status_recorded_before_it() {
    let document = trace(&two_exchanges());
    let events = super::testkit::events(&document);
    let context = TraceContext::new(&events);

    // The whole point of the backwards scan: seq 3's status is the 200 at seq
    // 2, not the 400 at seq 4 that a forwards scan would have found.
    assert_eq!(http_status_for(&context, 3), Some((2, 200)));
    assert_eq!(http_status_for(&context, 5), Some((4, 400)));
    // Strictly before: an event does not ride its own status event.
    assert_eq!(http_status_for(&context, 2), None);
    // Nothing precedes the first event.
    assert_eq!(http_status_for(&context, 0), None);
}

#[test]
fn only_server_sent_statuses_count() {
    // A client `http` event carrying a status is not a response status. The tap
    // never records one, but a hand-built or third-party trace might, and
    // reading it would attribute the client's own framing to the server.
    let mut lines = two_exchanges();
    lines[4] =
        r#"{"seq":4,"direction":"client-to-server","transport":"streamable-http","kind":"http","status":418}"#
            .to_owned();
    let document = trace(&lines);
    let events = super::testkit::events(&document);
    let context = TraceContext::new(&events);

    assert_eq!(
        http_status_for(&context, 5),
        Some((2, 200)),
        "a client-sent status must be skipped, not treated as the response's"
    );
}

#[test]
fn a_stdio_trace_has_no_status_to_read() {
    let document = [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#,
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}}"#,
    ]
    .join("\n");
    let events = super::testkit::events(&document);
    let context = TraceContext::new(&events);
    assert_eq!(http_status_for(&context, 1), None);
}
