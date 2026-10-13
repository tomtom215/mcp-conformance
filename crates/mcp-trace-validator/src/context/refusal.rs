// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Requests a protected resource refused before reading them.
//!
//! An MCP server that requires authorization answers a request carrying no
//! usable token with an HTTP `401` and a `WWW-Authenticate` challenge, and one
//! whose token lacks a scope with a `403` and an `insufficient_scope`
//! challenge. Either way the request was refused at the HTTP layer, before the
//! protocol read it, and the client — once it has a token — sends the *same*
//! request again. The official SDKs' clients do exactly that, retrying the
//! message they sent with its id unchanged.
//!
//! Judged as two deliveries of one message, that retry reuses a request id
//! (`BASE-003` at `2025-11-25`; `BASE-045` at `2026-07-28`, since the refused
//! one never got a JSON-RPC response and so stays in flight) and opens a second
//! session where there was one. Both are artifacts of reading an HTTP refusal
//! as a protocol exchange. So a request message is **not delivered** when the
//! framing ties it, without ambiguity, to an exchange whose status is `401`, or
//! `403` with a `WWW-Authenticate` challenge, and no JSON-RPC response answered
//! it. Its retry is the delivery.
//!
//! **The refusal's body** is the HTTP layer's too. A resource server may say
//! what went wrong in a JSON object of its own — `{"error":"invalid_token"}` —
//! and a recorder writes any JSON body as a message event. Such a body, the
//! first event after the refusal's status and carrying no `jsonrpc` member, is
//! not a protocol message either: read as one, it failed `BASE-006`, `BASE-008`
//! and `BASE-009` and the wire schema, and hid the refusal from the rule above.
//!
//! What stays judged: the refusal itself, by the authorization checks, which
//! read the exchange and not the message; the refused request's validity, by
//! the wire schema; and a refusal answered with a JSON-RPC response, which is an
//! ordinary exchange — the server read that request and answered it.

use std::collections::BTreeSet;

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent};

use super::framing::{Exchange, Framing};

/// Whether `exchange`'s status refuses authorization at the HTTP layer.
fn is_authorization_refusal(exchange: &Exchange<'_>) -> bool {
    exchange.status == 401
        || (exchange.status == 403 && exchange.headers.contains_key("www-authenticate"))
}

/// What the HTTP layer refused, by message `seq`.
#[derive(Debug, Default)]
pub(super) struct Refusals {
    /// Request messages refused before they were read.
    pub requests: BTreeSet<u64>,
    /// Refusal bodies: JSON the resource server sent with its refusal that is
    /// not a JSON-RPC message.
    pub bodies: BTreeSet<u64>,
}

impl Refusals {
    /// Whether the message at `seq` never took part in the protocol.
    pub(super) fn contains(&self, seq: u64) -> bool {
        self.requests.contains(&seq) || self.bodies.contains(&seq)
    }
}

/// The refusals in `events`, whose framing is `framing`.
pub(super) fn refusals(events: &[TraceEvent], framing: &Framing<'_>) -> Refusals {
    let mut found = Refusals::default();
    for exchange in framing
        .exchanges()
        .iter()
        .filter(|exchange| is_authorization_refusal(exchange))
    {
        let body = body_after(events, exchange.status_seq);
        if let Some(seq) = body {
            found.bodies.insert(seq);
        }
        if exchange.framed_response && body.is_none() {
            continue; // Answered in JSON-RPC: the server read the request.
        }
        let Some((http_seq, _, _)) = exchange.request else {
            continue;
        };
        if let Some(post) = framing
            .posts()
            .iter()
            .find(|post| post.seq == http_seq && post.payload.get("method").is_some())
        {
            found.requests.insert(post.message_seq);
        }
    }
    found
}

/// The refusal body recorded with the status at `status_seq`: the next event,
/// when it is a server message that is a JSON object without `jsonrpc`. A
/// recorder writes a response's status and its JSON body together, so the body
/// is the event that follows.
fn body_after(events: &[TraceEvent], status_seq: u64) -> Option<u64> {
    let at = events
        .binary_search_by_key(&status_seq, |event| event.seq)
        .ok()?;
    let next = events.get(at + 1)?;
    match (&next.body, next.direction) {
        (EventBody::Message { payload }, Direction::ServerToClient)
            if payload
                .as_object()
                .is_some_and(|object| !object.contains_key("jsonrpc")) =>
        {
            Some(next.seq)
        }
        _ => None,
    }
}
