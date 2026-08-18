// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The Streamable HTTP half of the envelope rules that no layer owns.
//!
//! rmcp's tower layer rejects a POST whose `_meta` lacks a required key, and
//! [`StatelessEnvelope`](crate::server::stateless::StatelessEnvelope) does the
//! same on stdio. Neither covers the other two rules — a log level rmcp cannot
//! decode reads as "no level asked", and a cursor no handler inspects is a
//! cursor nobody rejects — so this middleware is the HTTP side of them.
//!
//! **It is a layer rather than a service wrapper because it has to be.**
//! `StreamableHttpService` takes an `S: ServerHandler`, and rmcp blanket-impls
//! `Service<RoleServer>` for every `ServerHandler`; a wrapper that sees a
//! request before dispatch must implement `Service`, and is therefore not a
//! `ServerHandler` and cannot be handed to that constructor. axum's middleware
//! seam is the one place on this transport that runs before rmcp parses the
//! body, which is exactly where a rejection belongs.
//!
//! It calls [`rules::fault`] — the same function the stdio envelope calls, on
//! the same JSON — so the two transports cannot come to answer the same
//! adversarial request differently. That divergence is the defect this closes;
//! reintroducing it by writing the rule twice would be self-defeating.

use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{Method, Request, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use serde_json::Value;

use crate::server::ServedRevision;
use crate::server::stateless::rules;

/// Largest POST body this will buffer to inspect.
///
/// A body beyond it is forwarded unread rather than rejected: rmcp applies its
/// own limit, and a middleware that invented a second one would answer a size
/// error the transport never agreed to. One mebibyte is far above any
/// conforming MCP request and far below anything worth a denial-of-service
/// concern, since the bytes are dropped the moment the decision is made.
const MAX_INSPECTED: usize = 1024 * 1024;

/// Rejects a POST that breaks a rule no other layer on this transport checks.
///
/// Everything it does not understand is forwarded rather than refused: a GET,
/// a body over [`MAX_INSPECTED`], a body that is not JSON, a batch array. Each
/// of those has an owner downstream, and answering here as well would put two
/// rejections in a race for one request.
pub(super) async fn enforce(
    State(revision): State<ServedRevision>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if !revision.is_stateless() || request.method() != Method::POST {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_INSPECTED).await else {
        // Over the limit or a broken stream: rmcp owns both answers.
        return next.run(Request::from_parts(parts, Body::empty())).await;
    };
    if let Some(rejection) = rejection(&bytes) {
        return rejection;
    }
    next.run(Request::from_parts(parts, Body::from(bytes)))
        .await
}

/// The response refusing `body`, when a rule fires on it.
fn rejection(body: &Bytes) -> Option<Response> {
    let payload: Value = serde_json::from_slice(body).ok()?;
    let fault = rules::fault(&payload)?;
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    // `400 Bad Request`, matching what rmcp answers for the envelope rejections
    // it owns. The specification mandates that status for a *missing required
    // field* and is silent about these two, so the choice is consistency with
    // the neighbouring rejection rather than a claim about the clause.
    Some(
        (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": fault,
            })
            .to_string(),
        )
            .into_response(),
    )
}

#[cfg(test)]
mod tests;
