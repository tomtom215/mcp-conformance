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
        assert_eq!(framing.unidentified_answers().collect::<Vec<_>>(), [(3, 1)]);
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
        assert_eq!(framing.unidentified_answers().count(), 0);
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

#[test]
fn an_abort_ends_one_exchange_so_the_next_is_attributable() {
    // The proxy records an upstream failure in place of the status the first
    // POST never got. That ends it, so the second POST, alone in flight, owns
    // the next status. Ignoring the abort would leave two exchanges awaiting
    // one status, and nothing attributed.
    let abort = r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"lifecycle","event":"transport-abort"}"#;
    let lines = [
        post(0, ACCEPT),
        request(1, 1),
        abort.to_owned(),
        post(3, ACCEPT),
        request(4, 2),
        status(5, 200),
        answer(6, "2"),
    ];
    with_framing(&lines, |framing| {
        let exchanges = framing.exchanges();
        assert_eq!(exchanges.len(), 1, "{exchanges:?}");
        assert_eq!(exchanges[0].request.map(|(seq, ..)| seq), Some(3));
        assert_eq!(exchanges[0].status, 200);
    });
}

#[test]
fn a_body_pairs_with_the_post_awaiting_one_not_a_get() {
    // A GET carries no body, so a client message arriving while one is open
    // belongs to the POST.
    let get = r#"{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"GET","headers":{}}"#;
    let lines = [post(0, ACCEPT), get.to_owned(), request(2, 1)];
    with_framing(&lines, |framing| {
        let posts = framing.posts();
        assert_eq!(posts.len(), 1);
        assert_eq!((posts[0].seq, posts[0].message_seq), (0, 2));
    });
}

#[test]
fn only_a_result_or_error_counts_as_a_framed_response() {
    // An object with neither `method` nor `result`/`error` is not a response,
    // so it does not make a status no exchange awaited into a request's answer.
    let lines = [status(0, 200), server(1, r#"{"jsonrpc":"2.0","id":5}"#)];
    with_framing(&lines, |framing| {
        let exchanges = framing.exchanges();
        assert_eq!(exchanges.len(), 1);
        assert!(!exchanges[0].framed_response);
    });
    let answered = [status(0, 200), answer(1, "5")];
    with_framing(&answered, |framing| {
        assert!(framing.exchanges()[0].framed_response);
    });
}
