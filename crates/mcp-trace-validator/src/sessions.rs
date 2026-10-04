// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How many sessions a trace records.
//!
//! A trace is judged as one session. Two sessions recorded into one file — a
//! capture proxy left running while a client connects twice, or a client that
//! re-initializes after an HTTP 404 as `2025-11-25` requires — draw findings
//! that are artifacts of the recording rather than of either session: a
//! request id the second session legitimately reuses (`BASE-003`), a session id
//! that changed (`TRAN-013`). The validator cannot split such a trace reliably
//! (it carries no HTTP request/response correlation), but it can say that the
//! trace holds more than one, which is what the CLI does.

use std::collections::BTreeSet;

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent};
use serde_json::Value;

/// The number of sessions `events` records.
///
/// Counted by the strongest evidence available: the completed `initialize` handshakes (a client request answered by a result
/// with its id), or the distinct `Mcp-Session-Id` values the server assigned,
/// whichever is larger. `0` for a trace with neither, which includes every
/// `2026-07-28` stateless trace: that revision has no sessions to count.
#[must_use]
pub fn recorded_sessions(events: &[TraceEvent]) -> usize {
    handshakes(events).max(assigned_session_ids(events))
}

fn handshakes(events: &[TraceEvent]) -> usize {
    events
        .iter()
        .enumerate()
        .filter(|(_, event)| event.direction == Direction::ClientToServer)
        .filter_map(|(index, event)| {
            let payload = event.message_payload()?;
            if payload.get("method").and_then(Value::as_str) != Some("initialize") {
                return None;
            }
            Some((index, payload.get("id")?))
        })
        .filter(|(index, id)| {
            events[index + 1..]
                .iter()
                .filter(|later| later.direction == Direction::ServerToClient)
                .filter_map(TraceEvent::message_payload)
                .find(|later| later.get("method").is_none() && later.get("id") == Some(*id))
                .is_some_and(|response| response.get("result").is_some())
        })
        .count()
}

fn assigned_session_ids(events: &[TraceEvent]) -> usize {
    events
        .iter()
        .filter(|event| event.direction == Direction::ServerToClient)
        .filter_map(|event| match &event.body {
            EventBody::Http { headers, .. } => headers.get("mcp-session-id"),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .len()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::recorded_sessions;
    use crate::reader::{Limits, parse_trace};

    fn count(document: &str) -> usize {
        recorded_sessions(&parse_trace(document, &Limits::default()).unwrap())
    }

    const INIT: &str = r#"{"seq":S0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":S1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;

    fn handshake(first: u64) -> String {
        INIT.replace("S0", &first.to_string())
            .replace("S1", &(first + 1).to_string())
    }

    #[test]
    fn one_handshake_is_one_session_and_two_are_two() {
        assert_eq!(count(&handshake(0)), 1);
        assert_eq!(count(&format!("{}\n{}", handshake(0), handshake(2))), 2);
    }

    #[test]
    fn a_refused_initialize_is_not_a_session() {
        let refused = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":0,"error":{"code":-32602,"message":"Unsupported protocol version"}}}"#;
        assert_eq!(count(&format!("{refused}\n{}", handshake(2))), 1);
    }

    #[test]
    fn distinct_assigned_session_ids_count_as_sessions() {
        let assigned = |seq: u64, id: &str| {
            format!(
                r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{{"mcp-session-id":"{id}"}}}}"#
            )
        };
        let one = [assigned(0, "a"), assigned(1, "a")].join("\n");
        assert_eq!(count(&one), 1);
        let two = [assigned(0, "a"), assigned(1, "b")].join("\n");
        assert_eq!(count(&two), 2);
        assert_eq!(
            count(
                r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#
            ),
            0
        );
    }
}
