// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! 403 behavior of the HTTP app — the `dns-rebinding-protection` scenario's
//! requirements, exercised in-process via `tower::ServiceExt::oneshot` (no
//! sockets, no network).

#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mcp_everything_server::http::router;
use mcp_everything_server::policy::HttpSecurityPolicy;
use tower::ServiceExt as _;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"http-test","version":"0.0.0"}}}"#;

fn mcp_post(host: Option<&str>, origin: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(host) = host {
        builder = builder.header("host", host);
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    builder.body(Body::from(INITIALIZE)).unwrap()
}

#[tokio::test]
async fn rebinding_host_is_rejected_with_403_before_mcp_processing() {
    let app = router(HttpSecurityPolicy::default());
    let response = app
        .oneshot(mcp_post(Some("evil.example"), None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rebinding_origin_is_rejected_with_403() {
    let app = router(HttpSecurityPolicy::default());
    let response = app
        .oneshot(mcp_post(Some("localhost"), Some("http://evil.example")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn missing_host_is_rejected_fail_closed() {
    let app = router(HttpSecurityPolicy::default());
    let response = app.oneshot(mcp_post(None, None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn loopback_host_reaches_the_mcp_service_and_initializes() {
    let app = router(HttpSecurityPolicy::default());
    let response = app
        .oneshot(mcp_post(
            Some("localhost:8080"),
            Some("http://localhost:6274"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "initialize succeeds");
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains(r#""protocolVersion":"2025-11-25""#),
        "initialize result on the wire: {text}"
    );
    assert!(text.contains("mcp-everything-server"), "{text}");
}

#[tokio::test]
async fn custom_allowlist_replaces_loopback() {
    let app = router(HttpSecurityPolicy::with_allowed_hosts([
        "mcp.internal.example",
    ]));
    let allowed = mcp_post(Some("mcp.internal.example:443"), None);
    let denied = mcp_post(Some("localhost"), None);
    let response = router(HttpSecurityPolicy::with_allowed_hosts([
        "mcp.internal.example",
    ]))
    .oneshot(allowed)
    .await
    .unwrap();
    // "Allowed" means the MCP layer actually processed the initialize — not
    // merely "not 403": a 4xx/5xx from a broken admit path must fail here.
    assert_eq!(response.status(), StatusCode::OK);
    let response = app.oneshot(denied).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn dangerous_opt_out_admits_everything() {
    let app = router(HttpSecurityPolicy::default().dangerously_allow_any_host());
    let response = app
        .oneshot(mcp_post(Some("evil.example"), Some("http://evil.example")))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn unsupported_protocol_version_is_rejected_with_400() {
    // TRAN-020: "If the server receives a request with an invalid or
    // unsupported MCP-Protocol-Version, it MUST respond with 400 Bad
    // Request." Enforced by rmcp's streamable HTTP tower layer for requests
    // within an established session — the initialize exchange itself never
    // consults the header (negotiation is in-band there), measured against
    // rmcp 1.7.0. Pinned here so an rmcp regression is caught in this repo,
    // because the registry's TRAN-020 exclusion names this test as the
    // enforcement.
    let app = router(HttpSecurityPolicy::default());

    let init = app
        .clone()
        .oneshot(mcp_post(Some("localhost:8080"), None))
        .await
        .unwrap();
    assert_eq!(init.status(), StatusCode::OK, "initialize succeeds");
    let session_id = init
        .headers()
        .get("mcp-session-id")
        .expect("stateful server assigns a session id")
        .to_str()
        .unwrap()
        .to_owned();

    let ping = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("host", "localhost:8080")
        .header("mcp-session-id", &session_id)
        .header("mcp-protocol-version", "1999-01-01")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#))
        .unwrap();
    let response = app.oneshot(ping).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("Unsupported MCP-Protocol-Version"),
        "the 400 names the problem: {text}"
    );
}

#[tokio::test]
async fn non_mcp_paths_are_policy_gated_too() {
    // The middleware wraps the whole app: nothing is reachable with a bad
    // Host, including 404 probing.
    let app = router(HttpSecurityPolicy::default());
    let request = Request::builder()
        .method("GET")
        .uri("/anything")
        .header("host", "evil.example")
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn session_ids_are_version_4_uuids_and_distinct() {
    // TRAN-010 (session ids "SHOULD be generated using a secure random
    // number generator") is excluded from trace judgment — one sample
    // proves nothing about entropy — so the source is pinned here instead:
    // rmcp's ids are Uuid::new_v4 (OS RNG). A regression to anything
    // sequential or constant breaks the version/variant nibbles or the
    // distinctness check.
    let app = router(HttpSecurityPolicy::default());
    let mut seen = std::collections::HashSet::new();
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(mcp_post(Some("localhost:8080"), None))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let id = response
            .headers()
            .get("mcp-session-id")
            .expect("session id assigned")
            .to_str()
            .unwrap()
            .to_owned();
        let bytes = id.as_bytes();
        assert_eq!(bytes.len(), 36, "RFC 9562 textual form: {id}");
        for (index, byte) in bytes.iter().enumerate() {
            match index {
                8 | 13 | 18 | 23 => assert_eq!(*byte, b'-', "{id}"),
                _ => assert!(byte.is_ascii_hexdigit(), "{id}"),
            }
        }
        assert_eq!(bytes[14], b'4', "version nibble says v4 (random): {id}");
        assert!(
            matches!(bytes[19], b'8' | b'9' | b'a' | b'b' | b'A' | b'B'),
            "variant nibble is RFC 9562: {id}"
        );
        assert!(seen.insert(id), "session ids must be distinct");
    }
}
