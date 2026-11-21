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
async fn a_cursor_the_server_issued_passes_the_layer() {
    // The other half of the rule: the layer refuses what the server never
    // issued, so it must admit what it did, or pagination would be offered by
    // the handler and refused by the transport in front of it.
    let issued = mcp_everything_server::pagination::Listing::Tools
        .cursor(1)
        .unwrap();
    assert_eq!(
        status_of(
            ServedRevision::V2026_07_28,
            post(listing_with_cursor(&issued))
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_layer_is_inert_on_the_revision_that_has_no_envelope() {
    // `2025-11-25` has no `_meta` envelope and its own pagination story, so a
    // request carrying a cursor there is not this layer's business.
    //
    // The request names `2026-07-28`, which the legacy mode does not serve
    // and does not claim to, so the SDK refuses it for the revision
    // (`-32022`, with the revisions it does serve). That is how the refusal
    // is told from this layer's, which is `-32602` and about the cursor.
    let response = router(ServedRevision::V2025_11_25)
        .oneshot(post(listing_with_cursor("fabricated")))
        .await
        .expect("the router answers");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["error"]["code"], -32022, "{body}");
    assert_eq!(body["error"]["data"]["requested"], "2026-07-28", "{body}");
    assert!(
        !body.to_string().contains("cursor"),
        "the envelope layer must not fire at 2025-11-25: {body}"
    );
}
