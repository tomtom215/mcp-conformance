// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `2026-07-28` stateless surface, exercised in-process via
//! `tower::ServiceExt::oneshot` (no sockets, no network).
//!
//! What this file pins is the shape a *capture* of this mode will have, which
//! is why the assertions are about bytes on the wire rather than about handler
//! return values: `corpus/draft/captured/` holds a recording of this server,
//! and a unit test on `EverythingServer` would not have caught the transport
//! rejecting the request before the handler ever ran.

#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mcp_everything_server::policy::HttpSecurityPolicy;
use mcp_everything_server::server::ServedRevision;
use serde_json::Value;
use tower::ServiceExt as _;

/// The revision's `_meta` envelope: every ordinary request carries the
/// protocol version and the client's capabilities, because there is no
/// handshake left in which to have stated them once.
const META: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"stateless-test","version":"0.0.0"}}"#;

/// The app under test.
fn app() -> axum::Router {
    mcp_everything_server::http::router(HttpSecurityPolicy::default(), ServedRevision::V2026_07_28)
}

/// A loopback `/mcp` POST of `body`, with the per-request protocol header
/// unless `version` says otherwise, plus any `extra` headers.
fn post(body: String, version: Option<&str>, extra: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    if let Some(version) = version {
        builder = builder.header("mcp-protocol-version", version);
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body)).unwrap()
}

/// A request for `method` with `extra` params beside the `_meta` envelope.
fn request(id: u64, method: &str, extra: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{{META}{extra}}}}}"#)
}

/// Sends `request` and returns its status and the JSON-RPC message in its body.
///
/// SEP-2575 responses come back either as `application/json` or as a
/// one-event SSE stream depending on what the handler did, and the difference
/// is the transport's business rather than the protocol's — so this reads
/// through both framings and the assertions below are about the message.
async fn exchange(request: Request<Body>) -> (StatusCode, Value) {
    let response = app().oneshot(request).await.unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body).into_owned();
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or(&text);
    let parsed = serde_json::from_str(payload)
        .unwrap_or_else(|error| panic!("body is not a JSON-RPC message ({error}): {text}"));
    (status, parsed)
}

/// The SEP-2243 headers that must mirror a request's body at this revision.
///
/// `Mcp-Method` always; `Mcp-Name` when the method names a subject — the tool
/// for `tools/call`, the URI for `resources/read`. A client that omits either
/// is refused by the transport before the handler sees it, which makes these
/// part of what "a conforming exchange" means here rather than test scaffolding.
fn mirrored(method: &str, name: Option<&str>) -> Vec<(String, String)> {
    let mut headers = vec![("mcp-method".to_owned(), method.to_owned())];
    if let Some(name) = name {
        headers.push(("mcp-name".to_owned(), name.to_owned()));
    }
    headers
}

/// The `result` of a successful exchange for `method`.
async fn result_of(method: &str, extra: &str, name: Option<&str>) -> Value {
    let headers = mirrored(method, name);
    let borrowed: Vec<(&str, &str)> = headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();
    let (status, body) = exchange(post(
        request(1, method, extra),
        Some("2026-07-28"),
        &borrowed,
    ))
    .await;
    assert_eq!(status, StatusCode::OK, "{method}: {body}");
    body.get("result")
        .unwrap_or_else(|| panic!("{method} answered no result: {body}"))
        .clone()
}

#[tokio::test]
async fn discovery_advertises_only_the_revision_this_mode_serves() {
    // `server/discover` replaces `initialize`: it is where the capability set
    // and the version list live once there is no handshake to negotiate in.
    let result = result_of("server/discover", "", None).await;
    assert_eq!(
        result.get("supportedVersions"),
        Some(&serde_json::json!(["2026-07-28"])),
        "a mode whose transport rejects legacy requests must not advertise \
         legacy versions: {result}"
    );
    assert_eq!(result.get("resultType"), Some(&Value::from("complete")));
    let capabilities = result.get("capabilities").expect("capabilities");
    for capability in ["tools", "resources", "prompts", "logging", "completions"] {
        assert!(
            capabilities.get(capability).is_some(),
            "{capability} is implemented and must be declared: {capabilities}"
        );
    }
}

