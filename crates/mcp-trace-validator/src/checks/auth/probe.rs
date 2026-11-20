// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The invalid-token probe: what a protected server does with a credential
//! nobody issued.
//!
//! A recording never holds a credential, so whether a token a client sent
//! was valid is not in it. One `Authorization` value is: the public constant
//! `mcp-conformance check` presents on purpose
//! ([`PROBE_AUTHORIZATION`]), which a recorder keeps because it is not a
//! credential. A request carrying it is a request with a token that no
//! authorization server issued, by construction.
//!
//! **Stimulus.** An HTTP exchange whose request carries that value, in a trace
//! where the *same request without it* — same method, same message but for
//! its `id`, same recorded headers but for `Authorization` — was answered
//! `401` with a `Bearer` challenge. The twin is what makes the operation a
//! protected one: a server that serves an operation to anybody may ignore a
//! credential it never asked for, and a `401` the server sends for some other
//! request says nothing about this one.

use mcp_conformance_core::trace::PROBE_AUTHORIZATION;
use serde_json::Value;

use super::super::stateless::transport::framing::{Exchange, Framing};
use super::challenge;
use crate::checks::FindingSink;
use crate::context::TraceContext;

/// Whether `headers` carry a `Bearer` challenge.
fn challenges_bearer(exchange: &Exchange<'_>) -> bool {
    exchange
        .headers
        .get("www-authenticate")
        .is_some_and(|value| {
            challenge::parse(value)
                .iter()
                .any(challenge::Challenge::is_bearer)
        })
}

/// `message` without its `id`, which differs between a request and its twin.
fn without_id(message: &Value) -> Value {
    let mut message = message.clone();
    if let Some(members) = message.as_object_mut() {
        members.remove("id");
    }
    message
}

/// Whether `other` is `subject`'s request sent without the probe credential
/// and answered `401` with a `Bearer` challenge.
fn challenged_twin(subject: &Exchange<'_>, other: &Exchange<'_>) -> bool {
    let (Some((_, method, headers)), Some((_, other_method, other_headers))) =
        (subject.request, other.request)
    else {
        return false;
    };
    let same_headers = headers
        .iter()
        .filter(|(name, _)| name.as_str() != "authorization")
        .eq(other_headers.iter());
    let same_message = match (subject.carried, other.carried) {
        (Some(left), Some(right)) => without_id(left) == without_id(right),
        _ => false,
    };
    method.is_some()
        && method == other_method
        && same_headers
        && same_message
        && other.status == 401
        && challenges_bearer(other)
}

/// The exchanges that carry the probe credential to a protected operation.
fn probes<'f, 'a>(framing: &'f Framing<'a>) -> impl Iterator<Item = &'f Exchange<'a>> {
    framing.exchanges().iter().filter(|exchange| {
        exchange.request.is_some_and(|(_, _, headers)| {
            headers.get("authorization").map(String::as_str) == Some(PROBE_AUTHORIZATION)
        }) && framing
            .exchanges()
            .iter()
            .any(|other| challenged_twin(exchange, other))
    })
}

/// `AUTH-042`, `AUTH-047`, `AUTH-048`, `AUTH-081`, `AUTH-082` and their
/// `2026-07-28` counterparts: a server validates the tokens it is given and
/// accepts only valid ones.
///
/// **Stimulus:** see the module.
///
/// **Conforming answer:** anything but success. A token nobody issued cannot
/// pass validation, so the request is not served.
///
/// **Violation (fail):** a `2xx`. The server answered a protected operation
/// for a credential that is not one, so it did not validate it.
///
/// **Not judged here:** which refusal the server chose
/// ([`invalid_token_challenged`]).
pub(in crate::checks) fn invalid_token_not_accepted(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    for exchange in probes(framing) {
        sink.examined();
        if (200..300).contains(&exchange.status) {
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "the server answered HTTP {} to a request carrying a token no authorization \
                     server issued, though it had refused the same request without a credential \
                     with 401; it served the request without validating the token",
                    exchange.status
                ),
            );
        }
    }
}

/// `AUTH-044` / `AUTH-123`: "If validation fails, servers MUST respond
/// according to OAuth 2.1 Section 5.3 error handling requirements" — an
/// invalid token is answered `401` with a `WWW-Authenticate` challenge.
///
/// **Stimulus:** see the module, where the server refused the request with a
/// `4xx`. An accepted token is the other check's failure, and a `5xx`, `408`
/// or `429` says the server could not answer, not how it answers.
///
/// **Conforming answer:** `401` carrying a `Bearer` challenge.
///
/// **Violation (fail):** any other `4xx`, or a `401` with no `Bearer`
/// challenge.
pub(in crate::checks) fn invalid_token_challenged(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let framing = Framing::new(context);
    for exchange in probes(framing) {
        if !(400..500).contains(&exchange.status) || matches!(exchange.status, 408 | 429) {
            continue;
        }
        sink.examined();
        if exchange.status != 401 {
            sink.push(
                Some(exchange.status_seq),
                format!(
                    "the server refused a token no authorization server issued with HTTP {}; \
                     a token that fails validation is answered with 401",
                    exchange.status
                ),
            );
        } else if !challenges_bearer(exchange) {
            sink.push(
                Some(exchange.status_seq),
                "the server refused a token no authorization server issued with 401 and no \
                 `WWW-Authenticate: Bearer` challenge, which the refusal must carry"
                    .to_owned(),
            );
        }
    }
}

#[cfg(test)]
mod tests;
