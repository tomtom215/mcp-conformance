// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

const REQUEST: &str = "transport.success-content-type";
const GET: &str = "transport.get-content-type";
const ACCEPTED: &str = "transport.accepted-input-status";

/// The findings and subject count `check` reports on `lines`.
fn run(check: &str, lines: &[String]) -> (Vec<String>, u32) {
    let document = lines.join("\n");
    let events: Vec<TraceEvent> = parse_trace(&document, &Limits::default()).unwrap();
    let context = TraceContext::new(&events);
    let outcome = checks::find(check).unwrap().run(&context);
    (
        outcome
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect(),
        outcome.subjects,
    )
}

fn client_http(seq: u64, method: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"{method}","headers":{{"accept":"application/json, text/event-stream"}}}}"#
    )
}

fn status(seq: u64, code: u16, headers: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"http","status":{code},"headers":{headers}}}"#
    )
}

fn client_message(seq: u64, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{payload}}}"#
    )
}

fn server_message(seq: u64, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{payload}}}"#
    )
}

const PING: &str = r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#;
const PONG: &str = r#"{"jsonrpc":"2.0","id":7,"result":{}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

/// A POST carrying `payload`, answered with `code` and `headers`.
fn post(payload: &str, code: u16, headers: &str) -> Vec<String> {
    vec![
        client_http(0, "POST"),
        client_message(1, payload),
        status(2, code, headers),
    ]
}

/// A `POST`ed ping answered with `code` and `headers`, then the response.
fn ping(code: u16, headers: &str) -> Vec<String> {
    let mut lines = post(PING, code, headers);
    lines.push(server_message(3, PONG));
    lines
}

#[test]
fn a_request_must_be_answered_with_json_or_an_event_stream() {
    let (findings, subjects) = run(REQUEST, &ping(200, r#"{"content-type":"text/html"}"#));
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("text/html"), "{findings:?}");

    let (findings, _) = run(REQUEST, &ping(200, "{}"));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("no Content-Type"), "{findings:?}");

    for ok in [
        r#"{"content-type":"text/event-stream"}"#,
        r#"{"content-type":"application/json; charset=utf-8"}"#,
        r#"{"content-type":"Application/JSON ; charset=UTF-8"}"#,
    ] {
        let (findings, subjects) = run(REQUEST, &ping(200, ok));
        assert!(findings.is_empty(), "{ok}: {findings:?}");
        assert_eq!(subjects, 1, "{ok}");
    }
}

#[test]
fn the_media_type_is_compared_exactly() {
    for bad in [
        r#"{"content-type":"application/json-seq"}"#,
        r#"{"content-type":"text/plain, application/json"}"#,
    ] {
        let (findings, _) = run(REQUEST, &ping(200, bad));
        assert_eq!(findings.len(), 1, "{bad}: {findings:?}");
    }
}

#[test]
fn a_202_does_not_answer_a_request() {
    // A request needs a response; `202 Accepted` has no body to carry one.
    let (findings, subjects) = run(REQUEST, &post(PING, 202, "{}"));
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("HTTP 202"), "{findings:?}");
    // An error status is the refusal other clauses govern.
    let (findings, subjects) = run(REQUEST, &post(PING, 400, "{}"));
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(subjects, 0);
}

#[test]
fn a_session_teardown_is_not_judged() {
    // The official TypeScript SDK answers DELETE with `200` and no body or
    // Content-Type; neither content-type clause reaches it.
    let teardown = vec![client_http(0, "DELETE"), status(1, 200, "{}")];
    for check in [REQUEST, GET, ACCEPTED] {
        let (findings, subjects) = run(check, &teardown);
        assert!(findings.is_empty(), "{check}: {findings:?}");
        assert_eq!(subjects, 0, "{check}");
    }
}

#[test]
fn a_get_must_be_answered_with_an_event_stream() {
    let get = |code: u16, headers: &str| vec![client_http(0, "GET"), status(1, code, headers)];
    let (findings, subjects) = run(GET, &get(200, r#"{"content-type":"application/json"}"#));
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("GET"), "{findings:?}");
    // The request clause is about POSTed requests, not this.
    let (findings, subjects) = run(REQUEST, &get(200, r#"{"content-type":"application/json"}"#));
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(subjects, 0);

    let (findings, _) = run(GET, &get(200, r#"{"content-type":"text/event-stream"}"#));
    assert!(findings.is_empty(), "{findings:?}");
    // 405 is the permitted refusal; it is not a success to judge.
    let (findings, subjects) = run(GET, &get(405, "{}"));
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(subjects, 0);
}

#[test]
fn accepted_notifications_and_responses_are_answered_202() {
    let (findings, subjects) = run(ACCEPTED, &post(INITIALIZED, 202, "{}"));
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(subjects, 1);

    let (findings, _) = run(ACCEPTED, &post(INITIALIZED, 200, "{}"));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("notification"), "{findings:?}");

    let response = r#"{"jsonrpc":"2.0","id":"s1","result":{}}"#;
    let (findings, _) = run(ACCEPTED, &post(response, 204, "{}"));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("response"), "{findings:?}");

    // Refusing the input is the neighbouring clause's business.
    let (findings, subjects) = run(ACCEPTED, &post(INITIALIZED, 400, "{}"));
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(subjects, 0);
    // A request is not this clause's subject.
    let (_, subjects) = run(
        ACCEPTED,
        &ping(200, r#"{"content-type":"application/json"}"#),
    );
    assert_eq!(subjects, 0);
}

#[test]
fn overlapping_exchanges_are_left_unjudged() {
    // Two POSTs in flight, then two statuses: which status answered which
    // POST is not in the recording, so neither is judged — even though one
    // of them would be a violation for either reading.
    let lines = vec![
        client_http(0, "POST"),
        client_message(1, PING),
        client_http(2, "POST"),
        client_message(3, INITIALIZED),
        status(4, 200, r#"{"content-type":"application/json"}"#),
        server_message(5, PONG),
        status(6, 200, "{}"),
    ];
    for check in [REQUEST, ACCEPTED] {
        let (findings, subjects) = run(check, &lines);
        assert!(findings.is_empty(), "{check}: {findings:?}");
        assert_eq!(subjects, 0, "{check}");
    }
}

#[test]
fn response_framing_alone_judges_a_status_a_response_follows() {
    // A recording with no client `http` events: a status followed by a
    // JSON-RPC response framed a request's answer.
    let lines = vec![
        client_message(0, PING),
        status(1, 200, r#"{"content-type":"text/html"}"#),
        server_message(2, PONG),
    ];
    let (findings, subjects) = run(REQUEST, &lines);
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    // A lone 202 with nothing after it says nothing about what it answered.
    let (_, subjects) = run(REQUEST, &[status(0, 202, "{}")]);
    assert_eq!(subjects, 0);
}
