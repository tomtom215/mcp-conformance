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
//!    client must echo it exactly, so an equal value names that round. The value
//!    is opaque and the server's own, and the specification requires no
//!    uniqueness of it, so two unrelated rounds may carry the same one: among
//!    the rounds that issued it, those for the request the retry repeats (its
//!    method and target) are the candidates. A server may also repeat a state
//!    across the rounds of *one* request, so the latest unanswered candidate
//!    wins. When no round that issued the state was for this request, the state
//!    is being used on another request: the retry is paired with the latest
//!    round that issued it, which is what the clause on scoping then reports.
//! 2. **The original request.** A retry repeats its request's method and target
//!    (`params.name`, or `params.uri` for `resources/read`), so the unanswered
//!    rounds of that method and target are the candidates.
//!
//! Either way, one candidate is a pairing. Several are a pairing only when they
//! asked for the same inputs and issued the same state: then every candidate
//! yields the same verdict, so the choice cannot change a finding. Otherwise the
//! retry is [`Pairing::Ambiguous`] and judged by nothing, because convicting a
//! client on a guessed pairing is the defect this module exists to remove.
//!
//! Step 2 is what still judges a client that altered or dropped the state: its
//! retry no longer matches by state, but it still names its request.
//!
//! Only rounds already seen are candidates: a retry is paired with what had been
//! said when it was sent, so appending a later exchange never changes how an
//! earlier retry is judged.

use mcp_conformance_core::trace::Direction;
use serde_json::{Map, Value};

use crate::context::TraceContext;

/// The `resultType` that marks a round as incomplete.
const INPUT_REQUIRED: &str = "input_required";

/// An `InputRequiredResult` and the request it answered.
#[derive(Debug, Clone, Copy)]
pub(in crate::checks) struct Round<'a> {
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
pub(in crate::checks) struct Retry<'a> {
    pub seq: u64,
    pub id: &'a Value,
    pub method: &'a str,
    pub target: Option<&'a Value>,
    pub responses: Option<&'a Map<String, Value>>,
    pub state: Option<&'a Value>,
}

/// What a retry was paired with.
#[derive(Debug, Clone, Copy)]
pub(in crate::checks) enum Pairing<'a> {
    /// The round the retry answers.
    Round(Round<'a>),
    /// More than one round could be the one, and they would be judged differently.
    Ambiguous,
    /// No round before the retry could be the one.
    Unseen,
}

impl<'a> Pairing<'a> {
    /// The paired round, when the pairing is certain.
    pub(in crate::checks) const fn round(self) -> Option<Round<'a>> {
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
pub(in crate::checks) fn rounds<'a>(context: &'a TraceContext<'_>) -> Vec<Round<'a>> {
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
pub(in crate::checks) fn retries<'a>(context: &'a TraceContext<'_>) -> Vec<Retry<'a>> {
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
pub(in crate::checks) fn retries_with_rounds<'a>(
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

/// Whether `round` was drawn by the request `retry` repeats: the same method
/// and the same target.
fn repeats(retry: &Retry<'_>, round: &Round<'_>) -> bool {
    round.origin.2 == retry.method && round.target == retry.target
}

/// Pairs one retry against the rounds seen so far, marking the one it answers.
fn pair<'a>(retry: &Retry<'a>, seen: &mut [(Round<'a>, bool)]) -> Pairing<'a> {
    if let Some(state) = retry.state {
        let issued: Vec<usize> = (0..seen.len())
            .filter(|&index| seen[index].0.state == Some(state))
            .collect();
        if let Some(&latest) = issued.last() {
            let own: Vec<usize> = issued
                .iter()
                .copied()
                .filter(|&index| repeats(retry, &seen[index].0))
                .collect();
            if own.is_empty() {
                // The state was issued, but for another request: reused, so
                // paired with the round that issued it.
                seen[latest].1 = true;
                return Pairing::Round(seen[latest].0);
            }
            // The unanswered rounds of this request first; when a server has
            // repeated the state across one request's rounds and each is
            // answered, the latest of them.
            let open: Vec<usize> = own
                .iter()
                .copied()
                .filter(|&index| !seen[index].1)
                .collect();
            return choose(retry, seen, if open.is_empty() { &own } else { &open });
        }
    }
    let candidates: Vec<usize> = (0..seen.len())
        .filter(|&index| !seen[index].1 && repeats(retry, &seen[index].0))
        .collect();
    if candidates.is_empty() {
        return Pairing::Unseen;
    }
    choose(retry, seen, &candidates)
}

/// The pairing among `candidates` (indexes into `seen`, in trace order, not
/// empty): the latest when every candidate would be judged alike, else
/// ambiguous.
fn choose<'a>(
    retry: &Retry<'a>,
    seen: &mut [(Round<'a>, bool)],
    candidates: &[usize],
) -> Pairing<'a> {
    let latest = candidates[candidates.len() - 1];
    let same_verdict = candidates
        .iter()
        .all(|&index| interchangeable(retry, &seen[index].0, &seen[latest].0));
    if !same_verdict {
        return Pairing::Ambiguous;
    }
    seen[latest].1 = true;
    Pairing::Round(seen[latest].0)
}
