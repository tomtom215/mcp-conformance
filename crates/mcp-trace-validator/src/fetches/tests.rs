// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for setting well-known fetches apart.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::borrow::Cow;

use serde_json::{Value, json};

use super::split;
use crate::reader::{Limits, parse_trace};

fn events(lines: &[Value]) -> Vec<mcp_conformance_core::trace::TraceEvent> {
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    parse_trace(&text.join("\n"), &Limits::default()).expect("test trace parses")
}

fn asked(seq: u64, path: &str) -> Value {
    json!({"seq": seq, "direction": "client-to-server", "transport": "streamable-http",
        "kind": "http", "method": "GET", "path": path})
}

fn answered(seq: u64, path: &str, status: u16, document: Option<Value>) -> Value {
    let mut event = json!({"seq": seq, "direction": "server-to-client",
        "transport": "streamable-http", "kind": "http", "status": status, "path": path});
    if let Some(document) = document {
        event["document"] = document;
    }
    event
}

fn post(seq: u64) -> Value {
    json!({"seq": seq, "direction": "client-to-server", "transport": "streamable-http",
        "kind": "http", "method": "POST", "headers": {"content-type": "application/json"}})
}

#[test]
fn a_trace_without_a_fetch_is_handed_back_as_it_came() {
    let events = events(&[post(0)]);
    let (rest, fetches) = split(&events);
    let fetches = fetches.0;
    assert!(matches!(rest, Cow::Borrowed(_)));
    assert_eq!(rest.len(), 1);
    assert!(fetches.is_empty());
}

#[test]
fn a_fetch_and_its_answer_leave_the_session_and_are_paired() {
    let events = events(&[
        post(0),
        asked(1, "/.well-known/a"),
        post(2),
        answered(3, "/.well-known/a", 200, Some(json!({"k": 1}))),
        asked(4, "/.well-known/b"),
    ]);
    let (rest, fetches) = split(&events);
    let fetches = fetches.0;
    assert_eq!(
        rest.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(fetches.len(), 2);
    let first = fetches[0];
    assert_eq!(first.path, "/.well-known/a");
    let answer = first.answer.expect("answered");
    assert_eq!((answer.seq, answer.status), (3, 200));
    assert_eq!(answer.document, Some(&json!({"k": 1})));
    // Asked and never answered.
    assert_eq!(fetches[1].path, "/.well-known/b");
    assert!(fetches[1].answer.is_none());
}

#[test]
fn an_answer_goes_to_the_earliest_unanswered_request_for_its_path() {
    let events = events(&[
        asked(0, "/.well-known/a"),
        asked(1, "/.well-known/b"),
        asked(2, "/.well-known/a"),
        answered(3, "/.well-known/b", 404, None),
        answered(4, "/.well-known/a", 200, None),
        answered(5, "/.well-known/a", 500, None),
        // No request awaits this one: dropped with the rest.
        answered(6, "/.well-known/a", 204, None),
    ]);
    let (rest, fetches) = split(&events);
    let fetches = fetches.0;
    assert!(rest.is_empty());
    let got: Vec<_> = fetches
        .iter()
        .map(|fetch| {
            (
                fetch.path,
                fetch.answer.map(|answer| (answer.seq, answer.status)),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("/.well-known/a", Some((4, 200))),
            ("/.well-known/b", Some((3, 404))),
            ("/.well-known/a", Some((5, 500)))
        ]
    );
    assert!(
        fetches
            .iter()
            .all(|fetch| fetch.answer.unwrap().document.is_none())
    );
}
