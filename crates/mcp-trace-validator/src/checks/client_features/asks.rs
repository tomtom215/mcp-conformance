// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The server's questions to the client, and the client's answers, in either
//! revision's shape.
//!
//! `2025-11-25` asks with a JSON-RPC request the server sends —
//! `elicitation/create`, `sampling/createMessage` or `roots/list` — answered by
//! the client's response, against the capabilities its `initialize` request
//! declared. `2026-07-28` asks inside an `input_required` result's
//! `inputRequests` map and is answered by the matching `inputResponses` entry of
//! the client's retry, against the `clientCapabilities` each request carries in
//! its own `_meta`. The client-feature pages state their rules about "elicitation
//! requests" and "sampling requests" in words both revisions share, so one model
//! of an [`Ask`] and its [`Answer`] lets one check judge one sentence wherever it
//! appears.
//!
//! A direct server-to-client request in a `2026-07-28` trace is still an ask
//! here: it is a violation of its own (`MRTR-001`), and its parameters are still
//! held to the page's rules rather than ignored because they arrived the wrong
//! way.

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

use crate::checks::stateless::mrtr::pairing::retries_with_rounds;
use crate::context::TraceContext;

/// The methods by which a server asks the client for input.
const ASK_METHODS: &[&str] = &["elicitation/create", "sampling/createMessage", "roots/list"];

/// The `_meta` key a `2026-07-28` request declares its client capabilities under.
const CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";

/// What the client had declared where an ask or an answer is judged.
#[derive(Debug, Clone, Copy)]
pub(super) enum Surface<'a> {
    /// The capabilities object the client declared.
    Declared(&'a Value),
    /// The declaration exists and declares nothing: a `2026-07-28` request
    /// whose `_meta` names no capabilities, or an `initialize` without them.
    Nothing,
    /// The trace carries no declaration at all — a `2025-11-25` recording with
    /// no `initialize` request — so nothing can be held against it (ADR-0006).
    Unknowable,
}

impl<'a> Surface<'a> {
    /// Whether anything can be known from it: `false` only for
    /// [`Surface::Unknowable`].
    pub(super) const fn knowable(self) -> bool {
        !matches!(self, Self::Unknowable)
    }

    /// The value declared at `path`; `None` when it was not declared (absent,
    /// `null` or `false`) — or when nothing can be known, which callers tell
    /// apart with [`Surface::knowable`].
    pub(super) fn at(self, path: &[&str]) -> Option<&'a Value> {
        let Self::Declared(mut current) = self else {
            return None;
        };
        for segment in path {
            current = current.get(segment)?;
        }
        (!current.is_null() && current != &Value::Bool(false)).then_some(current)
    }

    /// Whether `path` was declared; `None` when nothing can be known.
    pub(super) fn declares(self, path: &[&str]) -> Option<bool> {
        self.knowable().then(|| self.at(path).is_some())
    }
}

/// One question the server put to the client.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ask<'a> {
    /// The event carrying it: the `input_required` result, or the request.
    pub seq: u64,
    /// Its `inputRequests` key; `None` for a direct request.
    pub key: Option<&'a str>,
    /// `elicitation/create`, `sampling/createMessage` or `roots/list`.
    pub method: &'a str,
    /// Its `params`, when present.
    pub params: Option<&'a Value>,
    /// What the client had declared when it was asked.
    pub declared: Surface<'a>,
}

impl<'a> Ask<'a> {
    /// One of its parameters.
    pub(super) fn param(&self, name: &str) -> Option<&'a Value> {
        self.params?.get(name)
    }

    /// Whether it arrived inside an `input_required` result.
    pub(super) const fn is_mrtr(&self) -> bool {
        self.key.is_some()
    }

    /// What a finding calls the declaration it, or its answer, is held to.
    pub(super) const fn declaration(&self) -> &'static str {
        if self.is_mrtr() {
            "the request's `clientCapabilities`"
        } else {
            "the client's `initialize` request"
        }
    }

    /// How a finding names it.
    pub(super) fn label(&self) -> String {
        self.key.map_or_else(
            || format!("the `{}` request", self.method),
            |key| format!("`inputRequests[{key}]` (`{}`)", self.method),
        )
    }
}

