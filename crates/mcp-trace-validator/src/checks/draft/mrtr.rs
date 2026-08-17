// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Multi Round-Trip Requests: the pattern that replaced server-initiated requests.
//!
//! A round is two independent JSON-RPC requests. The server answers the first
//! with `resultType: "input_required"` carrying an `inputRequests` map, an opaque
//! `requestState`, or both; the client gathers what was asked for and sends a
//! *new* request — different id — carrying `inputResponses` and echoing the
//! state back.
//!
//! **How a retry is identified, and why it matters.** Nothing in the protocol
//! labels a request as a retry, so these checks use the two fields that exist
//! only for that purpose: a client request carrying `inputResponses` or
//! `requestState` is a retry, of the most recent `input_required` before it.
//! That is exact for a serial session and is the only correlation the wire
//! supports; the specification itself contemplates parallel requests
//! (MRTR-020), and where a session interleaves rounds these checks would pair a
//! retry with the wrong round. A request carrying neither field is never treated
//! as a retry, which is what keeps an ordinary follow-up request — a second
//! `tools/call` for something else entirely — from being judged as one.
//!
//! Ten of the page's clauses carry exclusions rather than checks, and they
//! cluster on one thing: `requestState` is opaque *by design*. Whether it is
//! integrity-protected, what it contains, and whether the server validated it
//! are all invisible to a recording that carries only the blob.

use std::collections::BTreeMap;

use mcp_conformance_core::trace::Direction;
use serde_json::{Map, Value};

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The client requests that may draw an `InputRequiredResult` (#supported-requests).
const SUPPORTED: &[&str] = &["prompts/get", "resources/read", "tools/call"];

/// The request objects an `inputRequests` value may be.
const INPUT_REQUEST_METHODS: &[&str] =
    &["elicitation/create", "sampling/createMessage", "roots/list"];

/// The `resultType` that marks a round as incomplete.
const INPUT_REQUIRED: &str = "input_required";

/// An `InputRequiredResult` and the request it answered.
#[derive(Debug, Clone, Copy)]
struct Round<'a> {
    /// The `seq` of the result.
    seq: u64,
    /// The originating request's `seq`, `id` text and `method`.
    origin: (u64, &'a Value, &'a str),
    /// The `inputRequests` map, when the result carried one.
    requests: Option<&'a Map<String, Value>>,
    /// The `requestState` blob, when the result carried one.
    state: Option<&'a Value>,
}

/// A client request that identifies itself as a retry.
#[derive(Debug, Clone, Copy)]
struct Retry<'a> {
    seq: u64,
    id: &'a Value,
    method: &'a str,
    responses: Option<&'a Map<String, Value>>,
    state: Option<&'a Value>,
}

/// Every `input_required` answer in the trace, paired with its originating request.
///
/// Driven from exchanges, so a result whose request is not in the recording is
/// skipped: without the request there is no method to judge against and no id to
/// compare a retry's against.
fn rounds<'a>(context: &'a TraceContext<'_>) -> Vec<Round<'a>> {
    context
        .exchanges()
        .filter_map(|exchange| {
            let result = exchange.result?;
            if result.get("resultType").and_then(Value::as_str) != Some(INPUT_REQUIRED) {
                return None;
            }
            let id = exchange.request.message_payload()?.get("id")?;
            Some(Round {
                seq: exchange.response.seq,
                origin: (exchange.request.seq, id, exchange.method),
                requests: result.get("inputRequests").and_then(Value::as_object),
                state: result.get("requestState"),
            })
        })
        .collect()
}

/// Every client request carrying a retry's marker fields, in trace order.
fn retries<'a>(context: &'a TraceContext<'_>) -> Vec<Retry<'a>> {
    context
        .messages()
        .filter_map(|(event, _, _)| {
            if event.direction != Direction::ClientToServer {
                return None;
            }
            let payload = event.message_payload()?;
            let method = payload.get("method")?.as_str()?;
            let id = payload.get("id").filter(|id| !id.is_null())?;
            let params = payload.get("params")?;
            let responses = params.get("inputResponses").and_then(Value::as_object);
            let state = params.get("requestState");
            (responses.is_some() || state.is_some()).then_some(Retry {
                seq: event.seq,
                id,
                method,
                responses,
                state,
            })
        })
        .collect()
}

