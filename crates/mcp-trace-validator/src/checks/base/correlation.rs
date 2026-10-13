// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Response correlation: `BASE-004` and `BASE-009`, and the one walk that
//! answers both.
//!
//! Split from [`super`] because the two clauses cannot be judged
//! independently. A request is answered exactly once, by a result *xor* an
//! error, so deciding whether a result is unsolicited requires knowing which
//! errors have already consumed their requests — and vice versa. The shared
//! walk below is that bookkeeping, and it is long enough to deserve reading on
//! its own.

use std::collections::{HashMap, HashSet};

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;
use crate::context::framing::Framing;

/// Walks responses and reports those of the wanted flavor that match no outstanding
/// request from the opposite party. Shared by `BASE-004` (results) and `BASE-009`
/// (errors).
///
/// A request is answered exactly once, by a result XOR an error. Each flavor's pass
/// therefore consumes the outstanding entry on *both* flavors — its own (flagging a
/// mismatch) and the other's (silently, as that other response is the legitimate
/// first answer). The consequence: a request answered by both a result and an error,
/// in either order, leaves the *second* response with no outstanding request, and the
/// pass for the second response's flavor flags it. Without the cross-flavor consume,
/// each pass saw a clean 1-request→1-response and a double-answer slipped through.
fn responses_match_requests(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    want_results: bool,
) {
    // Outstanding request ids per requesting party, canonical id -> request seq.
    let mut outstanding: HashMap<(Direction, String), u64> = HashMap::new();
    let framed = framed_answers(context);
    let refusals = origin_refusals(context);
    for (event, kind, _) in context.messages() {
        // A null-id answer the HTTP framing ties to its request still answers
        // it: that request is no longer outstanding, whichever pass this is.
        if let Some(request) = framed.get(&event.seq) {
            release(request, &mut outstanding);
        }
        match kind {
            MessageKind::Request { id, .. } => {
                if !id.is_null() {
                    outstanding.insert((event.direction, to_canonical_string(id)), event.seq);
                }
            }
            // A malformed request whose id can still be read — `"method": 5`, or
            // no `method` at all — is answered with an error that "MUST have the
            // same ID as the request it corresponds to"; the null-id escape hatch
            // is only for an id that "could not be read". So a readable id on an
            // unclassifiable, non-response message opens an outstanding request
            // like any other. Server negative tests send exactly these.
            MessageKind::Invalid { .. } => {
                let readable_id = event.message_payload().and_then(|payload| {
                    let response_shaped =
                        payload.get("result").is_some() || payload.get("error").is_some();
                    payload
                        .get("id")
                        .filter(|id| !response_shaped && (id.is_string() || id.is_number()))
                });
                if let Some(id) = readable_id {
                    outstanding.insert((event.direction, to_canonical_string(id)), event.seq);
                }
            }
            MessageKind::Result { id } => {
                if want_results {
                    // The subject is a response of the flavour this pass
                    // judges; the other flavour is consumed silently and is
                    // not this clause's business.
                    sink.examined();
                    check_response_id(
                        event.seq,
                        event.direction,
                        *id,
                        &mut outstanding,
                        sink,
                        "result",
                    );
                } else {
                    // The other flavor's valid first answer: consume so a later
                    // same-id error is seen as answering an already-answered request.
                    consume_outstanding(event.direction, *id, &mut outstanding);
                }
            }
            MessageKind::Error { id, .. } => {
                // The null/absent-id condition is the spec's escape hatch ("except in
                // error cases where the ID could not be read due a malformed request"),
                // so a null/absent error id is not flagged on its own — the trace
                // usually cannot say which request it answered. When the framing
                // can, the request it answered is in hand, and so is whether its id
                // could be read.
                if want_results {
                    consume_outstanding(event.direction, *id, &mut outstanding);
                } else if id.is_some_and(|id| !id.is_null()) {
                    sink.examined();
                    check_response_id(
                        event.seq,
                        event.direction,
                        *id,
                        &mut outstanding,
                        sink,
                        "error",
                    );
                } else if let Some(request) = framed.get(&event.seq) {
                    check_unread_id(event.seq, request, refusals.contains(&event.seq), sink);
                }
            }
            _ => {}
        }
    }
}

/// Judges a null-id error the framing ties to `request`: excused when the
/// request's own id could not be read, or when the error is the body of an
/// `Origin` refusal (`origin_refusal`).
fn check_unread_id(seq: u64, request: &TraceEvent, origin_refusal: bool, sink: &mut FindingSink) {
    sink.examined();
    if origin_refusal {
        return;
    }
    if let Some(readable) = readable_id(request) {
        sink.push(
            Some(seq),
            format!(
                "error response carries a null id, but the request it answers (seq {}) has \
                 the readable id {readable}; only an id that could not be read may be \
                 answered with null",
                request.seq
            ),
        );
    }
}