/// The client's answer to one [`Ask`].
#[derive(Debug, Clone, Copy)]
pub(super) struct Answer<'a> {
    /// What was asked.
    pub ask: Ask<'a>,
    /// The event carrying the answer: the retry, or the response.
    pub seq: u64,
    /// The answer's result value; `None` for an error response.
    pub result: Option<&'a Value>,
    /// The error response's `code`, when the answer is one that carries one.
    pub error_code: Option<i64>,
    /// What the client declared where it answered: the retry's own
    /// `clientCapabilities`, or its `initialize` capabilities.
    pub declared: Surface<'a>,
}

/// The `clientCapabilities` a `2026-07-28` request declares.
fn request_surface(request: &TraceEvent) -> Surface<'_> {
    request
        .message_payload()
        .and_then(|payload| payload.get("params"))
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get(CLIENT_CAPABILITIES))
        .map_or(Surface::Nothing, Surface::Declared)
}

/// The capabilities the client's `initialize` request declared.
fn session_surface<'a>(context: &TraceContext<'a>) -> Surface<'a> {
    match context.initialize().request {
        None => Surface::Unknowable,
        Some(_) => context
            .client_capabilities()
            .map_or(Surface::Nothing, Surface::Declared),
    }
}

/// The event with `seq`, when the trace has one.
fn event_at<'a>(context: &TraceContext<'a>, seq: u64) -> Option<&'a TraceEvent> {
    let events = context.events();
    events
        .binary_search_by_key(&seq, |event| event.seq)
        .ok()
        .map(|index| &events[index])
}

/// The asks an `input_required` result carries, judged against `declared`.
fn mrtr_asks<'a>(result: &'a Value, seq: u64, declared: Surface<'a>) -> Vec<Ask<'a>> {
    result
        .get("inputRequests")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, request)| {
            let method = request.get("method").and_then(Value::as_str)?;
            ASK_METHODS.contains(&method).then_some(Ask {
                seq,
                key: Some(key.as_str()),
                method,
                params: request.get("params"),
                declared,
            })
        })
        .collect()
}

/// Whether `result` is an `InputRequiredResult`.
fn is_input_required(result: &Value) -> bool {
    result.get("resultType").and_then(Value::as_str) == Some("input_required")
}

/// Every ask in the trace, in event order: each `input_required` result's input
/// requests, and each request the server sent for one of the three methods.
pub(super) fn asks<'a>(context: &TraceContext<'a>) -> Vec<Ask<'a>> {
    let session = session_surface(context);
    let mut asks: Vec<Ask<'a>> = context
        .exchanges()
        .filter(|exchange| exchange.request.direction == Direction::ClientToServer)
        .filter_map(|exchange| {
            let result = exchange.result.filter(|result| is_input_required(result))?;
            Some(mrtr_asks(
                result,
                exchange.response.seq,
                request_surface(exchange.request),
            ))
        })
        .flatten()
        .collect();
    asks.extend(context.messages().filter_map(|(event, kind, _)| {
        let &MessageKind::Request { method, .. } = kind else {
            return None;
        };
        if event.direction != Direction::ServerToClient || !ASK_METHODS.contains(&method) {
            return None;
        }
        Some(Ask {
            seq: event.seq,
            key: None,
            method,
            params: event.message_payload()?.get("params"),
            declared: session,
        })
    }));
    asks.sort_by_key(|ask| ask.seq);
    asks
}

