// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a server answers a request whose session it ended (`TRAN-015`), and a
//! request that names no session at all (`TRAN-014`). Both are `2025-11-25`
//! clauses; the `2026-07-28` transport has no sessions.
//!
//! Each is a clause about a request an ordinary client does not send, so each
//! is judged only where the trace holds that request — the stimulus — and is
//! *not observed* everywhere else. Both read the exchanges the framing
//! attributes ([`Framing`]): a status tied to exactly one client `http` event.
//!
//! [`Framing`]: super::super::stateless::transport::framing::Framing

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::Direction;

use super::super::FindingSink;
use super::super::stateless::transport::framing::{Exchange, Framing};
use super::http_headers;
use super::twin::{blames_the_request, carries_initialize, served_twin};
use crate::context::TraceContext;

/// The header a session rides, as the trace reader spells it.
const SESSION: &str = "mcp-session-id";

/// `TRAN-015`: "The server MAY terminate the session at any time, after which
/// it MUST respond to requests containing that session ID with HTTP 404 Not
/// Found."
///
/// **Stimulus.** A session the recording *shows* ended, then a request naming
/// it:
///
/// 1. a `DELETE` carrying `MCP-Session-Id: S`, answered `200` or `204` — the
///    server saying it has done what the client asked, which was to terminate
///    `S`. A `202 Accepted` is left out (HTTP defines it as "accepted for
///    processing", so the session may still be live when the next request
///    lands), as is a `405`, the refusal to terminate that the page permits;
/// 2. a later `POST` or `GET` carrying `S` which is not an `initialize` (a POST
///    whose body the framing could not pair is not known not to be). "Later"
///    needs no comparison: the framing attributes a status only to an exchange
///    that was alone in awaiting one, so two attributed exchanges never
///    overlap, and one answered after the `DELETE` was also sent after it.
///    A repeated `DELETE` is not judged: answering it idempotently is a common
///    reading, and nothing on the page separates it from the clause's
///    "requests".
///
/// `S` stops being a terminated session if the server issues it again — a
/// response header naming `S` on an exchange whose request did not, or on a
/// status the framing could not attribute.
///
/// **Conforming.** `404`.
///
/// **Fails.** A `2xx`: the terminated session was served. Or a refusal that
/// blames the request with another status (`400`, `410`, …) while the trace
/// also holds that same request — verb, recorded headers, message — served,
/// so that nothing but the session's end explains the refusal. `401`, `403`,
/// `408`, `429` and `5xx` are never judged: each has a cause the recording
/// does not carry.
pub(in crate::checks) fn terminated_session_404(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    let by_status: BTreeMap<u64, &Exchange<'_>> = framing
        .exchanges()
        .iter()
        .map(|exchange| (exchange.status_seq, exchange))
        .collect();
    let mut terminated: BTreeSet<&str> = BTreeSet::new();
    for (seq, headers) in http_headers(context, Direction::ServerToClient) {
        let exchange = by_status.get(&seq).copied();
        let request = exchange.and_then(|exchange| exchange.request);
        let named = request.and_then(|(_, _, sent)| sent.get(SESSION));
        if let Some(issued) = headers.get(SESSION)
            && named != Some(issued)
        {
            terminated.remove(issued.as_str());
        }
        let (Some(exchange), Some((request_seq, method, _)), Some(session)) =
            (exchange, request, named)
        else {
            continue;
        };
        if method == Some("DELETE") {
            if matches!(exchange.status, 200 | 204) {
                terminated.insert(session);
            }
        } else if terminated.contains(session.as_str()) && names_a_live_request(exchange) {
            judge_terminated(framing, exchange, request_seq, session, sink);
        }
    }
}

/// Whether `exchange` is a `GET`, or a `POST` known not to be an `initialize`.
fn names_a_live_request(exchange: &Exchange<'_>) -> bool {
    match exchange.method() {
        Some("GET") => true,
        Some("POST") => exchange.carried.is_some() && !carries_initialize(exchange),
        _ => false,
    }
}

/// Holds one answer to a request on a terminated session against `TRAN-015`.
fn judge_terminated(
    framing: &Framing<'_>,
    exchange: &Exchange<'_>,
    request_seq: u64,
    session: &str,
    sink: &mut FindingSink,
) {
    let status = exchange.status;
    let served = (200..300).contains(&status);
    let refused_otherwise = blames_the_request(status)
        && status != 403
        && served_twin(framing, exchange, None).is_some();
    if status != 404 && !served && !refused_otherwise {
        return;
    }
    sink.examined();
    if status != 404 {
        sink.push(
            Some(exchange.status_seq),
            format!(
                "request at seq {request_seq} carries MCP-Session-Id {session:?}, a session the \
                 server terminated when it answered its DELETE, yet was answered HTTP {status}; \
                 requests on a terminated session must be answered 404 Not Found"
            ),
        );
    }
}

/// `TRAN-014`: "Servers that require a session ID SHOULD respond to requests
/// without an `MCP-Session-Id` header (other than initialization) with HTTP 400
/// Bad Request."
///
/// **Stimulus.** A request carrying no `MCP-Session-Id` that is not an
/// `initialize`, refused with a status that blames the request (a `4xx` other
/// than `401`, `403`, `408`, `429`), where the trace also holds its twin: the
/// same verb, the same recorded headers plus an `MCP-Session-Id`, the same
/// message but for its `id`, answered `2xx`. The pair is what shows the
/// antecedent — a server that serves the request with a session id and refuses
/// it without one *requires* a session id — which no single exchange can.
///
/// **Conforming.** `400`. A server that serves the session-less request does
/// not require a session id, is outside the clause, and is not a subject.
///
/// **Warns.** Any other refusal status, `404` included: the clause names
/// `400`, and `404` is what the page reserves for a session that has ended.
pub(in crate::checks) fn missing_session_id_400(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        let Some((request_seq, _, headers)) = exchange.request else {
            continue;
        };
        let status = exchange.status;
        if headers.contains_key(SESSION)
            || carries_initialize(exchange)
            || !blames_the_request(status)
            || status == 403
        {
            continue;
        }
        let Some(twin) = served_twin(framing, exchange, Some(SESSION)) else {
            continue;
        };
        sink.examined();
        if status != 400 {
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "request at seq {request_seq} carries no MCP-Session-Id and was refused HTTP \
                     {status}, while the same request with one was answered HTTP {} at seq {}; a \
                     server that requires a session ID should answer its absence with 400 Bad \
                     Request",
                    twin.status, twin.status_seq
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
