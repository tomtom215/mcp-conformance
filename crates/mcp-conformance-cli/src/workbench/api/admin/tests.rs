// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The limits of a change request's body.

#![allow(clippy::unwrap_used)]

use axum::http::{Method, Request};

use super::super::tests::{send, shared};
use super::*;

/// A change request whose body is `body`, as the page sends it.
fn change(shared: &Shared, path: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method(Method::POST)
        .uri(path)
        .header(header::HOST, "127.0.0.1:4200")
        .header(header::COOKIE, shared.keys.cookie())
        .header(header::ORIGIN, "http://127.0.0.1:4200")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn a_body_of_many_megabytes_is_read_whole() {
    let (shared, _dir) = shared("body-limit");
    // 8 MiB of JSON whitespace after a small request: far past any
    // kilobyte- or single-megabyte-scale limit, well within a recording at
    // the message limit.
    let mut body = br#"{"days": 100000}"#.to_vec();
    body.resize(body.len() + 8 * 1024 * 1024, b' ');
    let (status, _, answer) = send(&shared, change(&shared, "/api/retention", body)).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&answer)
    );
    let answer: Value = serde_json::from_slice(&answer).unwrap();
    assert_eq!(answer["entries"], json!([]));
}
