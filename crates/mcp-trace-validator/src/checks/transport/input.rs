// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `TRAN-028` at `2025-11-25`, `TRAN-062` at `2026-07-28`: a posted
//! notification or response the server cannot accept draws an HTTP error
//! status.
//!
//! The clause is the second half of a pair. Of a POST whose input is a
//! JSON-RPC response or notification, the page says: "If the server accepts
//! the input, the server MUST return HTTP status code 202 Accepted with no
//! body. If the server cannot accept the input, it MUST return an HTTP error
//! status code (e.g., 400 Bad Request). The HTTP response body MAY comprise a
//! JSON-RPC *error response* that has no `id`." `TRAN-027`/`TRAN-061` judge the
//! first half; this is the second.
//!
//! Whether a server *could* accept an input is its own judgment. The trace
//! shows that judgment in two ways, and only those are read:
//!
//! - an error status: the server did not accept, and said so as required;
//! - the body the page gives a refusal — a JSON-RPC error response with no
//!   `id` — written straight after a status that is *not* an error. A
//!   notification or a response has no id to answer, so an id-less error in
//!   its HTTP response is the server declining it in-band.
//!
//! **Stimulus.** An exchange the framing attributes whose POST carried a
//! JSON-RPC notification or response (an object that is not a request), and
//! which the server declined in one of those two ways. An input answered `202`
//! (or any non-error status) with no such body was accepted, and is
//! `TRAN-027`'s subject, not this clause's.
//!
//! **Conforming.** A `4xx` or `5xx` status, with or without the body.
//!
//! **Fails.** A status below `400` followed at once by an id-less JSON-RPC
//! error response.
//!
//! A POST whose body is not JSON-RPC at all is outside the quote, which speaks
//! of "the input" of the response-or-notification branch, and is not judged.

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent};
use serde_json::Value;

use super::super::FindingSink;
use super::super::stateless::transport::framing::Framing;
use crate::context::TraceContext;

/// `TRAN-028` / `TRAN-062`: an input the server declines is declined with an
/// HTTP error status.
pub(in crate::checks) fn unaccepted_input_error_status(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        let Some(payload) = exchange.carried else {
            continue;
        };
        if exchange.carried_request() || !payload.is_object() {
            continue;
        }
        if exchange.status >= 400 {
            sink.examined();
        } else if declined_in_band(context.events(), exchange.status_seq) {
            sink.examined();
            let what = if payload.get("method").is_some() {
                "notification"
            } else {
                "response"
            };
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "the server declined a POSTed JSON-RPC {what} with an id-less JSON-RPC error \
                     under HTTP {}; an input the server cannot accept must be answered with an \
                     HTTP error status",
                    exchange.status
                ),
            );
        }
    }
}

/// Whether the event straight after the status at `status_seq` is a JSON-RPC
/// error response with no `id` — the body a recorder writes with its status.
fn declined_in_band(events: &[TraceEvent], status_seq: u64) -> bool {
    let Ok(at) = events.binary_search_by_key(&status_seq, |event| event.seq) else {
        return false;
    };
    events.get(at + 1).is_some_and(|next| {
        next.direction == Direction::ServerToClient
            && matches!(&next.body, EventBody::Message { payload }
                if payload.get("error").is_some_and(Value::is_object)
                    && payload.get("id").is_none_or(Value::is_null))
    })
}

#[cfg(test)]
mod tests;
