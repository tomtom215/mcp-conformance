// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `BASE-033`, `BASE-037` and `DISC-003`: each side names itself on the wire —
//! `io.modelcontextprotocol/clientInfo` in a request's `_meta`,
//! `io.modelcontextprotocol/serverInfo` in a result's.
//!
//! All three are SHOULDs, so a finding is a warning, never a failure. Until
//! 2026-10-13 they were excluded on the ground that the first two carry an
//! escape hatch — "unless specifically configured not to do so" — and that an
//! absent field is either non-compliance or that configured-off case, which a
//! trace cannot tell apart. That reasoning proves too much: every SHOULD
//! admits "valid reasons in particular circumstances" (RFC 2119), which a
//! trace can never see either, and the registry judges SHOULDs as warnings
//! precisely so the reader can weigh the reason the recording cannot show.
//! The hatch names one such reason; the warning says so, and stays a warning.
//! What the recording *does* show is decidable: the field is on the message
//! or it is not. And it separates implementations — the Go and Python SDK
//! servers put `serverInfo` on every result, rmcp only on `server/discover`
//! and `subscriptions/listen` results (`corpus/draft/captured/`).
//!
//! `DISC-003` restates the rule for one result type, without the hatch. It is
//! its own check because its subject is narrower: a `server/discover` result
//! without `serverInfo` breaks both clauses, any other result only `BASE-037`.

use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::super::FindingSink;
use super::{LEGACY_HANDSHAKE, params_meta};
use crate::context::{Exchange, TraceContext};

/// The request `_meta` key a client names itself with.
const CLIENT_INFO: &str = "io.modelcontextprotocol/clientInfo";
/// The result `_meta` key a server names itself with.
const SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
/// The request `_meta` key that marks a request as this revision's.
const PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// The discovery method `DISC-003` is about.
const DISCOVER: &str = "server/discover";

/// Whether `params` carries the per-request envelope (its protocol version).
///
/// The scope of both "every request" and "every result": the per-request
/// fields belong to requests sent under the per-request model. A legacy
/// `initialize` in a dual-era trace carries `clientInfo` in its own `params`
/// under the previous era's rules, and a request with no envelope at all is
/// `BASE-030`'s finding — judging its missing `clientInfo` too would report
/// one defect twice.
fn modern(params: Option<&Value>) -> bool {
    params
        .and_then(|params| params.get("_meta"))
        .and_then(Value::as_object)
        .is_some_and(|meta| meta.contains_key(PROTOCOL_VERSION))
}

/// Whether a result names its server.
fn names_server(result: &Value) -> bool {
    result
        .get("_meta")
        .and_then(Value::as_object)
        .is_some_and(|meta| meta.contains_key(SERVER_INFO))
}

/// The results answering client requests, as `(exchange, result)`.
fn client_results<'a>(
    context: &'a TraceContext<'_>,
) -> impl Iterator<Item = (Exchange<'a>, &'a Value)> + 'a {
    context.exchanges().filter_map(|exchange| {
        if exchange.request.direction != Direction::ClientToServer
            || exchange.method == LEGACY_HANDSHAKE
        {
            return None;
        }
        Some((exchange, exchange.result?))
    })
}

/// `BASE-033`: a client request carries `io.modelcontextprotocol/clientInfo`.
pub(in crate::checks) fn client_info_on_requests(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, _, _) in context.messages() {
        if event.direction != Direction::ClientToServer {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        let Some(method) = payload.get("method").and_then(Value::as_str) else {
            continue;
        };
        // A notification is not a request; the clause says "every request".
        if payload.get("id").is_none_or(Value::is_null) || !modern(payload.get("params")) {
            continue;
        }
        sink.examined();
        if !params_meta(payload).is_some_and(|meta| meta.contains_key(CLIENT_INFO)) {
            sink.push(
                Some(event.seq),
                format!(
                    "request `{method}` carries no `{CLIENT_INFO}` in its `_meta`; clients \
                     should include it on every request unless specifically configured not to"
                ),
            );
        }
    }
}

/// `BASE-037`: every result answering a request of this revision carries
/// `io.modelcontextprotocol/serverInfo` in its `_meta`.
///
/// An error is not a result, and the clause says "every result's `_meta`".
pub(in crate::checks) fn server_info_on_results(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (exchange, result) in client_results(context) {
        if !modern(exchange.params) {
            continue;
        }
        sink.examined();
        if !names_server(result) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "result answering `{}` carries no `{SERVER_INFO}` in its `_meta`; servers \
                     should include it in every result unless specifically configured not to",
                    exchange.method
                ),
            );
        }
    }
}

/// `DISC-003`: a `server/discover` result carries `serverInfo`.
///
/// Every `server/discover` result, envelope or not: the clause is a field of
/// `DiscoverResult` itself, whatever the request that drew it looked like.
pub(in crate::checks) fn discover_server_info(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (exchange, result) in client_results(context) {
        if exchange.method != DISCOVER {
            continue;
        }
        sink.examined();
        if !names_server(result) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`{DISCOVER}` result carries no `{SERVER_INFO}` in its `_meta`; servers \
                     should include this field"
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
