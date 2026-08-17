// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The two envelope rules no transport layer enforces for us, as one
//! implementation over the request's JSON.
//!
//! Everything else about the `2026-07-28` envelope has an owner already:
//! rmcp's tower layer checks the required `_meta` keys and the protocol
//! version on Streamable HTTP, and [`super::StatelessEnvelope`] does the same
//! on stdio using rmcp's own constants. These two have no owner anywhere — a
//! level rmcp cannot decode reads as "no level asked", and a cursor no handler
//! inspects is a cursor nobody rejects — so this server owes them on *both*
//! transports.
//!
//! **Written against JSON rather than the typed request, so there is exactly
//! one of each rule.** The two call sites see different things: stdio holds a
//! decoded `ClientRequest`, and the HTTP layer holds the bytes of a POST body
//! it must inspect before rmcp parses them. Extracting the same field twice,
//! in two shapes, is how the two transports come to answer the same
//! adversarial request differently — which is the defect these rules exist to
//! fix, so reintroducing it here would be absurd. The stdio side pays one
//! `to_value` per request for that guarantee, which a reference server can
//! afford.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items a private module exports upward; this follows
// the rustc lint, as `logging` and `server` already do in this crate.
#![allow(clippy::redundant_pub_crate)]

use rmcp::ErrorData as McpError;
use rmcp::model::LoggingLevel;
use serde_json::Value;

#[cfg(test)]
mod tests;

/// The `_meta` key a request asks for log messages with.
pub(crate) const LOG_LEVEL: &str = "io.modelcontextprotocol/logLevel";

/// The list operations that accept a `cursor`.
const PAGINATED: &[&str] = &[
    "tools/list",
    "prompts/list",
    "resources/list",
    "resources/templates/list",
];

/// The fault in one request's JSON, if any.
///
/// `payload` is the whole JSON-RPC request object. Anything it cannot read —
/// a body that is not an object, params that are not an object, a method that
/// is not a string — is *not* this function's finding: those are malformed at
/// a level the transport rejects first, and inventing an answer for them here
/// would put two rejections in a race.
#[must_use]
pub(crate) fn fault(payload: &Value) -> Option<McpError> {
    let method = payload.get("method")?.as_str()?;
    let params = payload.get("params");
    let meta = params.and_then(|params| params.get("_meta"));
    if let Some(fault) = log_level_fault(meta.and_then(|meta| meta.get(LOG_LEVEL))) {
        return Some(fault);
    }
    cursor_fault(method, params.and_then(|params| params.get("cursor")))
}

/// `LOG-010`: a level outside RFC 5424's eight draws `-32602`.
///
/// Takes the raw value rather than rmcp's decoded `LoggingLevel`, which
/// answers `None` for *both* "absent" and "not a level". Those are opposite
/// cases: the first asks for no logs — which the revision requires a server to
/// honour by staying silent — and the second is the malformed request this
/// clause exists to reject. A server that cannot tell them apart serves the
/// second, which is exactly what this one did until a probe session asked.
fn log_level_fault(asked: Option<&Value>) -> Option<McpError> {
    let asked = asked?;
    if serde_json::from_value::<LoggingLevel>(asked.clone()).is_ok() {
        return None;
    }
    Some(McpError::invalid_params(
        format!("`{LOG_LEVEL}` is {asked}, which is not one of the eight RFC 5424 levels"),
        Some(serde_json::json!({ "field": LOG_LEVEL })),
    ))
}

/// `PAGE-011`: a cursor this server never issued draws `-32602`.
///
/// This server issues none: every catalogue it serves fits in one page, so no
/// result of its own has ever carried a `nextCursor`. That makes *any* cursor
/// presented to it one it did not issue — fabricated, modified, or carried
/// over from another server — which is what the clause forbids honouring. A
/// server that paginated would compare against what it had issued; this one
/// can answer from the stronger fact that it issued nothing.
///
/// Scoped to the four list operations because they are the only requests a
/// `cursor` means anything on. A `cursor` member on a `tools/call` is an
/// argument named "cursor", not a pagination token, and rejecting it would be
/// this server inventing a rule.
fn cursor_fault(method: &str, cursor: Option<&Value>) -> Option<McpError> {
    if !PAGINATED.contains(&method) {
        return None;
    }
    let cursor = cursor?;
    Some(McpError::invalid_params(
        format!("cursor {cursor} was not issued by this server, which paginates nothing"),
        Some(serde_json::json!({ "cursor": cursor })),
    ))
}