/// The `seq` of every response message that is the body of an HTTP `403`
/// answering a request that carried an `Origin` header.
///
/// The Streamable HTTP transport's security warning, at both revisions: "If
/// the `Origin` header is present and invalid, servers MUST respond with HTTP
/// 403 Forbidden. The HTTP response body MAY comprise a JSON-RPC _error
/// response_ that has no `id`." Such a refusal happens before the request is
/// read, so its id was never available to echo, and the transport clause
/// permits exactly the id-less error the message clause would otherwise
/// forbid. Until 2026-10-13 that error drew BASE-009 on every recording of the
/// official suite's `dns-rebinding-protection` scenario taken in front of the
/// server's `Origin` check (`conformance/wire-findings.json`).
///
/// Whether the `Origin` was *invalid* is the server's policy, which the trace
/// does not hold; a `403` to a request that carried one is read as that
/// refusal. A `403` to a request with no `Origin` is not the case the clause
/// describes, and stays judged.
fn origin_refusals(context: &TraceContext<'_>) -> HashSet<u64> {
    const FORBIDDEN: u16 = 403;
    let framing = Framing::new(context);
    let refused: HashSet<u64> = framing
        .exchanges()
        .iter()
        .filter(|exchange| exchange.status == FORBIDDEN)
        .filter(|exchange| {
            exchange
                .request
                .is_some_and(|(_, _, headers)| headers.contains_key("origin"))
        })
        .map(|exchange| exchange.status_seq)
        .collect();
    if refused.is_empty() {
        return HashSet::new();
    }
    context
        .messages()
        .filter(|(event, _, _)| {
            framing
                .status_for(event.seq)
                .is_some_and(|(status_seq, _)| refused.contains(&status_seq))
        })
        .map(|(event, _, _)| event.seq)
        .collect()
}

/// The null- or absent-`id` responses the context pairs with a request: response
/// `seq` → that request. Pairing by id never matches such a response to a
/// well-formed request, so these are the answers the HTTP framing ties to their
/// POST ([`Framing::unidentified_answers`]).
///
/// [`Framing::unidentified_answers`]: crate::context::framing::Framing::unidentified_answers
fn framed_answers<'a>(context: &TraceContext<'a>) -> HashMap<u64, &'a TraceEvent> {
    context
        .exchanges()
        .filter(|exchange| {
            exchange
                .response
                .message_payload()
                .and_then(|payload| payload.get("id"))
                .is_none_or(Value::is_null)
        })
        .map(|exchange| (exchange.response.seq, exchange.request))
        .collect()
}

/// The request's id, when it is one that could be read: a string or a number,
/// the only forms an id takes.
fn readable_id(request: &TraceEvent) -> Option<String> {
    request
        .message_payload()
        .and_then(|payload| payload.get("id"))
        .filter(|id| id.is_string() || id.is_number())
        .map(to_canonical_string)
}

/// Removes `request` from `outstanding` — only that request, not a later one
/// that reused its id.
fn release(request: &TraceEvent, outstanding: &mut HashMap<(Direction, String), u64>) {
    let Some(id) = readable_id(request) else {
        return;
    };
    let key = (request.direction, id);
    if outstanding.get(&key) == Some(&request.seq) {
        outstanding.remove(&key);
    }
}

/// Removes the outstanding request a response answers, without flagging — the path
/// for a response of the flavor a given pass does not judge. A null/absent id matches
/// no request and removes nothing.
fn consume_outstanding(
    response_direction: Direction,
    id: Option<&Value>,
    outstanding: &mut HashMap<(Direction, String), u64>,
) {
    if let Some(id) = id.filter(|id| !id.is_null()) {
        let requester = match response_direction {
            Direction::ClientToServer => Direction::ServerToClient,
            Direction::ServerToClient => Direction::ClientToServer,
        };
        outstanding.remove(&(requester, to_canonical_string(id)));
    }
}

fn check_response_id(
    seq: u64,
    response_direction: Direction,
    id: Option<&Value>,
    outstanding: &mut HashMap<(Direction, String), u64>,
    sink: &mut FindingSink,
    flavor: &str,
) {
    let requester = match response_direction {
        Direction::ClientToServer => Direction::ServerToClient,
        Direction::ServerToClient => Direction::ClientToServer,
    };
    match id {
        None => sink.push(
            Some(seq),
            format!("{flavor} response is missing its id; responses must echo the request id"),
        ),
        Some(id) if id.is_null() => sink.push(
            Some(seq),
            format!("{flavor} response carries a null id; responses must echo the request id"),
        ),
        Some(id) => {
            let key = (requester, to_canonical_string(id));
            if outstanding.remove(&key).is_none() {
                sink.push(
                    Some(seq),
                    format!(
                        "{flavor} response answers id {}, but that party has no outstanding request with that id (never sent, or already answered)",
                        key.1
                    ),
                );
            }
        }
    }
}

/// `BASE-004`: "Result responses MUST include the same ID as the request they
/// correspond to."
pub(in crate::checks) fn result_id_matches(context: &TraceContext<'_>, sink: &mut FindingSink) {
    responses_match_requests(context, sink, true);
}

/// `BASE-009`: "Error responses MUST include the same ID as the request they correspond
/// to (except in error cases where the ID could not be read due a malformed request)."
pub(in crate::checks) fn error_id_matches(context: &TraceContext<'_>, sink: &mut FindingSink) {
    responses_match_requests(context, sink, false);
}
