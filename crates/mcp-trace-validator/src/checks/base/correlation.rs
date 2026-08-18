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

use std::collections::HashMap;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use crate::context::TraceContext;

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
    for (event, kind, _) in context.messages() {
        match kind {
            MessageKind::Request { id, .. } => {
                if !id.is_null() {
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
                // so a null/absent error id is neither flagged nor consumes anything.
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
                }
            }
            _ => {}
        }
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
