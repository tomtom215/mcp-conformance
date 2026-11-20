// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `TRAN-007` at `2025-11-25`, `TRAN-052` at `2026-07-28`: "If the `Origin`
//! header is present and invalid, servers MUST respond with HTTP 403
//! Forbidden."
//!
//! Which origins a server holds invalid is its own configuration, and no trace
//! carries it — the reason both clauses were excluded. What a trace can carry
//! is the server *acting* on that configuration: one request refused, and the
//! same request served once its `Origin` was changed or dropped. That pair is
//! the server's own statement that the first origin was invalid to it, and the
//! clause then names the one status the refusal may have.
//!
//! **Stimulus.** An exchange whose request carries a well-formed web origin
//! (`http://` or `https://` and a host), refused with a status that blames the
//! request (a `4xx` other than `401`, `408`, `429`), where the trace also holds
//! its twin ([`served_twin`]): the same verb, the same recorded headers —
//! `Host` included, so a refusal on the host cannot pass for one on the origin
//! — but for `Origin`, the same message but for its `id`, answered `2xx`.
//!
//! A malformed `Origin` value is left out: a front end may refuse it as a
//! malformed HTTP request before any MCP server sees it.
//!
//! **Conforming.** `403`.
//!
//! **Fails.** Any other refusal status.
//!
//! The existing `http-origin-refused-without-id` traces change `Host` together
//! with `Origin`, as a DNS-rebinding request does; they show the conforming
//! shape but not which of the two headers was refused, so the clause is *not
//! observed* on them.

use super::super::FindingSink;
use super::super::stateless::transport::framing::Framing;
use super::twin::{blames_the_request, served_twin};
use crate::context::TraceContext;

/// The header under judgment, as the trace reader spells it.
const ORIGIN: &str = "origin";

/// Whether `origin` is a web origin: `http://` or `https://`, then a host with
/// an optional port. Syntax only.
fn is_web_origin(origin: &str) -> bool {
    origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .is_some_and(|host| {
            !host.is_empty()
                && host.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b':' | b'[' | b']')
                })
        })
}

/// `TRAN-007` / `TRAN-052`: a request refused for its `Origin` is refused `403`.
pub(in crate::checks) fn invalid_origin_403(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let framing = Framing::new(context);
    for exchange in framing.exchanges() {
        let Some((request_seq, _, headers)) = exchange.request else {
            continue;
        };
        let Some(origin) = headers.get(ORIGIN) else {
            continue;
        };
        if !is_web_origin(origin) || !blames_the_request(exchange.status) {
            continue;
        }
        let Some(twin) = served_twin(framing, exchange, Some(ORIGIN)) else {
            continue;
        };
        sink.examined();
        if exchange.status != 403 {
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "request at seq {request_seq} carries Origin {origin:?} and was refused HTTP \
                     {}, while the same request without that Origin was answered HTTP {} at seq \
                     {}; a request refused for its Origin must be answered 403 Forbidden",
                    exchange.status, twin.status, twin.status_seq
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