/// Every answer in the trace, in event order: each `inputResponses` entry of a
/// retry for an input its round asked for, and each response the client sent
/// to a server request for one of the three methods.
///
/// A retry is matched to its round as the MRTR checks match it
/// (`mrtr::pairing`); one whose round is unseen or ambiguous answers nothing
/// here, because judging an answer needs to know what was asked.
pub(super) fn answers<'a>(context: &'a TraceContext<'a>) -> Vec<Answer<'a>> {
    let session = session_surface(context);
    let mut answers: Vec<Answer<'a>> = Vec::new();
    for (retry, pairing) in retries_with_rounds(context) {
        let Some(round) = pairing.round() else {
            continue;
        };
        let (Some(responses), Some(round_result)) = (
            retry.responses,
            event_at(context, round.seq)
                .and_then(TraceEvent::message_payload)
                .and_then(|payload| payload.get("result")),
        ) else {
            continue;
        };
        let asked = event_at(context, round.origin.0).map_or(Surface::Nothing, request_surface);
        let answering = event_at(context, retry.seq).map_or(Surface::Nothing, request_surface);
        for ask in mrtr_asks(round_result, round.seq, asked) {
            let Some(value) = ask.key.and_then(|key| responses.get(key)) else {
                continue;
            };
            answers.push(Answer {
                ask,
                seq: retry.seq,
                result: Some(value),
                error_code: None,
                declared: answering,
            });
        }
    }
    for exchange in context.exchanges() {
        if exchange.request.direction != Direction::ServerToClient
            || !ASK_METHODS.contains(&exchange.method)
        {
            continue;
        }
        let error_code = exchange
            .response
            .message_payload()
            .and_then(|payload| payload.get("error"))
            .and_then(|error| error.get("code"))
            .and_then(Value::as_i64);
        answers.push(Answer {
            ask: Ask {
                seq: exchange.request.seq,
                key: None,
                method: exchange.method,
                params: exchange.params,
                declared: session,
            },
            seq: exchange.response.seq,
            result: exchange.result,
            error_code,
            declared: session,
        });
    }
    answers.sort_by_key(|answer| answer.seq);
    answers
}

/// The asks for one method.
pub(super) fn asks_for<'a>(context: &TraceContext<'a>, method: &str) -> Vec<Ask<'a>> {
    asks(context)
        .into_iter()
        .filter(|ask| ask.method == method)
        .collect()
}

/// The answers to asks for one method.
pub(super) fn answers_for<'a>(context: &'a TraceContext<'a>, method: &str) -> Vec<Answer<'a>> {
    answers(context)
        .into_iter()
        .filter(|answer| answer.ask.method == method)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{answers, asks};
    use crate::checks::client_features::testkit::{ask, client, reply, server, session};
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    #[test]
    fn only_server_requests_for_the_ask_methods_are_asks_and_answers() {
        // A client request for an ask method (answered by the server), and a
        // server request for another method (answered by the client), are
        // neither asks nor answers; only the server's `roots/list` is.
        let trace = session(
            r#"{"roots":{}}"#,
            &[
                client(
                    3,
                    r#"{"jsonrpc":"2.0","id":5,"method":"sampling/createMessage","params":{}}"#,
                ),
                server(4, r#"{"jsonrpc":"2.0","id":5,"result":{}}"#),
                ask(5, 1, "ping", "{}"),
                reply(6, 1, "{}"),
                ask(7, 2, "roots/list", "{}"),
                reply(8, 2, r#"{"roots":[]}"#),
            ],
        );
        let events = parse_trace(&trace, &Limits::default()).expect("test trace parses");
        let context = TraceContext::new(&events);
        let asked: Vec<(u64, &str)> = asks(&context)
            .iter()
            .map(|ask| (ask.seq, ask.method))
            .collect();
        assert_eq!(asked, [(7, "roots/list")]);
        let answered: Vec<(u64, u64, &str)> = answers(&context)
            .iter()
            .map(|answer| (answer.ask.seq, answer.seq, answer.ask.method))
            .collect();
        assert_eq!(answered, [(7, 8, "roots/list")]);
    }
}
