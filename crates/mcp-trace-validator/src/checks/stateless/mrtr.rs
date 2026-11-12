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
//! `requestState` is a retry. Which round it answers is decided by the state it
//! echoes, then by the request it repeats, and left undecided when rounds that
//! would be judged differently both fit — see [`pairing`], which explains why
//! "the most recent round" convicted conforming parallel clients. A request
//! carrying neither field is never treated
//! as a retry, which is what keeps an ordinary follow-up request — a second
//! `tools/call` for something else entirely — from being judged as one.
//!
//! Ten of the page's clauses carry exclusions rather than checks, and they
//! cluster on one thing: `requestState` is opaque *by design*. Whether it is
//! integrity-protected, what it contains, and whether the server validated it
//! are all invisible to a recording that carries only the blob.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod correlation_tests;
pub(in crate::checks) mod pairing;
#[cfg(test)]
mod tests;

use pairing::{Retry, Round, retries, retries_with_rounds, rounds};

/// The client requests that may draw an `InputRequiredResult` (#supported-requests).
const SUPPORTED: &[&str] = &["prompts/get", "resources/read", "tools/call"];

/// The request objects an `inputRequests` value may be.
const INPUT_REQUEST_METHODS: &[&str] =
    &["elicitation/create", "sampling/createMessage", "roots/list"];

/// `MRTR-004`: `InputRequiredResult` answers only the three supported requests.
pub(in crate::checks) fn input_required_supported_methods(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for round in rounds(context) {
        sink.examined();
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
            sink.examined();
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
        sink.examined();
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
    for (retry, pairing) in retries_with_rounds(context) {
        let Some(round) = pairing.round() else {
            continue;
        };
        // The subject is a retry of a round that actually asked for something:
        // where nothing was asked, there is nothing a retry could omit.
        if round.requests.is_none_or(Map::is_empty) {
            continue;
        }
        sink.examined();
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
    for (retry, pairing) in retries_with_rounds(context) {
        let Some(round) = pairing.round() else {
            continue;
        };
        let Some(issued) = round.state else { continue };
        sink.examined();
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
///
/// Judged only against a round the recording carries. A retry presenting state
/// with no round before it is not evidence of invention: `requestState` exists
/// so that a retry can reach a server instance that never saw the round
/// ("without requiring a shared storage layer across server instances"), and a
/// capture can begin between a round and its retry. The clause is falsified by
/// a retry of a recorded round that issued no state.
pub(in crate::checks) fn no_unsolicited_request_state(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (retry, pairing) in retries_with_rounds(context) {
        if retry.state.is_none() {
            continue;
        }
        // An unseen round, or rounds that would answer differently: no verdict
        // on a guess.
        let Some(round) = pairing.round() else {
            continue;
        };
        sink.examined();
        if round.state.is_none() {
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
    for (retry, pairing) in retries_with_rounds(context) {
        let Some(round) = pairing.round() else {
            continue;
        };
        sink.examined();
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
/// Judged by the request a retry repeats (its method and target) against the
/// rounds that had issued the state *when the retry was sent*. A state presented
/// on a request other than the one that drew it is being used for some other
/// request, which is what the clause forbids. Two retries of the same request
/// are not reported: the specification explicitly allows a server to open a
/// further round on a repeated attempt (`#server-requirements-basic-workflow`,
/// item 8), and nothing requires a server to mint a different opaque value for
/// an unrelated request, so any one round that issued the state for this
/// request is enough. A state nothing had yet issued is not judged here (the
/// clause on unsolicited state is the one for that), and a round issued later
/// says nothing about what an earlier retry used.
pub(in crate::checks) fn request_state_scoped_to_retry(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let rounds = rounds(context);
    for retry in retries(context) {
        let Some(state) = retry.state else { continue };
        let Some(latest) = rounds
            .iter()
            .filter(|round| round.seq < retry.seq && round.state == Some(state))
            .max_by_key(|round| round.seq)
        else {
            continue;
        };
        sink.examined();
        let own = rounds.iter().any(|round| {
            round.seq < retry.seq
                && round.state == Some(state)
                && round.origin.2 == retry.method
                && round.target == retry.target
        });
        if own {
            continue;
        }
        let issued_for = latest.origin.2;
        let detail = if issued_for == retry.method {
            // Same method, so the request differs by what it addresses.
            format!(
                "`{}` of {} carries the `requestState` issued for a `{issued_for}` request of {}; \
                 it affects only that request's retry",
                retry.method,
                addressed(retry.target),
                addressed(latest.target),
            )
        } else {
            format!(
                "`{}` carries the `requestState` issued for a `{issued_for}` request; \
                 it affects only that request's retry",
                retry.method
            )
        };
        sink.push(Some(retry.seq), detail);
    }
}

/// What a request addresses, for a message.
fn addressed(target: Option<&serde_json::Value>) -> String {
    target.map_or_else(|| "nothing named".to_owned(), ToString::to_string)
}

/// Whether `retry` carries a fault a server may reject it for whatever input was
/// missing: a `requestState` that is not the one the round issued (altered, or
/// dropped), or the id of the request it repeats.
fn has_fault_of_its_own(round: &Round<'_>, retry: &Retry<'_>) -> bool {
    let state = match (round.state, retry.state) {
        (Some(issued), Some(presented)) => issued != presented,
        (Some(_), None) => true,
        (None, _) => false,
    };
    state || retry.id == round.origin.1
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
///
/// That inference holds only when the shortfall is the retry's one fault. A
/// retry that also altered or dropped the `requestState` it was given, or reused
/// the first request's id, gives the server a reason of its own to refuse it
/// (the page requires a state that fails verification to be rejected), so the
/// error is no evidence about the missing input and the clause is left unjudged.
pub(in crate::checks) fn missing_input_reasked(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let paired: BTreeMap<u64, (Retry<'_>, Option<Round<'_>>)> = retries_with_rounds(context)
        .into_iter()
        .map(|(retry, pairing)| (retry.seq, (retry, pairing.round())))
        .collect();
    for exchange in context.exchanges() {
        let Some((retry, Some(round))) = paired.get(&exchange.request.seq).copied() else {
            continue;
        };
        let missing = missing_keys(&round, &retry);
        if missing.is_empty() {
            continue; // Nothing was omitted, so no shortfall to answer.
        }
        if has_fault_of_its_own(&round, &retry) {
            continue;
        }
        // The subject is an *answered* retry that fell short: the clause is
        // about which of the two answers the server chose.
        sink.examined();
        if exchange.result.is_some() {
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
