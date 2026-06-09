// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The recorded-trace event schema.
//!
//! A *trace* is an ordered record of everything observable about one protocol session:
//! the JSON-RPC messages in both directions, plus the transport-level events real
//! requirements attach to (connections opening and closing; HTTP exchanges with their
//! status and security-relevant headers). Traces are exchanged as JSON Lines — one
//! [`TraceEvent`] object per line — because that format streams, diffs, and survives
//! truncation legibly.
//!
//! Design rules:
//!
//! - `seq` is assigned at capture time and is the only ordering authority. Validators
//!   never infer order from anything else.
//! - Timestamps are optional, informational metadata; no check may depend on them
//!   (determinism would die).
//! - Capture tooling redacts credential-bearing headers by default; the schema carries
//!   whatever the capturer recorded and adds no secrets of its own.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Which party emitted an event.
///
/// Deliberately *not* `#[non_exhaustive]`: a protocol exchange has exactly two ends,
/// so downstream code may match this exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    /// Client → server.
    ClientToServer,
    /// Server → client.
    ServerToClient,
}

/// The transport a session ran over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum TransportKind {
    /// stdio transport: newline-delimited messages over a child process's stdin/stdout.
    Stdio,
    /// Streamable HTTP transport: HTTP POST/GET with optional SSE streaming.
    StreamableHttp,
}

/// Transport lifecycle moments that checks may need to anchor to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum LifecycleEvent {
    /// The transport became usable (process spawned / connection established).
    TransportOpen,
    /// The transport closed in an orderly way.
    TransportClose,
    /// The transport failed or was torn down abnormally.
    TransportAbort,
}

/// The event-specific portion of a [`TraceEvent`], discriminated by a `kind` field on
/// the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum EventBody {
    /// A JSON-RPC message, stored as parsed JSON.
    Message {
        /// The message payload exactly as captured.
        payload: Value,
    },
    /// An HTTP-level observation (Streamable HTTP transport only).
    Http {
        /// Response status, when this event records a response.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<u16>,
        /// Headers relevant to conformance (lowercased names). Capture tooling redacts
        /// credential-bearing headers by default.
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        headers: BTreeMap<String, String>,
    },
    /// A transport lifecycle moment.
    Lifecycle {
        /// Which moment occurred.
        event: LifecycleEvent,
    },
}

/// One captured event in a protocol trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TraceEvent {
    /// Capture-assigned sequence number; the total order within the trace.
    pub seq: u64,
    /// Which party emitted the event.
    pub direction: Direction,
    /// The transport the session ran over.
    pub transport: TransportKind,
    /// The event-specific body (`kind` discriminated).
    #[serde(flatten)]
    pub body: EventBody,
    /// Optional informational timestamp (RFC 3339). Never consulted by checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

impl TraceEvent {
    /// The JSON-RPC payload, when this event is a message.
    #[must_use]
    pub const fn message_payload(&self) -> Option<&Value> {
        match &self.body {
            EventBody::Message { payload } => Some(payload),
            EventBody::Http { .. } | EventBody::Lifecycle { .. } => None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn message_event_round_trips() {
        let line = r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize"}}"#;
        let event: TraceEvent = serde_json::from_str(line).unwrap();
        assert_eq!(event.seq, 1);
        assert_eq!(event.direction, Direction::ClientToServer);
        assert_eq!(event.transport, TransportKind::Stdio);
        assert_eq!(
            event.message_payload().unwrap()["method"],
            json!("initialize")
        );

        let serialized = serde_json::to_string(&event).unwrap();
        let back: TraceEvent = serde_json::from_str(&serialized).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn http_event_round_trips_and_omits_empty_fields() {
        let event = TraceEvent {
            seq: 4,
            direction: Direction::ServerToClient,
            transport: TransportKind::StreamableHttp,
            body: EventBody::Http {
                status: Some(403),
                headers: BTreeMap::new(),
            },
            ts: None,
        };
        let serialized = serde_json::to_string(&event).unwrap();
        assert!(!serialized.contains("headers"), "{serialized}");
        assert!(!serialized.contains("ts"), "{serialized}");
        let back: TraceEvent = serde_json::from_str(&serialized).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn lifecycle_event_round_trips() {
        let line = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#;
        let event: TraceEvent = serde_json::from_str(line).unwrap();
        assert_eq!(
            event.body,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportOpen
            }
        );
        let back: TraceEvent =
            serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let line =
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"telepathy"}"#;
        assert!(serde_json::from_str::<TraceEvent>(line).is_err());
    }
}