/// Each retry paired with the round it answers: the most recent one before it.
///
/// One ordered pass rather than a `round.seq < retry.seq` comparison. A round is
/// a server *result* and a retry a client *request*, so no two can share a `seq`
/// — which makes `<` and `<=` indistinguishable by construction, a difference no
/// trace could ever exhibit and therefore no test could ever catch. Walking the
/// messages in order states the intent directly instead.
fn retries_with_rounds<'a>(context: &'a TraceContext<'_>) -> Vec<(Retry<'a>, Option<Round<'a>>)> {
    let rounds: BTreeMap<u64, Round<'a>> = rounds(context)
        .into_iter()
        .map(|round| (round.seq, round))
        .collect();
    let retries: BTreeMap<u64, Retry<'a>> = retries(context)
        .into_iter()
        .map(|retry| (retry.seq, retry))
        .collect();
    let mut latest: Option<Round<'a>> = None;
    let mut out = Vec::new();
    for (event, _, _) in context.messages() {
        if let Some(round) = rounds.get(&event.seq) {
            latest = Some(*round);
        } else if let Some(retry) = retries.get(&event.seq) {
            out.push((*retry, latest));
        }
    }
    out
}

/// `MRTR-004`: `InputRequiredResult` answers only the three supported requests.
pub(in crate::checks) fn input_required_supported_methods(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for round in rounds(context) {
        let (_, _, method) = round.origin;
        if !SUPPORTED.contains(&method) {
            sink.push(
                Some(round.seq),
                format!(
                    "`input_required` answers `{method}`; this revision permits it only on \
                     {}",
                    SUPPORTED.join(", ")
                ),
            );
        }
    }
}

/// `MRTR-006`: each `inputRequests` value is one of the three request objects.
pub(in crate::checks) fn input_request_methods(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for round in rounds(context) {
        let Some(requests) = round.requests else {
            continue;
        };
        for (key, request) in requests {
            match request.get("method").and_then(Value::as_str) {
                Some(method) if INPUT_REQUEST_METHODS.contains(&method) => {}
                Some(method) => sink.push(
                    Some(round.seq),
                    format!(
                        "`inputRequests[{key}]` asks for `{method}`, which is not one of \
                         ElicitRequest, CreateMessageRequest or ListRootsRequest"
                    ),
                ),
                None => sink.push(
                    Some(round.seq),
                    format!("`inputRequests[{key}]` is not a request object with a `method`"),
                ),
            }
        }
    }
}

/// `MRTR-011`: an `InputRequiredResult` carries `inputRequests`, `requestState`, or both.
///
/// A result with neither asks for nothing and remembers nothing, so the round it
/// opens can never be completed — which is why the clause makes it a MUST rather
/// than leaving both fields optional independently.
pub(in crate::checks) fn input_required_has_content(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for round in rounds(context) {
        if round.requests.is_none() && round.state.is_none() {
            sink.push(
                Some(round.seq),
                "`input_required` carries neither `inputRequests` nor `requestState`, so the \
                 round it opens cannot be completed"
                    .to_owned(),
            );
        }
    }
}

/// `MRTR-015`: a retry carries responses for everything the round asked for.
pub(in crate::checks) fn retry_carries_input_responses(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (retry, round) in retries_with_rounds(context) {
        let Some(round) = round else {
            continue;
        };
        for key in missing_keys(&round, &retry) {
            sink.push(
                Some(retry.seq),
                format!(
                    "the retry carries no `inputResponses[{key}]` for the input the \
                     `input_required` at seq {} asked for",
                    round.seq
                ),
            );
        }
    }
}

/// The `inputRequests` keys a retry left unanswered.
fn missing_keys(round: &Round<'_>, retry: &Retry<'_>) -> Vec<String> {
    let Some(requests) = round.requests else {
        return Vec::new();
    };
    requests
        .keys()
        .filter(|key| {
            !retry
                .responses
                .is_some_and(|responses| responses.contains_key(*key))
        })
        .cloned()
        .collect()
}

