// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The exerciser's two transports behind one interface: a message out, the
//! answer back, within the run's bounds.
//!
//! Both reach the server *through the recorder* — the stdio wrapper's relay, or
//! the HTTP proxy — so the trace is the recorder's account of the bytes that
//! crossed, not this client's account of what it meant to send (ADR-0024,
//! decision 1).

use std::time::Duration;

use serde_json::Value;

use super::request::{Outgoing, Requests};

pub(crate) mod http;
pub(crate) mod stdio;

/// The bounds one exchange runs under.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// The longest wait for an answer.
    pub(crate) timeout: Duration,
    /// The most bytes read for one answer.
    pub(crate) max_response: usize,
}

/// Why an exchange produced no JSON-RPC answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// No answer within [`Limits::timeout`].
    Timeout,
    /// The server ended the session: it exited, or closed the stream.
    Closed,
    /// An HTTP response that is not a JSON-RPC message.
    Http(u16),
    /// The answer passed [`Limits::max_response`].
    TooLarge,
    /// The transport failed; the text says how.
    Transport(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => f.write_str("no answer within the time allowed (--timeout)"),
            Self::Closed => f.write_str("the server ended the session without answering"),
            Self::Http(status) => write!(
                f,
                "the server answered HTTP {status} with a body that is not a JSON-RPC message"
            ),
            Self::TooLarge => f.write_str("the answer is larger than --max-response-bytes allows"),
            Self::Transport(error) => f.write_str(error),
        }
    }
}

/// A JSON-RPC answer, and what else the exchange showed.
#[derive(Debug, Clone)]
pub(crate) struct Answer {
    /// The response message: it has the request's `id` and a `result` or an
    /// `error`.
    pub(crate) message: Value,
    /// The header value of `Mcp-Session-Id` on the response, if any.
    pub(crate) session: Option<String>,
    /// How many requests of its own the server sent before answering.
    pub(crate) server_requests: usize,
}

/// A connection to the server under test.
#[derive(Debug)]
pub(crate) enum Wire {
    /// A server process, under the recorder's stdio wrapper.
    Stdio(Box<stdio::Stdio>),
    /// A Streamable HTTP endpoint, behind the recorder's proxy.
    Http(Box<http::Http>),
}

impl Wire {
    /// Sends the request `out` and waits for its answer, answering any
    /// request the server sends meanwhile with `requests`' refusal.
    pub(crate) async fn exchange(
        &mut self,
        out: &Outgoing,
        requests: &Requests,
        limits: Limits,
    ) -> Result<Answer, Failure> {
        let exchange = async {
            match self {
                Self::Stdio(stdio) => stdio.exchange(out, requests, limits.max_response).await,
                Self::Http(http) => http.exchange(out, requests, limits.max_response).await,
            }
        };
        tokio::time::timeout(limits.timeout, exchange)
            .await
            .unwrap_or(Err(Failure::Timeout))
    }

    /// Sends the notification `out`.
    pub(crate) async fn notify(&mut self, out: &Outgoing, limits: Limits) -> Result<(), Failure> {
        let sending = async {
            match self {
                Self::Stdio(stdio) => stdio.send(&out.body).await,
                Self::Http(http) => http.notify(out).await,
            }
        };
        tokio::time::timeout(limits.timeout, sending)
            .await
            .unwrap_or(Err(Failure::Timeout))
    }

    /// Ends the session as a client does and closes the trace.
    ///
    /// # Errors
    ///
    /// The trace is incomplete: a write to it failed.
    pub(crate) async fn close(self) -> Result<(), String> {
        match self {
            Self::Stdio(stdio) => stdio.close().await,
            Self::Http(http) => http.close().await,
        }
    }
}

/// What a message read from the server is, to an exchange waiting on `id`.
pub(crate) enum Incoming {
    /// The answer to the request.
    Answer(Value),
    /// A request of the server's own, to be answered.
    Request(Value),
    /// Anything else: a notification, an answer to another id, or not JSON-RPC
    /// at all. The recorder has it; the exchange goes on waiting.
    Other,
}

/// Classifies `message` for an exchange waiting on `id`.
pub(crate) fn classify(message: Value, id: Option<u64>) -> Incoming {
    let is_request = message.get("method").is_some() && message.get("id").is_some();
    let answers = message.get("result").is_some() || message.get("error").is_some();
    if is_request {
        Incoming::Request(message)
    } else if answers && id.is_some() && message.get("id").and_then(Value::as_u64) == id {
        Incoming::Answer(message)
    } else {
        Incoming::Other
    }
}

#[cfg(test)]
mod tests;
