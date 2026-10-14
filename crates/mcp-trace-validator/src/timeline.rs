// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How a trace's messages read on a timeline: what each one is, which request
//! a response answers, and how much of a payload a view shows.
//!
//! The HTML report's message timeline ([`crate::html`]) is drawn from these,
//! and they are public so that another view of the same trace — the
//! `mcp-conformance serve` workbench — reads every message exactly as the
//! report does rather than by a second set of rules.
//!
//! ```
//! use mcp_trace_validator::timeline::{self, Shape};
//! use serde_json::json;
//!
//! assert_eq!(
//!     timeline::shape(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})),
//!     Shape::Request { method: "ping".into(), id: "1".into() },
//! );
//! ```

use std::collections::{BTreeMap, HashMap};

use mcp_conformance_core::trace::{Direction, TraceEvent};
use serde_json::Value;

/// The most of a payload shown, in bytes of its pretty-printed JSON.
///
/// A recording can carry a 64 MiB tool result; a view shows the start of it
/// and says how much more the trace line holds, rather than becoming the size
/// of the recording several times over.
pub const PAYLOAD_PREVIEW_BYTES: usize = 16 * 1024;

/// What a message is, read from its members.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Shape {
    /// A method and an id.
    Request {
        /// The method name, or the JSON of whatever a malformed message put there.
        method: String,
        /// The id, as one line of JSON (`1`, `"a"`).
        id: String,
    },
    /// A method and no id.
    Notification {
        /// The method name, or the JSON of whatever a malformed message put there.
        method: String,
    },
    /// An id and a result.
    Result {
        /// The id, as one line of JSON.
        id: String,
    },
    /// An id and an error.
    Error {
        /// The id, as one line of JSON.
        id: String,
        /// `error <code>`, or `error` when the error carries no code.
        error: String,
    },
    /// A JSON array of messages, with its length.
    Batch(usize),
    /// None of the above: a malformed message.
    Other,
}

/// `value` as one line of JSON, for ids and error codes.
fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// A method name as text: the string itself, or the JSON of whatever else a
/// malformed message put there.
fn method_text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| compact(value), ToOwned::to_owned)
}

/// What `payload` is.
#[must_use]
pub fn shape(payload: &Value) -> Shape {
    let Some(message) = payload.as_object() else {
        return payload
            .as_array()
            .map_or(Shape::Other, |batch| Shape::Batch(batch.len()));
    };
    let id = message.get("id").map(compact);
    match (message.get("method"), id) {
        (Some(method), Some(id)) => Shape::Request {
            method: method_text(method),
            id,
        },
        (Some(method), None) => Shape::Notification {
            method: method_text(method),
        },
        (None, Some(id)) => match message.get("error") {
            Some(error) => Shape::Error {
                id,
                error: error.get("code").map_or_else(
                    || "error".to_owned(),
                    |code| format!("error {}", compact(code)),
                ),
            },
            None if message.contains_key("result") => Shape::Result { id },
            None => Shape::Other,
        },
        (None, None) => Shape::Other,
    }
}

/// Requests paired with the responses that answer them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pairs {
    /// A request's `seq` → the `seq` of its response.
    pub answered_by: BTreeMap<u64, u64>,
    /// A response's `seq` → its request's `seq` and method.
    pub answers: BTreeMap<u64, (u64, String)>,
}

/// Pairs each response with the request it answers.
///
/// A response pairs with the latest unanswered request with its id, sent
/// the other way. A response to an id nobody asked
/// for, or a second answer to one, pairs with nothing — the validator's
/// findings say what is wrong with it; a timeline only draws what is there.
#[must_use]
pub fn pair(events: &[TraceEvent]) -> Pairs {
    let mut open: HashMap<(Direction, String), (u64, String)> = HashMap::new();
    let mut pairs = Pairs::default();
    for event in events {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        match shape(payload) {
            Shape::Request { method, id } => {
                open.insert((event.direction, id), (event.seq, method));
            }
            Shape::Result { id } | Shape::Error { id, .. } => {
                let asker = match event.direction {
                    Direction::ClientToServer => Direction::ServerToClient,
                    Direction::ServerToClient => Direction::ClientToServer,
                };
                if let Some((request, method)) = open.remove(&(asker, id)) {
                    pairs.answered_by.insert(request, event.seq);
                    pairs.answers.insert(event.seq, (request, method));
                }
            }
            Shape::Notification { .. } | Shape::Batch(_) | Shape::Other => {}
        }
    }
    pairs
}

/// The start of `text` a view shows — at most [`PAYLOAD_PREVIEW_BYTES`], cut
/// at a character boundary — and how many bytes it leaves out.
#[must_use]
pub fn preview(text: &str) -> (&str, usize) {
    // The last character boundary at or before the limit: a bounded search
    // that a boundary always ends within four bytes (0 is one too).
    let end = (0..=PAYLOAD_PREVIEW_BYTES.min(text.len()))
        .rev()
        .find(|&end| text.is_char_boundary(end))
        .unwrap_or_default();
    (&text[..end], text.len() - end)
}

#[cfg(test)]
mod tests;
