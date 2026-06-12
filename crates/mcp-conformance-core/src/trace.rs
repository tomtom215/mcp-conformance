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

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Deserializes an HTTP-headers map with field names normalized to lowercase.
///
/// HTTP field names are case-insensitive (RFC 9110 §5.1); the checks compare
/// against lowercase names, so normalizing here keeps a trace's casing from
/// changing a verdict. If two differently-cased spellings of one field appear
/// (a malformed trace), they collapse to one lowercase key — last-in-sorted-
/// order wins, which is deterministic; conflicting duplicates are themselves a
/// capture defect.
fn deserialize_lowercase_header_keys<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = BTreeMap::<String, String>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .collect())
}

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
        /// Headers relevant to conformance. HTTP field names are
        /// case-insensitive (RFC 9110 §5.1), so keys are normalized to
        /// lowercase on deserialization — the contract the transport checks
        /// rely on when they look a header up by its lowercase name. A trace
        /// recording on-the-wire casing (`Mcp-Session-Id`) is therefore judged
        /// the same as one already lowercased; a lossy capture cannot hide a
        /// bad header behind its casing. Capture tooling redacts
        /// credential-bearing headers by default.
        #[serde(
            default,
            skip_serializing_if = "BTreeMap::is_empty",
            deserialize_with = "deserialize_lowercase_header_keys"
        )]
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
    /// Builds an event with no timestamp — the constructor capture tooling
    /// uses (the struct is `#[non_exhaustive]`, so literals only work inside
    /// this crate). `ts` stays settable afterwards; checks never read it.
    #[must_use]
    pub const fn new(
        seq: u64,
        direction: Direction,
        transport: TransportKind,
        body: EventBody,
    ) -> Self {
        Self {
            seq,
            direction,
            transport,
            body,
            ts: None,
        }
    }

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
    fn new_builds_the_exact_event_with_no_timestamp() {
        let event = TraceEvent::new(
            7,
            Direction::ServerToClient,
            TransportKind::StreamableHttp,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose,
            },
        );
        assert_eq!(event.seq, 7);
        assert_eq!(event.direction, Direction::ServerToClient);
        assert_eq!(event.transport, TransportKind::StreamableHttp);
        assert_eq!(
            event.body,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose
            }
        );
        assert_eq!(event.ts, None, "checks never read ts; new() never sets it");
    }

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
        let error = serde_json::from_str::<TraceEvent>(line).unwrap_err();
        assert!(
            error.to_string().contains("telepathy"),
            "the rejection names the unknown kind so the trace author can find it: {error}"
        );
    }

    #[test]
    fn http_header_keys_are_lowercased_on_deserialization() {
        // On-the-wire casing must normalize so a check looking a header up by
        // its lowercase name cannot be fooled by a capitalized capture.
        let line = r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{"Mcp-Session-Id":"abc","Content-Type":"application/json"}}"#;
        let event: TraceEvent = serde_json::from_str(line).unwrap();
        let EventBody::Http { headers, .. } = event.body else {
            panic!("expected an http event");
        };
        assert_eq!(
            headers.get("mcp-session-id").map(String::as_str),
            Some("abc")
        );
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        // The capitalized spellings are gone, not merely duplicated.
        assert!(!headers.contains_key("Mcp-Session-Id"));
        assert_eq!(headers.len(), 2);
    }

    #[test]
    fn header_values_are_preserved_verbatim_only_keys_normalize() {
        // Only field NAMES are case-insensitive; values are untouched.
        let line = r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","headers":{"ACCEPT":"Application/JSON, Text/Event-Stream"}}"#;
        let event: TraceEvent = serde_json::from_str(line).unwrap();
        let EventBody::Http { headers, .. } = event.body else {
            panic!("expected an http event");
        };
        assert_eq!(
            headers.get("accept").map(String::as_str),
            Some("Application/JSON, Text/Event-Stream")
        );
    }
}
