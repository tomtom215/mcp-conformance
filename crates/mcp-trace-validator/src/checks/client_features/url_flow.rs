// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `2025-11-25`'s URL mode bookkeeping: `elicitationId`, the completion
//! notification that names it, and `URLElicitationRequiredError` (`-32042`).
//!
//! `2026-07-28` removed all three — a URL mode round is resumed by the client's
//! retry and the state it echoes — so these checks find nothing to judge in a
//! trace of that revision, and its registry names none of them.

use std::collections::BTreeSet;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::super::FindingSink;
use super::asks::asks_for;
use super::elicitation::Mode;
use crate::context::TraceContext;

/// `URLElicitationRequiredError`'s code.
const URL_ELICITATION_REQUIRED: i64 = -32042;

/// The completion notification's method.
const COMPLETE: &str = "notifications/elicitation/complete";

/// Each `-32042` error the server sent: its `seq` and its `data`, if any.
fn url_required_errors<'a>(context: &'a TraceContext<'_>) -> Vec<(u64, Option<&'a Value>)> {
    context
        .messages()
        .filter(|(event, _, _)| event.direction == Direction::ServerToClient)
        .filter_map(|(event, kind, _)| {
            let MessageKind::Error { error, .. } = kind else {
                return None;
            };
            (error.get("code").and_then(Value::as_i64) == Some(URL_ELICITATION_REQUIRED))
                .then(|| (event.seq, error.get("data")))
        })
        .collect()
}

/// One event of the URL mode bookkeeping.
#[derive(Debug, Clone, Copy)]
enum Bookkeeping<'a> {
    /// A URL mode elicitation, or a `-32042` listing one, established this id.
    Establishes(&'a str),
    /// A completion notification named this id (`None`: it named none).
    Completes(Option<&'a str>),
}

/// The elicitations a `-32042` error's `data` lists, when it lists any.
fn listed(data: Option<&Value>) -> Option<&Vec<Value>> {
    data?.get("elicitations")?.as_array()
}

/// "Servers sending notifications: MUST include the `elicitationId` established
/// in the original `elicitation/create` request."
///
/// Established means sent earlier in the session: by a URL mode
/// `elicitation/create`, or inside a `-32042` error, whose elicitations the
/// page's own flow completes the same way. A `2025-11-25` recording starts at
/// `initialize`, so an id the session never carried was never established for
/// this client.
pub(in crate::checks) fn complete_notification_id(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Every event that establishes an id or names one, walked in trace order
    // so "established" can only mean "earlier".
    let mut events: Vec<(u64, Bookkeeping<'_>)> = asks_for(context, "elicitation/create")
        .into_iter()
        .filter(|ask| Mode::of(ask) == Mode::Url)
        .filter_map(|ask| {
            let id = ask.param("elicitationId")?.as_str()?;
            Some((ask.seq, Bookkeeping::Establishes(id)))
        })
        .collect();
    for (seq, data) in url_required_errors(context) {
        for elicitation in listed(data).into_iter().flatten() {
            if let Some(id) = elicitation.get("elicitationId").and_then(Value::as_str) {
                events.push((seq, Bookkeeping::Establishes(id)));
            }
        }
    }
    for (event, kind, _) in context.messages() {
        if event.direction == Direction::ServerToClient
            && matches!(kind, MessageKind::Notification { method } if *method == COMPLETE)
        {
            let id = event
                .message_payload()
                .and_then(|payload| payload.get("params"))
                .and_then(|params| params.get("elicitationId"))
                .and_then(Value::as_str);
            events.push((event.seq, Bookkeeping::Completes(id)));
        }
    }
    events.sort_by_key(|(seq, _)| *seq);
    let mut established: BTreeSet<&str> = BTreeSet::new();
    for (seq, item) in events {
        let id = match item {
            Bookkeeping::Establishes(id) => {
                established.insert(id);
                continue;
            }
            Bookkeeping::Completes(id) => id,
        };
        sink.examined();
        let Some(id) = id else {
            sink.push(
                Some(seq),
                "the completion notification carries no string `elicitationId`".to_owned(),
            );
            continue;
        };
        if !established.contains(id) {
            sink.push(
                Some(seq),
                format!(
                    "the completion notification names `elicitationId` {id:?}, which no URL mode \
                     elicitation sent earlier in this session established"
                ),
            );
        }
    }
}

/// "The error MUST include a list of elicitations that are required to complete
/// before the original can be retried."
///
/// A list that is empty names nothing to complete, so it is not one.
pub(in crate::checks) fn url_required_error_lists(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, data) in url_required_errors(context) {
        sink.examined();
        if listed(data).is_none_or(Vec::is_empty) {
            sink.push(
                Some(seq),
                "the `-32042` error's `data` carries no non-empty `elicitations` list".to_owned(),
            );
        }
    }
}

/// "Any elicitations returned in the error MUST be URL mode elicitations and
/// have an `elicitationId` property."
pub(in crate::checks) fn url_required_error_url_mode(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, data) in url_required_errors(context) {
        for (index, elicitation) in listed(data).into_iter().flatten().enumerate() {
            sink.examined();
            let url_mode = elicitation.get("mode").and_then(Value::as_str) == Some("url");
            let has_id = elicitation.get("elicitationId").is_some();
            if !url_mode || !has_id {
                let mut wrong = Vec::new();
                if !url_mode {
                    wrong.push("is not `mode: \"url\"`");
                }
                if !has_id {
                    wrong.push("has no `elicitationId`");
                }
                sink.push(
                    Some(seq),
                    format!(
                        "the `-32042` error's `elicitations[{index}]` {}",
                        wrong.join(" and ")
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
