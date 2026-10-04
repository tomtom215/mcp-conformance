// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the area's shared HTTP framing: which POST carried which body, and
//! which status each response rode — or that the order cannot say.

use super::testkit::{client, post, server, status, trace};
use super::transport::framing::Framing;
use crate::context::TraceContext;

const ACCEPT: &str = r#"{"accept":"application/json, text/event-stream"}"#;

fn request(seq: u64, id: u64) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list"}}"#),
    )
}

fn answer(seq: u64, id: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete"}}}}"#),
    )
}

fn with_framing(lines: &[String], test: impl FnOnce(&Framing<'_>)) {
    let document = trace(lines);
    let events = super::testkit::events(&document);
    let context = TraceContext::new(&events);
    test(&Framing::new(&context));
}

#[test]
fn serial_exchanges_pair_and_attribute_by_order() {
    let lines = [
        post(0, ACCEPT),
        request(1, 1),
        status(2, 200),
        answer(3, "1"),
        post(4, ACCEPT),
        request(5, 2),
        status(6, 400),
        answer(7, "2"),
    ];
    with_framing(&lines, |framing| {
        let paired: Vec<(u64, u64)> = framing
            .posts()
            .iter()
            .map(|post| (post.seq, post.message_seq))
            .collect();
        assert_eq!(paired, [(0, 1), (4, 5)]);
        assert_eq!(framing.status_for(3), Some((2, 200)));
        assert_eq!(framing.status_for(7), Some((6, 400)));
        // A status event is not a message riding itself.
        assert_eq!(framing.status_for(2), None);
    });
}

#[test]
fn overlapping_posts_are_left_unpaired() {
    // The interleaving the capture produces for two concurrent POSTs. Pairing
    // "the next client message" put the second POST's headers on the first
    // body; neither pairing is evidence, so neither is made.
    let lines = [
        post(0, ACCEPT),
        post(1, ACCEPT),
        request(2, 1),
        request(3, 2),
        status(4, 200),
        status(5, 200),
        answer(6, "1"),
        answer(7, "2"),
        // Once the overlap drains, a serial exchange pairs again.
        post(8, ACCEPT),
        request(9, 3),
        status(10, 404),
        answer(11, "3"),
    ];
    with_framing(&lines, |framing| {
        let paired: Vec<u64> = framing.posts().iter().map(|post| post.seq).collect();
        assert_eq!(paired, [8]);
        assert_eq!(framing.status_for(6), None);
        assert_eq!(framing.status_for(7), None);
        assert_eq!(framing.status_for(11), Some((10, 404)));
    });
}

#[test]
fn a_response_rides_its_own_requests_status_not_the_latest() {
    // A long-lived stream opened first, then a second exchange: the second
    // exchange's answer rides its own status even though a later status for
    // neither — here, none — and the first stream's status precede it.
    let lines = [
        post(0, ACCEPT),
        request(1, 1),
        status(2, 200),
        post(3, ACCEPT),
        request(4, 2),
        status(5, 400),
        answer(6, "1"),
        answer(7, "2"),
    ];
    with_framing(&lines, |framing| {
        assert_eq!(framing.status_for(6), Some((2, 200)));
        assert_eq!(framing.status_for(7), Some((5, 400)));
    });
}

#[test]
fn a_null_id_answer_is_tied_only_to_an_unambiguous_status() {
    let lines = [
        post(0, ACCEPT),
        request(1, 4),
        status(2, 400),
        answer(3, "null"),
    ];
    with_framing(&lines, |framing| {
        assert_eq!(framing.status_for(3), Some((2, 400)));
    });
    // With two exchanges awaiting a status, the null-id answer is no one's.
    let overlapping = [
        post(0, ACCEPT),
        request(1, 4),
        post(2, ACCEPT),
        request(3, 5),
        status(4, 400),
        answer(5, "null"),
    ];
    with_framing(&overlapping, |framing| {
        assert_eq!(framing.status_for(5), None);
    });
}

#[test]
fn a_stdio_trace_has_no_framing() {
    let document = [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}"#,
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}}"#,
    ]
    .join("\n");
    let events = super::testkit::events(&document);
    let context = TraceContext::new(&events);
    let framing = Framing::new(&context);
    assert!(framing.posts().is_empty());
    assert_eq!(framing.status_for(1), None);
}
