// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which `input_required` round a retry answers.
//!
//! Nothing on the wire labels a request as a retry *of* a particular round, and
//! the specification contemplates rounds in parallel (MRTR-020), so "the most
//! recent round before it" — the rule this module replaced — paired a retry with
//! another request's round whenever two rounds interleaved. The official Python
//! SDK does exactly that for two concurrent `tools/call`s, and every client-side
//! MRTR clause then reported a conforming client.
//!
//! A retry is now paired by the evidence it carries, strongest first:
//!
//! 1. **The echoed `requestState`.** The server minted it for one round and the
//!    client must echo it exactly, so an equal value names that round; the latest
//!    round issuing it wins, since a server may repeat a state across rounds of
//!    one request.
//! 2. **The original request.** A retry repeats its request's method and target
//!    (`params.name`, or `params.uri` for `resources/read`), so the unanswered
//!    rounds of that method and target are the candidates. One candidate is a
//!    pairing. Several are a pairing only when they asked for the same inputs and
//!    issued the same state — then every candidate yields the same verdict, so
//!    the choice cannot change a finding; otherwise the retry is
//!    [`Pairing::Ambiguous`] and judged by nothing, because convicting a client
//!    on a guessed pairing is the defect this module exists to remove.
//!
//! Step 2 is what still judges a client that altered or dropped the state: its
//! retry no longer matches by state, but it still names its request.

use mcp_conformance_core::trace::Direction;
use serde_json::{Map, Value};

use crate::context::TraceContext;

/// The `resultType` that marks a round as incomplete.
const INPUT_REQUIRED: &str = "input_required";

/// An `InputRequiredResult` and the request it answered.
#[derive(Debug, Clone, Copy)]
pub(super) struct Round<'a> {
    /// The `seq` of the result.
    pub seq: u64,
    /// The originating request's `seq`, `id` text and `method`.
    pub origin: (u64, &'a Value, &'a str),
    /// What the originating request addressed (`name` or `uri`), when stated.
    pub target: Option<&'a Value>,
    /// The `inputRequests` map, when the result carried one.
    pub requests: Option<&'a Map<String, Value>>,
    /// The `requestState` blob, when the result carried one.
    pub state: Option<&'a Value>,
}

/// A client request that identifies itself as a retry.
#[derive(Debug, Clone, Copy)]
pub(super) struct Retry<'a> {
    pub seq: u64,
    pub id: &'a Value,
    pub method: &'a str,
    pub target: Option<&'a Value>,
    pub responses: Option<&'a Map<String, Value>>,
    pub state: Option<&'a Value>,
}

/// What a retry was paired with.
#[derive(Debug, Clone, Copy)]
pub(super) enum Pairing<'a> {
    /// The round the retry answers.
    Round(Round<'a>),
    /// More than one round could be the one, and they would be judged differently.
    Ambiguous,
    /// No round before the retry could be the one.
    Unseen,
}

impl<'a> Pairing<'a> {
    /// The paired round, when the pairing is certain.
    pub(super) const fn round(self) -> Option<Round<'a>> {
        match self {
            Self::Round(round) => Some(round),
            Self::Ambiguous | Self::Unseen => None,
        }
    }
}

/// What a request addresses: `params.uri` for `resources/read`, else `params.name`.
fn target<'a>(method: &str, params: Option<&'a Value>) -> Option<&'a Value> {
    let field = if method == "resources/read" {
        "uri"
    } else {
        "name"
    };
    params?.get(field)
}

/// Every `input_required` answer in the trace, paired with its originating request.
///
/// Driven from exchanges, so a result whose request is not in the recording is
/// skipped: without the request there is no method to judge against and no id to
/// compare a retry's against.
pub(super) fn rounds<'a>(context: &'a TraceContext<'_>) -> Vec<Round<'a>> {
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
                target: target(exchange.method, exchange.params),
                requests: result.get("inputRequests").and_then(Value::as_object),
                state: result.get("requestState"),
            })
        })
        .collect()
}

/// Every client request carrying a retry's marker fields, in trace order.
pub(super) fn retries<'a>(context: &'a TraceContext<'_>) -> Vec<Retry<'a>> {
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
                target: target(method, Some(params)),
                responses,
                state,
            })
        })
        .collect()
}

/// Whether pairing `retry` with `a` rather than `b` could change any verdict:
/// the inputs asked for, the state issued, and whether the retry reused the
/// originating id are everything the client-side clauses compare.
fn interchangeable(retry: &Retry<'_>, a: &Round<'_>, b: &Round<'_>) -> bool {
    let same_asks = match (a.requests, b.requests) {
        (Some(a), Some(b)) => a.keys().eq(b.keys()),
        (None, None) => true,
        _ => false,
    };
    same_asks && a.state == b.state && (retry.id == a.origin.1) == (retry.id == b.origin.1)
}

/// Each retry paired with the round it answers, in trace order.
pub(super) fn retries_with_rounds<'a>(
    context: &'a TraceContext<'_>,
) -> Vec<(Retry<'a>, Pairing<'a>)> {
    let mut pending: Vec<Round<'a>> = rounds(context);
    pending.sort_by_key(|round| round.seq);
    let mut retries = retries(context);
    retries.sort_by_key(|retry| retry.seq);
    // Rounds already seen, with whether a retry has answered each.
    let mut seen: Vec<(Round<'a>, bool)> = Vec::new();
    let mut upcoming = pending.into_iter().peekable();
    let mut out = Vec::with_capacity(retries.len());
    for retry in retries {
        while let Some(round) = upcoming.next_if(|round| round.seq < retry.seq) {
            seen.push((round, false));
        }
        let pairing = pair(&retry, &mut seen);
        out.push((retry, pairing));
    }
    out
}

/// Pairs one retry against the rounds seen so far, marking the one it answers.
fn pair<'a>(retry: &Retry<'a>, seen: &mut [(Round<'a>, bool)]) -> Pairing<'a> {
    if let Some(state) = retry.state
        && let Some(index) = seen
            .iter()
            .rposition(|(round, _)| round.state == Some(state))
    {
        seen[index].1 = true;
        return Pairing::Round(seen[index].0);
    }
    let candidates: Vec<usize> = seen
        .iter()
        .enumerate()
        .filter(|(_, (round, answered))| {
            !answered && round.origin.2 == retry.method && round.target == retry.target
        })
        .map(|(index, _)| index)
        .collect();
    let Some(&latest) = candidates.last() else {
        return Pairing::Unseen;
    };
    let same_verdict = candidates
        .iter()
        .all(|&index| interchangeable(retry, &seen[index].0, &seen[latest].0));
    if !same_verdict {
        return Pairing::Ambiguous;
    }
    seen[latest].1 = true;
    Pairing::Round(seen[latest].0)
}
