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
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
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
    assert_ne!(response.status(), StatusCode::FORBIDDEN);
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
