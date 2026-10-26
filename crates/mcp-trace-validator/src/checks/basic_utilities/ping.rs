// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! basic/utilities/ping: the one message shape the page mandates.

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// `PING-001`: a `ping` is answered with an empty result.
///
/// "The receiver MUST respond promptly with an empty response" has two halves
/// and only one is on the wire. *Promptly* is a duration, and checks may not
/// consult time (the limit LIFE-015 records); a ping the recording never sees
/// answered is likewise not judged, because the recording may end before the
/// answer. What is judged is every ping that *was* answered, by either party:
/// the answer must be a result, and the result must be empty. `_meta` is the
/// one member allowed, since `EmptyResult` is `Result`, which carries it.
///
/// An error that rode an HTTP error status is not that answer. It is the
/// transport refusing the request, which other clauses require: a request with
/// an invalid or unsupported `MCP-Protocol-Version` "MUST" be answered `400`, and
/// a session the server no longer knows `404`. The ping handler never saw it, so
/// the receiver is not failing to respond with an empty result, and the exchange
/// is not a subject. An error over stdio, or one that rode a `2xx`, is judged.
pub(in crate::checks) fn empty_result(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for exchange in context.exchanges_for("ping") {
        let refused_by_transport = exchange.result.is_none()
            && context
                .http_status(exchange.response.seq)
                .is_some_and(|status| !(200..300).contains(&status));
        if refused_by_transport {
            continue;
        }
        // The subject is an answered ping, whichever party sent it.
        sink.examined();
        let Some(result) = exchange.result else {
            let code = exchange
                .response
                .message_payload()
                .and_then(|payload| payload.get("error")?.get("code"))
                .map_or_else(|| "no code".to_owned(), ToString::to_string);
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`ping` (seq {}) was answered with an error ({code}); the receiver must \
                     respond with an empty result",
                    exchange.request.seq
                ),
            );
            continue;
        };
        let Some(members) = result.as_object() else {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`ping` (seq {}) was answered with the result {result}, not an empty object",
                    exchange.request.seq
                ),
            );
            continue;
        };
        let extra: Vec<&str> = members
            .keys()
            .map(String::as_str)
            .filter(|key| *key != "_meta")
            .collect();
        if !extra.is_empty() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`ping` (seq {}) was answered with a result carrying {}; the receiver must \
                     respond with an empty result",
                    exchange.request.seq,
                    quoted(&extra)
                ),
            );
        }
    }
}

/// `keys` as `` `a`, `b` ``.
fn quoted(keys: &[&str]) -> String {
    keys.iter()
        .map(|key| format!("`{key}`"))
        .collect::<Vec<_>>()
        .join(", ")
}
