// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the server's HTTP status and `Content-Type` must be for each kind of
//! exchange: a request POST (`TRAN-029` at `2025-11-25`, `TRAN-063` at
//! `2026-07-28`), a GET (`TRAN-040`), and an accepted notification or response
//! POST (`TRAN-027`, `TRAN-061`).
//!
//! Each obligation depends on what the exchange carried, so each is judged only
//! on an exchange the trace attributes: a response status tied to exactly one
//! client `http` event, and — where the clause depends on the body — a body
//! paired with that event without ambiguity ([`Framing`] states the rules). An
//! overlapping exchange is left out, which can make a clause *not observed* but
//! never convict. A status no recorded exchange awaited (a recording with
//! response framing only) counts as answering a request when a JSON-RPC
//! response follows it.
//!
//! [`Framing`]: super::super::stateless::transport::framing::Framing

use super::super::FindingSink;
use super::super::stateless::transport::framing::{Exchange, Framing};
use crate::context::TraceContext;

const JSON: &str = "application/json";
const EVENT_STREAM: &str = "text/event-stream";

/// `TRAN-029` / `TRAN-063`: a request POST answered with a success status must
/// carry `application/json` or `text/event-stream` — so a `202`, which has no
/// body, does not answer a request. Error statuses are other clauses' subject.
pub(in crate::checks) fn request_success_content_type(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        if !is_success(exchange) || !exchange.carried_request() {
            continue;
        }
        sink.examined();
        match exchange.media_type() {
            Some(media) if media == JSON || media == EVENT_STREAM => {}
            Some(_) => sink.push(
                Some(exchange.status_seq),
                format!(
                    "HTTP {} answering a JSON-RPC request has Content-Type {:?}; it must be \
                     application/json or text/event-stream",
                    exchange.status,
                    exchange
                        .headers
                        .get("content-type")
                        .map_or("", String::as_str),
                ),
            ),
            None => sink.push(
                Some(exchange.status_seq),
                format!(
                    "HTTP {} answering a JSON-RPC request carries no Content-Type; a request \
                     must be answered with application/json or text/event-stream",
                    exchange.status
                ),
            ),
        }
    }
}

/// `TRAN-040`: a GET is answered with `text/event-stream` or `405`. Only a
/// success status is judged: an error such as a `404` for an expired session is
/// another clause's subject, and a `405` is the permitted refusal.
pub(in crate::checks) fn get_stream_content_type(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        if exchange.method() != Some("GET") || !is_success(exchange) {
            continue;
        }
        sink.examined();
        if exchange.media_type().as_deref() != Some(EVENT_STREAM) {
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "HTTP {} answering a GET has Content-Type {:?}; a GET must be answered with \
                     text/event-stream or 405 Method Not Allowed",
                    exchange.status,
                    exchange
                        .headers
                        .get("content-type")
                        .map_or("", String::as_str),
                ),
            );
        }
    }
}

/// `TRAN-027` / `TRAN-061`: a POST carrying a notification or a response that
/// the server accepts is answered `202 Accepted`. A success status other than
/// `202` is a violation; an error status is the refusal the neighbouring clause
/// permits. The clause's "with no body" half is not judged: a body after a
/// `202` would be recorded as server messages, which the framing does not tie
/// to the exchange.
pub(in crate::checks) fn accepted_input_status(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        let Some(payload) = exchange.carried else {
            continue;
        };
        if exchange.carried_request() || !payload.is_object() || !is_success(exchange) {
            continue;
        }
        sink.examined();
        if exchange.status != 202 {
            let what = if payload.get("method").is_some() {
                "notification"
            } else {
                "response"
            };
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "the server accepted a POSTed JSON-RPC {what} with HTTP {}; accepted \
                     notifications and responses are answered 202 Accepted",
                    exchange.status
                ),
            );
        }
    }
}

fn is_success(exchange: &Exchange<'_>) -> bool {
    (200..300).contains(&exchange.status)
}

#[cfg(test)]
mod tests;