#[tokio::test]
async fn every_cacheable_operation_carries_its_hints() {
    // CACH-001's six operations, all of them. `tools/list` and `prompts/list`
    // get their hints from rmcp's handler macros and the rest from this
    // crate's handlers; the requirement does not care which, so neither does
    // this test.
    for (method, extra, name) in [
        ("server/discover", "", None),
        ("tools/list", "", None),
        ("prompts/list", "", None),
        ("resources/list", "", None),
        ("resources/templates/list", "", None),
        (
            "resources/read",
            r#","uri":"test://static-text""#,
            Some("test://static-text"),
        ),
    ] {
        let result = result_of(method, extra, name).await;
        let ttl = result
            .get("ttlMs")
            .unwrap_or_else(|| panic!("{method} carries no ttlMs: {result}"));
        assert!(
            ttl.as_i64().is_some_and(|ttl| ttl >= 0),
            "{method} ttlMs must be a non-negative number, got {ttl}"
        );
        assert!(
            result.get("cacheScope").is_some(),
            "{method} carries no cacheScope: {result}"
        );
        assert_eq!(
            result.get("resultType"),
            Some(&Value::from("complete")),
            "{method} must state its result type at this revision: {result}"
        );
    }
}

#[tokio::test]
async fn a_request_without_the_protocol_header_is_refused() {
    // The header is the transport's half of the per-request envelope. Without
    // the enforcement this mode turns on, an absent header would be read as
    // protocol version 2025-03-26 and the request would be served.
    let (status, body) = exchange(post(
        request(1, "tools/list", ""),
        None,
        &[("mcp-method", "tools/list")],
    ))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.get("error").is_some(), "{body}");
}

#[tokio::test]
async fn a_request_without_the_protocol_meta_is_refused() {
    // And the body's half. `server/discover` is exempt (it is how a client
    // learns which versions exist), so this asks with an ordinary method.
    let naked = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#;
    let (status, body) = exchange(post(
        naked.to_owned(),
        Some("2026-07-28"),
        &[("mcp-method", "tools/list")],
    ))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body.pointer("/error/code"),
        Some(&Value::from(-32602)),
        "{body}"
    );
}

#[tokio::test]
async fn no_session_is_ever_opened() {
    // SEP-2575 removed sessions. A server that still minted `Mcp-Session-Id`
    // would be inviting clients to hold state this revision has no rules for
    // — and the trace tap keys captures on that header, so its absence is
    // what makes the capture a *stateless* one.
    let response = app()
        .oneshot(post(
            request(1, "tools/list", ""),
            Some("2026-07-28"),
            &[("mcp-method", "tools/list")],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("mcp-session-id").is_none(),
        "headers: {:?}",
        response.headers()
    );
}

#[tokio::test]
async fn the_removed_handshake_is_not_served() {
    // `initialize` is exempt from the per-request metadata rules (it predates
    // them), so nothing rejects it at the transport. What must reject it is
    // the version list: this mode supports one revision, and it is not one
    // `initialize` belongs to.
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"legacy","version":"0.0.0"}}}"#;
    let (_, body) = exchange(post(init.to_owned(), None, &[])).await;
    assert_eq!(
        body.pointer("/error/code").and_then(Value::as_i64),
        Some(-32022),
        "the refusal is an UnsupportedProtocolVersionError: {body}"
    );
    assert_eq!(
        body.pointer("/error/data/supported"),
        Some(&serde_json::json!(["2026-07-28"])),
        "VERS-008: the one diagnostic a legacy client can surface must name \
         the versions this server does speak: {body}"
    );
    // Deliberately not asserted: the HTTP status. rmcp answers a legacy-shaped
    // POST — no `MCP-Protocol-Version` header — on its non-negotiated
    // stateless path, which returns 200 unconditionally, so the refusal rides
    // a 200 with the JSON-RPC error in the body. TRAN-074's 400 is stated for
    // the *header* case and this request carries no header; the transport
    // rules for a legacy POST at a modern-only server are the client's
    // fall-back territory (TRAN-107), where the body is what the client is
    // told to inspect.
}

#[tokio::test]
async fn tool_calls_work_without_a_handshake() {
    // The point of the mode, in one exchange: real work, no `initialize`, no
    // session, nothing carried between requests but the `_meta` envelope.
    let result = result_of(
        "tools/call",
        r#","name":"echo","arguments":{"message":"hi"}"#,
        Some("echo"),
    )
    .await;
    let text = result.pointer("/content/0/text").and_then(Value::as_str);
    assert_eq!(text, Some("Echo: hi"), "{result}");
}
