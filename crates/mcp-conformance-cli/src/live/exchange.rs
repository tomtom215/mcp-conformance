// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Exchanges as the wire completes them: a request paired with its response,
//! or a notification on its own.
//!
//! This is what `watch` re-judges after, and what `--otlp` makes a span of.
//! Pairing is by direction and id, exactly as JSON-RPC pairs them: a response
//! answers the earliest unanswered request of its id sent the other way. A
//! response nothing asked for completes no exchange — but it is still a
//! reason to judge, since the validator has something to say about it.

use std::collections::HashMap;
use std::time::SystemTime;

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};
use serde_json::Value;

/// How an exchange ended.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ending {
    /// A notification: complete as sent.
    Notification,
    /// The response arrived; `error` is its JSON-RPC error, if it was one.
    Answered {
        /// The error's `code` (as written, for an attribute) and `message`,
        /// each when present.
        error: Option<(Option<String>, Option<String>)>,
        /// A `tools/call` result with `isError: true`.
        tool_error: bool,
    },
    /// The recording ended first.
    Unanswered,
}

/// One completed exchange.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Exchange {
    /// The method.
    pub method: String,
    /// Who sent the request or notification.
    pub initiator: Direction,
    /// The request's id, as JSON text (`1`, `"a"`); `None` for a notification.
    pub id: Option<String>,
    /// The request's (or notification's) `seq`.
    pub request_seq: u64,
    /// The response's `seq`.
    pub response_seq: Option<u64>,
    /// When the request was recorded.
    pub started: SystemTime,
    /// When the response was recorded (the request's time for a notification;
    /// the recording's end for an unanswered request).
    pub ended: SystemTime,
    /// How it ended.
    pub ending: Ending,
    /// `params.name` of a `tools/call`.
    pub tool: Option<String>,
    /// `params.name` of a `prompts/get`.
    pub prompt: Option<String>,
    /// `params.uri` of a request or notification about one resource.
    pub resource_uri: Option<String>,
    /// The `jsonrpc` member, when it is not `"2.0"`.
    pub jsonrpc: Option<String>,
    /// The transport.
    pub transport: TransportKind,
    /// The session's `Mcp-Session-Id` when last seen (HTTP).
    pub session_id: Option<String>,
    /// The protocol revision the session negotiated or declared, when seen.
    pub protocol_version: Option<String>,
}

impl Exchange {
    /// Whether `seq` is this exchange's request or response.
    #[must_use]
    pub fn holds(&self, seq: u64) -> bool {
        self.request_seq == seq || self.response_seq == Some(seq)
    }
}

/// What one recorded event did.
#[derive(Debug, Default)]
pub(super) struct Step {
    /// The event completed exchanges (one, or several in a batch).
    pub(super) completed: Vec<Exchange>,
    /// The event is a reason to judge again: anything but an unanswered
    /// request, a transport event or a lifecycle event.
    pub(super) due: bool,
}

/// Pairs requests with responses as events arrive.
#[derive(Debug, Default)]
pub(super) struct Pairing {
    open: HashMap<(Direction, String), Vec<Exchange>>,
    session_id: Option<String>,
    protocol_version: Option<String>,
}

impl Pairing {
    /// Takes in one event, recorded at `at`.
    pub(super) fn push(&mut self, event: &TraceEvent, at: SystemTime) -> Step {
        let mut step = Step::default();
        match &event.body {
            EventBody::Message { payload } => match payload {
                Value::Array(batch) => {
                    for message in batch {
                        self.message(event, message, at, &mut step);
                    }
                }
                message => self.message(event, message, at, &mut step),
            },
            EventBody::Http { headers, .. } => {
                if let Some(id) = headers.get("mcp-session-id") {
                    self.session_id = Some(id.clone());
                }
                if let Some(version) = headers.get("mcp-protocol-version") {
                    self.protocol_version = Some(version.clone());
                }
            }
            _ => {}
        }
        step
    }

