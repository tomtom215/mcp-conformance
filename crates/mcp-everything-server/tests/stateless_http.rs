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
    // CACH-001's six operations, all of them, each built by this crate's
    // handlers (the list operations a page at a time).
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

#[tokio::test]
async fn every_listing_paginates_over_the_wire_under_one_cache_scope() {
    // The cursor flow end to end: the first page names a `nextCursor`, the
    // request presenting it is admitted by the envelope layer and answered
    // with the last page, and both pages carry the same `cacheScope`
    // (CACH-015: "Servers MUST apply the same `cacheScope` to all response
    // pages for a given list request").
    for (method, second) in [
        ("tools/list", "test_parameter_headers"),
        ("prompts/list", "test_prompt_with_audio"),
        ("resources/list", "test://annotated-text"),
        ("resources/templates/list", "test://annotated/{id}"),
    ] {
        let first = result_of(method, "", None).await;
        let cursor = first
            .get("nextCursor")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("{method}'s first page names no next page: {first}"));
        let last = result_of(method, &format!(r#","cursor":"{cursor}""#), None).await;
        assert!(last.get("nextCursor").is_none(), "{method}: {last}");
        assert_eq!(first.get("cacheScope"), last.get("cacheScope"), "{method}");
        assert!(
            last.to_string().contains(second),
            "{method}'s second page carries {second}: {last}"
        );
        assert!(
            !first.to_string().contains(second),
            "{method}'s first page stays the suite's catalogue: {first}"
        );
    }
}

#[tokio::test]
async fn an_unissued_cursor_is_invalid_params() {
    // PAGE-011's SHOULD: "Invalid cursors SHOULD result in an error with code
    // -32602 (Invalid params)."
    let (status, body) = exchange(post(
        request(1, "resources/list", r#","cursor":"resources/list#page-9""#),
        Some("2026-07-28"),
        &[("mcp-method", "resources/list")],
    ))
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body.pointer("/error/code"), Some(&Value::from(-32602)));
}

/// A call of `test_parameter_headers` with `arguments`, carrying `headers`
/// beside the ones every `tools/call` mirrors.
async fn call_with_headers(arguments: &str, headers: &[(&str, &str)]) -> (StatusCode, Value) {
    let mut all = vec![
        ("mcp-method", "tools/call"),
        ("mcp-name", "test_parameter_headers"),
    ];
    all.extend_from_slice(headers);
    exchange(post(
        request(
            1,
            "tools/call",
            &format!(r#","name":"test_parameter_headers","arguments":{arguments}"#),
        ),
        Some("2026-07-28"),
        &all,
    ))
    .await
}

#[tokio::test]
async fn designated_parameters_are_checked_against_their_headers() {
    // The `x-mcp-header` table, from the server's side. A value provided and
    // mirrored is served; an argument left out needs no header (TRAN-135:
    // "Server MUST NOT expect the header").
    let (status, body) = call_with_headers(
        r#"{"region":"us-west1","limit":7}"#,
        &[("mcp-param-region", "us-west1"), ("mcp-param-limit", "7")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body.pointer("/result/content/0/text"),
        Some(&Value::from("region=us-west1 limit=7 zone=(not supplied)"))
    );
    // A designated value sent without its header is a non-conforming client,
    // and TRAN-136 says the server "MUST reject the request": rmcp's
    // transport does, with HeaderMismatch under HTTP 400.
    let (status, body) = call_with_headers(
        r#"{"region":"us-west1","limit":7}"#,
        &[("mcp-param-limit", "7")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body.pointer("/error/code"), Some(&Value::from(-32020)));
    // So is a header that disagrees with the body.
    let (status, body) = call_with_headers(
        r#"{"region":"us-west1","limit":7}"#,
        &[("mcp-param-region", "us-east1"), ("mcp-param-limit", "7")],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body.pointer("/error/code"), Some(&Value::from(-32020)));
}

/// The server's name as a result's `_meta` carries it, if it does.
fn named_server(result: &Value) -> Option<&str> {
    result
        .pointer("/_meta/io.modelcontextprotocol~1serverInfo/name")
        .and_then(Value::as_str)
}

#[tokio::test]
async fn every_result_names_the_server_over_the_wire() {
    // BASE-037 on the transport the tap records: the field is put on in the
    // handler, and this is where a framing layer that rebuilt the result
    // would be caught dropping it. The MRTR arm and `subscriptions/listen`
    // are walked over stdio (`tests/stateless_stdio.rs`), which needs no
    // stream reader for the long-lived request.
    for (method, extra, name) in [
        ("server/discover", "", None),
        ("tools/list", "", None),
        (
            "tools/call",
            r#","name":"echo","arguments":{"message":"hi"}"#,
            Some("echo"),
        ),
        ("prompts/list", "", None),
        (
            "prompts/get",
            r#","name":"test_simple_prompt""#,
            Some("test_simple_prompt"),
        ),
        ("resources/list", "", None),
        ("resources/templates/list", "", None),
        (
            "resources/read",
            r#","uri":"test://static-text""#,
            Some("test://static-text"),
        ),
        (
            "completion/complete",
            r#","ref":{"type":"ref/prompt","name":"test_prompt_with_arguments"},"argument":{"name":"arg1","value":"pa"}"#,
            None,
        ),
    ] {
        let result = result_of(method, extra, name).await;
        assert_eq!(
            named_server(&result),
            Some("mcp-everything-server"),
            "{method}: {result}"
        );
    }
}

#[tokio::test]
async fn the_older_revision_still_names_itself_only_where_rmcp_does() {
    // `2025-11-25` defines no `serverInfo` in a result's `_meta`, and the
    // legacy server answering a `2026-07-28` request — what `draft-readiness`
    // drives every run — is that server held to a revision it does not
    // implement. Its answer is unchanged: the field stays off `tools/list`.
    let legacy = mcp_everything_server::http::router(
        HttpSecurityPolicy::default(),
        ServedRevision::V2025_11_25,
    );
    let response = legacy
        .oneshot(post(
            request(1, "tools/list", ""),
            Some("2026-07-28"),
            &[("mcp-method", "tools/list")],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let text = String::from_utf8_lossy(&body).into_owned();
    let payload = text
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .unwrap_or(&text);
    let answer: Value = serde_json::from_str(payload).unwrap();
    let result = answer.get("result").unwrap_or_else(|| panic!("{answer}"));
    assert_eq!(named_server(result), None, "{result}");
}
