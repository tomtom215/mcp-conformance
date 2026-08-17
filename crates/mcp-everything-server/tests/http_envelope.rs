// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The envelope layer as the router actually mounts it, in-process via
//! `tower::ServiceExt::oneshot`.
//!
//! `http::envelope`'s unit tests pin the decision — which bodies draw a
//! rejection and which are forwarded. What they cannot reach is the *guard*:
//! whether the layer is inert at `2025-11-25`, whether it leaves a GET alone,
//! and whether a body large enough to matter is still inspected. Each of those
//! is a property of how the middleware is wired rather than of what it
//! decides, so each needs the wiring.
//!
//! It is the same seam the DNS-rebinding tests use, for the same reason: no
//! sockets, no ports, no flakes.

#![cfg(feature = "http")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::body::Body;
use axum::http::{Request, StatusCode};
use mcp_everything_server::policy::HttpSecurityPolicy;
use mcp_everything_server::server::ServedRevision;
use tower::ServiceExt as _;

/// The `_meta` envelope a `2026-07-28` request carries.
const ENVELOPE: &str = r#""io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}"#;

fn router(revision: ServedRevision) -> axum::Router {
    mcp_everything_server::http::router(
        HttpSecurityPolicy::with_allowed_hosts(vec!["localhost".to_owned()]),
        revision,
    )
}

/// A POST of `body` to the MCP endpoint, with the headers the layer needs to
/// see past.
fn post(body: String) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .header("mcp-method", "tools/list")
        .body(Body::from(body))
        .unwrap()
}

/// A `tools/list` presenting `cursor` — the request the layer must refuse.
fn listing_with_cursor(cursor: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"_meta":{{{ENVELOPE}}},"cursor":"{cursor}"}}}}"#
    )
}

async fn status_of(revision: ServedRevision, request: Request<Body>) -> StatusCode {
    router(revision)
        .oneshot(request)
        .await
        .expect("the router answers")
        .status()
}

#[tokio::test]
async fn a_fabricated_cursor_is_refused_on_the_stateless_revision() {
    assert_eq!(
        status_of(
            ServedRevision::V2026_07_28,
            post(listing_with_cursor("fabricated"))
        )
        .await,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn the_layer_is_inert_on_the_revision_that_has_no_envelope() {
    // `2025-11-25` has no `_meta` envelope and its own pagination story, so a
    // request carrying a cursor there is not this layer's business. Answering
    // it anyway would be the stateless revision's rules leaking backwards onto
    // the surface every committed baseline pins.
    let answered = status_of(
        ServedRevision::V2025_11_25,
        post(listing_with_cursor("fabricated")),
    )
    .await;
    assert_ne!(
        answered,
        StatusCode::BAD_REQUEST,
        "the envelope layer must not fire at 2025-11-25"
    );
}

#[tokio::test]
async fn a_get_is_never_inspected() {
    // Only a POST carries a JSON-RPC request; a GET opens a stream, and
    // buffering its body to look for a cursor would be reading something that
    // is not there — and, worse, would consume the stream.
    let get = Request::builder()
        .method("GET")
        .uri("/mcp")
        .header("host", "localhost")
        .header("accept", "text/event-stream")
        .header("mcp-protocol-version", "2026-07-28")
        .body(Body::empty())
        .unwrap();
    let answered = status_of(ServedRevision::V2026_07_28, get).await;
    assert_ne!(
        answered,
        StatusCode::BAD_REQUEST,
        "a GET must reach the transport untouched"
    );
}

#[tokio::test]
async fn a_body_far_larger_than_a_request_is_still_inspected() {
    // The buffering limit has to be large enough that no conforming request
    // reaches it, or the layer silently stops enforcing on exactly the traffic
    // an adversary controls the size of. A cursor padded well past any
    // plausible small bound must still be refused.
    let padded = "x".repeat(64 * 1024);
    assert_eq!(
        status_of(
            ServedRevision::V2026_07_28,
            post(listing_with_cursor(&padded))
        )
        .await,
        StatusCode::BAD_REQUEST,
        "a 64 KiB body is far below the limit and must still be read"
    );
}

#[tokio::test]
async fn a_conforming_request_reaches_the_server() {
    // The case that matters most: every ordinary request passes through this
    // layer, so a false rejection breaks the whole transport. `tools/list`
    // without a cursor is the plainest conforming request there is.
    let clean = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"_meta":{{{ENVELOPE}}}}}}}"#
    );
    assert_eq!(
        status_of(ServedRevision::V2026_07_28, post(clean)).await,
        StatusCode::OK
    );
}
