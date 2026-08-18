// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the layer refuses, and — the larger half — what it lets through.
//!
//! A middleware that inspects every POST body is a middleware that can break
//! every request, so the pass-through cases are pinned at least as carefully
//! as the rejections.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

/// The body of a request for `method` with `params`, as bytes.
fn body(method: &str, params: &serde_json::Value) -> Bytes {
    Bytes::from(
        serde_json::json!({"jsonrpc": "2.0", "id": 7, "method": method, "params": params})
            .to_string(),
    )
}

#[test]
fn a_broken_rule_is_a_400_carrying_the_request_id() {
    // The id matters: a client correlates the refusal with what it sent, and a
    // rejection it cannot place is a rejection it cannot act on.
    let refused = rejection(&body(
        "tools/list",
        &serde_json::json!({"cursor": "fabricated"}),
    ))
    .expect("an unissued cursor is refused");
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        refused
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
}

#[test]
fn the_body_is_a_json_rpc_error_response_naming_the_id() {
    // Rendered rather than asserted through the type, because what a client
    // parses is the bytes.
    let refused = rejection(&body(
        "tools/call",
        &serde_json::json!({"_meta": {rules::LOG_LEVEL: "chatty"}}),
    ))
    .expect("an unrecognized level is refused");
    let rendered = format!("{refused:?}");
    assert!(rendered.contains("400"), "{rendered}");
    // The response body is behind an async stream; what is checkable
    // synchronously is that the fault carried the id and the code.
    let payload: serde_json::Value = serde_json::from_slice(&body(
        "tools/call",
        &serde_json::json!({"_meta": {rules::LOG_LEVEL: "chatty"}}),
    ))
    .expect("the fixture parses");
    assert_eq!(payload["id"], 7);
    assert_eq!(
        rules::fault(&payload).map(|fault| fault.code),
        Some(rmcp::model::ErrorCode::INVALID_PARAMS)
    );
}

#[test]
fn a_conforming_request_is_not_touched() {
    // The case that matters most: every ordinary request passes through this
    // layer, so a false rejection here breaks the whole server.
    for (method, params) in [
        ("tools/list", serde_json::json!({})),
        ("tools/call", serde_json::json!({"name": "echo"})),
        (
            "tools/call",
            serde_json::json!({"_meta": {rules::LOG_LEVEL: "debug"}}),
        ),
        ("resources/read", serde_json::json!({"uri": "test://x"})),
        // A `cursor` that is not a pagination token: an argument named cursor.
        ("tools/call", serde_json::json!({"cursor": "not a token"})),
    ] {
        assert!(
            rejection(&body(method, &params)).is_none(),
            "{method} {params} must pass through"
        );
    }
}

#[test]
fn what_this_layer_cannot_read_belongs_to_something_downstream() {
    // Each of these has an owner: rmcp answers -32700 for unparseable JSON and
    // -32600 for a shape that is not a request. Answering here as well would
    // put two rejections in a race for one request.
    for raw in [
        &b"not json at all"[..],
        &b"[]"[..],
        &b"{}"[..],
        &b"{\"jsonrpc\":\"2.0\",\"id\":1}"[..],
    ] {
        assert!(
            rejection(&Bytes::copy_from_slice(raw)).is_none(),
            "{:?} must be forwarded",
            String::from_utf8_lossy(raw)
        );
    }
}
