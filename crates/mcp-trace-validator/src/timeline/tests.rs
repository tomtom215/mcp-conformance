// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The reading of messages: their shape, their pairing, and how
//! much of a payload is shown.

#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::{PAYLOAD_PREVIEW_BYTES, Shape, pair, preview, shape};
use crate::reader::{Limits, parse_trace};

#[test]
fn messages_are_read_by_their_members() {
    let id = || "1".to_owned();
    assert_eq!(
        shape(&json!({"id":1,"method":"ping"})),
        Shape::Request {
            method: "ping".into(),
            id: id()
        }
    );
    assert_eq!(
        shape(&json!({"id":1,"method":7})),
        Shape::Request {
            method: "7".into(),
            id: id()
        }
    );
    assert_eq!(
        shape(&json!({"method":"n"})),
        Shape::Notification { method: "n".into() }
    );
    assert_eq!(
        shape(&json!({"id":1,"result":{}})),
        Shape::Result { id: id() }
    );
    assert_eq!(
        shape(&json!({"id":1,"error":{"code":-32601,"message":"x"}})),
        Shape::Error {
            id: id(),
            error: "error -32601".into()
        }
    );
    assert_eq!(
        shape(&json!({"id":1,"error":{}})),
        Shape::Error {
            id: id(),
            error: "error".into()
        }
    );
    assert_eq!(shape(&json!({"id":1})), Shape::Other);
    assert_eq!(shape(&json!({})), Shape::Other);
    assert_eq!(shape(&json!([{}, {}])), Shape::Batch(2));
    assert_eq!(shape(&json!("x")), Shape::Other);
}

#[test]
fn a_response_pairs_with_the_latest_open_request_sent_the_other_way() {
    let events = parse_trace(
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"a"}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"b"}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}
{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":1,"message":"m"}}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}
"#,
        &Limits::default(),
    )
    .unwrap();
    let pairs = pair(&events);
    // The client's answer to the server's request `b`, then the server's to `a`.
    assert_eq!(pairs.answers.get(&2), Some(&(1, "b".to_owned())));
    assert_eq!(pairs.answers.get(&3), Some(&(0, "a".to_owned())));
    assert_eq!(pairs.answered_by.get(&0), Some(&3));
    assert_eq!(pairs.answered_by.get(&1), Some(&2));
    // A second answer to `a` answers nothing.
    assert_eq!(pairs.answers.get(&4), None);
    assert_eq!(pairs.answers.len(), 2);
}

#[test]
fn a_large_payload_is_cut_at_a_character_boundary_and_counted() {
    let small = "x".repeat(PAYLOAD_PREVIEW_BYTES);
    assert_eq!(preview(&small), (small.as_str(), 0));
    let large = format!(
        "{}é{}",
        "x".repeat(PAYLOAD_PREVIEW_BYTES - 1),
        "y".repeat(10)
    );
    let (shown, rest) = preview(&large);
    assert_eq!(
        shown.len(),
        PAYLOAD_PREVIEW_BYTES - 1,
        "é straddles the limit, so it is left out"
    );
    assert_eq!(shown.len() + rest, large.len());
}
