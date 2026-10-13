// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! MCP Inspector protocol exports as traces.
//!
//! The Inspector's web client (`@modelcontextprotocol/inspector`, read at
//! 2.10.1 from its published npm package, `clients/web/dist`) exports its
//! protocol log as `inspector-protocol-<server>-<time>.json`: a JSON array,
//! pretty-printed, of the entries its message log holds. An entry is
//!
//! ```text
//! { "id": "<uuid>", "timestamp": "<ISO 8601>", "direction": "request" | "response" | "notification",
//!   "origin": "client" | "server", "message": { …JSON-RPC… },
//!   "response": { …JSON-RPC… }, "duration": <ms>, "clientError": … }
//! ```
//!
//! `origin` is the sender. A request's answer is folded into the request's
//! own entry (`response`, with `duration` from request to answer, both read
//! from the Inspector's clock) whenever the log still held the request,
//! matched by id; a response it could not match is an entry of its own.
//!
//! Each message becomes a message event: a request at its timestamp, its
//! folded answer at the request's timestamp plus the duration, sent the
//! other way; events are ordered by those times (to the millisecond the
//! export records), and messages at the same millisecond keep the export's
//! order. The list below is what such a trace cannot hold.

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};
use serde_json::Value;

use super::time::{format_millis, parse_millis};
use super::{ImportError, Imported, write};

/// What an Inspector protocol export does not record.
pub const NOT_RECORDED: [&str; 7] = [
    "the transport: it is stated when the export is imported, not read from it",
    "HTTP requests, responses, status codes and headers: the protocol export holds JSON-RPC \
     messages only (the Inspector's separate network export is not imported)",
    "transport lifecycle (a process starting or exiting, a connection opening or failing)",
    "a folded response's own time and place in the order: it is placed at its request's \
     timestamp plus the recorded duration, to the millisecond",
    "messages the Inspector's log had already dropped: it keeps the latest 1000 by default",
    "batches: the Inspector logs each message of one separately",
    "the bytes on the wire: messages are as the Inspector's SDK client parsed them",
];

fn sender(origin: Option<&str>) -> Result<Direction, ImportError> {
    match origin {
        Some("client") => Ok(Direction::ClientToServer),
        Some("server") => Ok(Direction::ServerToClient),
        other => Err(ImportError(format!(
            "an entry's origin is {other:?}; the export says client or server"
        ))),
    }
}

const fn opposite(direction: Direction) -> Direction {
    match direction {
        Direction::ClientToServer => Direction::ServerToClient,
        Direction::ServerToClient => Direction::ClientToServer,
    }
}

/// Imports an Inspector protocol export of a session that ran over
/// `transport`.
///
/// ```
/// use mcp_conformance_cli::import::inspector;
/// use mcp_conformance_core::trace::TransportKind;
///
/// let export = r#"[
///   { "id": "a", "timestamp": "2026-10-13T10:00:00.000Z", "direction": "request", "origin": "client",
///     "message": { "jsonrpc": "2.0", "id": 0, "method": "ping" },
///     "response": { "jsonrpc": "2.0", "id": 0, "result": {} }, "duration": 7 },
///   { "id": "b", "timestamp": "2026-10-13T10:00:00.003Z", "direction": "notification", "origin": "server",
///     "message": { "jsonrpc": "2.0", "method": "notifications/message", "params": { "level": "info", "data": "x" } } }
/// ]"#;
/// let imported = inspector::import(export, TransportKind::Stdio)?;
/// let lines: Vec<&str> = imported.trace.lines().collect();
/// // The notification at .003 comes before the answer at .007.
/// assert!(lines[1].contains("notifications/message"));
/// assert!(lines[2].contains(r#""result""#) && lines[2].contains("10:00:00.007Z"));
/// # Ok::<(), mcp_conformance_cli::import::ImportError>(())
/// ```
///
/// # Errors
///
/// [`ImportError`] when the document is not a JSON array of entries, or an
/// entry lacks a message, a sender or a timestamp the format defines.
pub fn import(text: &str, transport: TransportKind) -> Result<Imported, ImportError> {
    let entries: Value =
        serde_json::from_str(text).map_err(|error| ImportError(format!("not JSON: {error}")))?;
    let entries = entries
        .as_array()
        .ok_or_else(|| ImportError("an Inspector export is a JSON array of entries".to_owned()))?;
    let mut timed: Vec<(i64, TraceEvent)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let at = |what: &str| format!("entry {} {what}", index + 1);
        let message = entry
            .get("message")
            .cloned()
            .ok_or_else(|| ImportError(at("has no message")))?;
        let direction = sender(entry.get("origin").and_then(Value::as_str))?;
        let millis = entry
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_millis)
            .ok_or_else(|| ImportError(at("has no timestamp in the form toISOString writes")))?;
        let mut event = TraceEvent::new(
            0,
            direction,
            transport,
            EventBody::Message { payload: message },
        );
        event.ts = Some(format_millis(millis));
        timed.push((millis, event));
        if let Some(response) = entry.get("response").cloned() {
            let duration = entry
                .get("duration")
                .and_then(Value::as_i64)
                .ok_or_else(|| ImportError(at("has a response but no duration")))?;
            let answered = millis.saturating_add(duration.max(0));
            let mut event = TraceEvent::new(
                0,
                opposite(direction),
                transport,
                EventBody::Message { payload: response },
            );
            event.ts = Some(format_millis(answered));
            timed.push((answered, event));
        }
    }
    // Stable: the same millisecond keeps the export's order.
    timed.sort_by_key(|(millis, _)| *millis);
    let recorded_at = timed.first().and_then(|(_, event)| event.ts.clone());
    Ok(Imported {
        trace: write(timed.into_iter().map(|(_, event)| event).collect()),
        not_recorded: NOT_RECORDED.iter().map(|gap| (*gap).to_owned()).collect(),
        recorded_at,
    })
}
