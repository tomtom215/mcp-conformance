// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `TRAN-020`: "If the server receives a request with an invalid or
//! unsupported `MCP-Protocol-Version`, it MUST respond with `400 Bad Request`."
//!
//! Excluded until 2026-10-13, on the ground that the server's supported-version
//! set is not in the trace. In general it is not — but the trace does carry one
//! piece of it, stated by the server itself. Version negotiation binds the
//! server: "If the server supports the requested protocol version, it MUST
//! respond with the same version" (`basic/lifecycle#version-negotiation`). So
//! an `initialize` result naming a version *other* than the one the client
//! requested is the server saying it does not support the requested one. A
//! later request carrying that refused version in `MCP-Protocol-Version` is a
//! request with an unsupported header by the server's own account, and must be
//! answered `400`.
//!
//! That is the whole of what is judged; the rest stays out of reach:
//!
//! - a header naming any *other* version the trace never discussed — the server
//!   may support it, and nothing recorded says otherwise;
//! - an *invalid* header — no in-scope page of this revision states the
//!   version format, so "invalid" has no definition a check could apply;
//! - the `initialize` POST itself, whose version is negotiated in its body.
//!
//! A server that answered `initialize` with a different version while in fact
//! supporting the requested one broke the negotiation clause first; the
//! finding here then rests on that server's own false statement, which is the
//! one premise this check takes on trust.
//!
//! Judged on exchanges the framing attributes ([`Framing`]): a request POST
//! whose status is tied to it, and whose carried message is a JSON-RPC
//! request. "Receives a request" is a request; a notification POST is left to
//! the neighbouring status clauses.
//!
//! [`Framing`]: super::super::stateless::transport::framing::Framing

use serde_json::Value;

use super::super::FindingSink;
use super::super::stateless::transport::framing::Framing;
use crate::context::TraceContext;

/// The HTTP status the clause requires.
const BAD_REQUEST: u16 = 400;

/// The version the client requested and the server declined, with the `seq` of
/// the result that declined it.
fn refused_version<'a>(context: &TraceContext<'a>) -> Option<(u64, &'a str)> {
    let exchange = context.initialize();
    let (_, params) = exchange.request?;
    let requested = params?.get("protocolVersion")?.as_str()?;
    let (seq, result) = exchange.result?;
    let negotiated = result.get("protocolVersion")?.as_str()?;
    (requested != negotiated).then_some((seq, requested))
}

/// `TRAN-020`: a request bearing the version the server declined at
/// `initialize` is answered `400 Bad Request`.
pub(in crate::checks) fn unsupported_version_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let Some((declined_at, refused)) = refused_version(context) else {
        return;
    };
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        let Some((seq, _, headers)) = exchange.request else {
            continue;
        };
        if seq <= declined_at || !exchange.carried_request() {
            continue;
        }
        if exchange
            .carried
            .and_then(|payload| payload.get("method"))
            .and_then(Value::as_str)
            == Some("initialize")
        {
            continue;
        }
        if headers.get("mcp-protocol-version").map(String::as_str) != Some(refused) {
            continue;
        }
        sink.examined();
        if exchange.status != BAD_REQUEST {
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "request at seq {seq} carries MCP-Protocol-Version {refused:?}, which the \
                     server declined at initialize (it answered another version at seq \
                     {declined_at}), yet was answered HTTP {}; an unsupported version must be \
                     answered 400 Bad Request",
                    exchange.status
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