/// `MRTR-016`, `MRTR-003` and `MRTR-017`: the retry echoes `requestState` exactly.
///
/// The three clauses share this check because they state one rule from two
/// sides: the client must echo the exact value, and must not modify it. A
/// changed value is the only wire-visible form of "modified" — inspecting and
/// parsing leave no trace — so a finding here is a true finding for all three.
pub(in crate::checks) fn request_state_echoed(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (retry, round) in retries_with_rounds(context) {
        let Some(round) = round else {
            continue;
        };
        let Some(issued) = round.state else { continue };
        match retry.state {
            Some(echoed) if echoed == issued => {}
            Some(echoed) => sink.push(
                Some(retry.seq),
                format!(
                    "the retry echoes `requestState` {echoed} instead of the {issued} the \
                     `input_required` at seq {} issued",
                    round.seq
                ),
            ),
            None => sink.push(
                Some(retry.seq),
                format!(
                    "the retry omits the `requestState` the `input_required` at seq {} \
                     issued, which it must echo back exactly",
                    round.seq
                ),
            ),
        }
    }
}

/// `MRTR-018`: no `requestState` in a retry the server did not give one for.
pub(in crate::checks) fn no_unsolicited_request_state(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (retry, round) in retries_with_rounds(context) {
        if retry.state.is_none() {
            continue;
        }
        let issued = round.and_then(|round| round.state);
        if issued.is_none() {
            sink.push(
                Some(retry.seq),
                "the request carries a `requestState` that no `input_required` before it \
                 issued"
                    .to_owned(),
            );
        }
    }
}

/// `MRTR-019`: the retry is a new request, with a new id.
pub(in crate::checks) fn retry_id_differs(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (retry, round) in retries_with_rounds(context) {
        let Some(round) = round else {
            continue;
        };
        let (origin_seq, origin_id, _) = round.origin;
        if retry.id == origin_id {
            sink.push(
                Some(retry.seq),
                format!(
                    "the retry reuses id {origin_id} from the request at seq {origin_seq}; \
                     the two are independent requests and must not share one"
                ),
            );
        }
    }
}

/// `MRTR-020`: a round's state is used for its own retry and nothing else.
///
/// Judged by method: a `requestState` presented on a request of a different
/// method than the one that drew it is being used for some other request, which
/// is what the clause forbids. Two retries of the *same* method are not reported
/// — the specification explicitly allows a server to open a further round on a
/// repeated attempt (`#server-requirements-basic-workflow`, item 8).
pub(in crate::checks) fn request_state_scoped_to_retry(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Every state a round issued, and the method of the request that drew it.
    let issued: BTreeMap<String, &str> = rounds(context)
        .iter()
        .filter_map(|round| round.state.map(|state| (state.to_string(), round.origin.2)))
        .collect();
    for retry in retries(context) {
        let Some(state) = retry.state else { continue };
        let Some(&origin_method) = issued.get(&state.to_string()) else {
            continue;
        };
        if retry.method != origin_method {
            sink.push(
                Some(retry.seq),
                format!(
                    "`{}` carries the `requestState` issued for a `{origin_method}` request; \
                     it affects only that request's retry",
                    retry.method
                ),
            );
        }
    }
}

/// `MRTR-024`: a shortfall draws another `input_required`, not an error.
///
/// Fires only when the trace shows all three parts the clause names: a round
/// that asked for input, a retry that did not supply all of it, and an *error*
/// answering that retry. The clause's remaining condition — that the missing
/// information was necessary — is the server's own judgement and is not
/// observable; a server that could proceed without it would have completed the
/// request rather than failing it, which is why the error is treated as
/// evidence that it could not.
pub(in crate::checks) fn missing_input_reasked(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let paired: BTreeMap<u64, (Retry<'_>, Option<Round<'_>>)> = retries_with_rounds(context)
        .into_iter()
        .map(|(retry, round)| (retry.seq, (retry, round)))
        .collect();
    for exchange in context.exchanges() {
        let Some((retry, Some(round))) = paired.get(&exchange.request.seq).copied() else {
            continue;
        };
        let missing = missing_keys(&round, &retry);
        if missing.is_empty() || exchange.result.is_some() {
            continue;
        }
        sink.push(
            Some(exchange.response.seq),
            format!(
                "the retry omitted {} that the `input_required` at seq {} asked for, and the \
                 server answered with an error rather than asking again",
                missing
                    .iter()
                    .map(|key| format!("`{key}`"))
                    .collect::<Vec<_>>()
                    .join(", "),
                round.seq
            ),
        );
    }
}