    fn message(&mut self, event: &TraceEvent, message: &Value, at: SystemTime, step: &mut Step) {
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").map(Value::to_string);
        match (method, id) {
            (Some(method), id) => {
                let exchange = self.begin(event, message, method, id.clone(), at);
                if let Some(id) = id {
                    self.open
                        .entry((event.direction, id))
                        .or_default()
                        .push(exchange);
                } else {
                    step.completed.push(exchange);
                    step.due = true;
                }
            }
            (None, Some(id)) => {
                step.due = true;
                let asker = other(event.direction);
                let Some(waiting) = self.open.get_mut(&(asker, id.clone())) else {
                    return;
                };
                if waiting.is_empty() {
                    return;
                }
                let mut exchange = waiting.remove(0);
                if waiting.is_empty() {
                    self.open.remove(&(asker, id));
                }
                if exchange.method == "initialize"
                    && let Some(version) = message
                        .pointer("/result/protocolVersion")
                        .and_then(Value::as_str)
                {
                    self.protocol_version = Some(version.to_owned());
                }
                exchange.response_seq = Some(event.seq);
                exchange.ended = at;
                exchange.ending = answered(message);
                exchange.session_id.clone_from(&self.session_id);
                exchange.protocol_version.clone_from(&self.protocol_version);
                step.completed.push(exchange);
            }
            (None, None) => step.due = true,
        }
    }

    fn begin(
        &mut self,
        event: &TraceEvent,
        message: &Value,
        method: &str,
        id: Option<String>,
        at: SystemTime,
    ) -> Exchange {
        let param = |name: &str| {
            message
                .pointer(&format!("/params/{name}"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        if method == "initialize"
            && let Some(version) = param("protocolVersion")
        {
            self.protocol_version.get_or_insert(version);
        }
        let ending = if id.is_some() {
            Ending::Unanswered
        } else {
            Ending::Notification
        };
        Exchange {
            method: method.to_owned(),
            initiator: event.direction,
            id,
            request_seq: event.seq,
            response_seq: None,
            started: at,
            ended: at,
            ending,
            tool: (method == "tools/call").then(|| param("name")).flatten(),
            prompt: (method == "prompts/get").then(|| param("name")).flatten(),
            resource_uri: matches!(
                method,
                "resources/read"
                    | "resources/subscribe"
                    | "resources/unsubscribe"
                    | "notifications/resources/updated"
            )
            .then(|| param("uri"))
            .flatten(),
            jsonrpc: match message.get("jsonrpc") {
                Some(Value::String(version)) if version == "2.0" => None,
                Some(other) => Some(
                    other
                        .as_str()
                        .map_or_else(|| other.to_string(), str::to_owned),
                ),
                None => None,
            },
            transport: event.transport,
            session_id: self.session_id.clone(),
            protocol_version: self.protocol_version.clone(),
        }
    }

    /// The requests still waiting, in the order they were sent, each ended at
    /// `at`; the pairing is empty afterwards.
    pub(super) fn unanswered(&mut self, at: SystemTime) -> Vec<Exchange> {
        let mut left: Vec<Exchange> = self.open.drain().flat_map(|(_, waiting)| waiting).collect();
        left.sort_by_key(|exchange| exchange.request_seq);
        for exchange in &mut left {
            exchange.ended = at.max(exchange.started);
        }
        left
    }
}

const fn other(direction: Direction) -> Direction {
    match direction {
        Direction::ClientToServer => Direction::ServerToClient,
        Direction::ServerToClient => Direction::ClientToServer,
    }
}

fn answered(response: &Value) -> Ending {
    let error = response.get("error").map(|error| {
        let code = error.get("code").map(Value::to_string);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned);
        (code, message)
    });
    let tool_error = response.pointer("/result/isError") == Some(&Value::Bool(true));
    Ending::Answered { error, tool_error }
}
