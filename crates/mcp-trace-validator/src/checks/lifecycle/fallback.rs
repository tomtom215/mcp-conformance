// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `LIFE-012`: the version a server falls back to "SHOULD be the latest
//! version supported by the server".
//!
//! Split from [`super`] because it is the one lifecycle check that reads every
//! `initialize` exchange a trace holds rather than the one the session ran
//! under.

use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// One `initialize` request the server answered with a dated version.
struct Negotiation<'a> {
    /// The result's `seq`.
    seq: u64,
    /// The version the client asked for, when it sent a string.
    requested: Option<&'a str>,
    /// The version the server answered, as written and as a date.
    answered: (&'a str, ProtocolRevision),
}

/// Every `initialize` the server answered with a result naming a dated
/// version, in response order.
fn negotiations<'a>(context: &TraceContext<'a>) -> Vec<Negotiation<'a>> {
    context
        .exchanges_for("initialize")
        .filter(|exchange| exchange.request.direction == Direction::ClientToServer)
        .filter_map(|exchange| {
            let answered = exchange.result?.get("protocolVersion")?.as_str()?;
            Some(Negotiation {
                seq: exchange.response.seq,
                requested: exchange
                    .params
                    .and_then(|params| params.get("protocolVersion"))
                    .and_then(Value::as_str),
                answered: (answered, answered.parse().ok()?),
            })
        })
        .collect()
}

/// `LIFE-012`: a server that does not support the requested version answers
/// with the latest one it does support.
///
/// The set of versions a server supports is internal, but a trace can hold
/// more than one `initialize` exchange — a client that tries a second version,
/// a harness that opens several sessions against one endpoint — and every
/// version the server names in an `initialize` result is one it supports: it
/// either agreed to it ("If the server supports the requested protocol
/// version, it MUST respond with the same version") or offered it ("Otherwise,
/// the server MUST respond with another protocol version it supports").
///
/// **Stimulus.** An `initialize` request whose `protocolVersion` is a string,
/// answered by a result whose `protocolVersion` is a *different* dated
/// version — the fallback — in a trace that holds at least one other
/// `initialize` result naming a dated version. Each such fallback is a
/// subject; a trace with a single answered `initialize` shows nothing to
/// compare it with and leaves the clause not observed.
///
/// **Conforming.** The fallback is the latest (by date) of the versions the
/// server names anywhere in the trace.
///
/// **Violation (warn; the clause is a SHOULD).** Another `initialize` result
/// in the trace names a later version than the fallback.
///
/// That the exchanges are one server's is what a trace is: one recorded
/// transport — one stdio process, or one HTTP endpoint behind the recorder —
/// which the validator judges as one peer throughout. A deployment that
/// changes its supported versions between two handshakes of one recording
/// would read as a server that did not offer its latest; from the client's
/// side of that endpoint, it did not.
pub(in crate::checks) fn fallback_version_latest(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let negotiations = negotiations(context);
    let Some(latest) = negotiations
        .iter()
        .map(|negotiation| negotiation.answered)
        .max_by_key(|(_, date)| *date)
    else {
        return;
    };
    if negotiations.len() < 2 {
        return;
    }
    for negotiation in &negotiations {
        let (answered, date) = negotiation.answered;
        let Some(requested) = negotiation.requested.filter(|version| *version != answered) else {
            continue;
        };
        sink.examined();
        if latest.1 > date {
            sink.push(
                Some(negotiation.seq),
                format!(
                    "the server answered the unsupported version {requested:?} with \
                     {answered:?}, but another `initialize` result in this trace names \
                     {:?}, a later version it supports",
                    latest.0
                ),
            );
        }
    }
}
