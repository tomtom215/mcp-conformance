// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the trace format.

#![allow(clippy::unwrap_used)]

use super::*;
use serde_json::json;

#[test]
fn the_size_defaults_are_the_documented_values_and_add_up() {
    assert_eq!(DEFAULT_MAX_MESSAGE_BYTES, 67_108_864, "64 MiB");
    assert_eq!(LINE_ENVELOPE_BYTES, 1_048_576, "1 MiB");
    assert_eq!(DEFAULT_MAX_LINE_BYTES, 68_157_440, "their sum, 65 MiB");
}

#[test]
fn the_event_schema_is_embedded_json() {
    let schema: serde_json::Value = serde_json::from_str(EVENT_JSON_SCHEMA).unwrap();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(
        schema["required"],
        json!(["seq", "direction", "transport", "kind"])
    );
}

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
            path: None,
            document: None,
            method: None,
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
    let back: TraceEvent = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
    assert_eq!(back, event);
}

#[test]
fn unknown_kind_is_rejected() {
    let line = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"telepathy"}"#;
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
fn http_request_method_normalizes_to_uppercase() {
    // A capturer that lowercases its method must not make a POST-gated
    // clause stop being judged.
    let line = r#"{"seq":2,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"post","headers":{}}"#;
    let event: TraceEvent = serde_json::from_str(line).unwrap();
    let EventBody::Http { method, .. } = event.body else {
        panic!("expected an http event");
    };
    assert_eq!(method.as_deref(), Some("POST"));
}

#[test]
fn http_event_without_a_method_round_trips_byte_identically() {
    // Every capture predating the field, and every response event, omits
    // it — those recordings must serialize exactly as they did before.
    let line = r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":202}"#;
    let event: TraceEvent = serde_json::from_str(line).unwrap();
    let EventBody::Http { method, .. } = &event.body else {
        panic!("expected an http event");
    };
    assert!(method.is_none());
    assert_eq!(serde_json::to_string(&event).unwrap(), line);
}

#[test]
fn an_unrecognized_event_member_is_tolerated_rather_than_rejected() {
    // The schema's extension contract, and the reason a field may be added
    // to it without stranding either side: a reader ignores members it does
    // not know, so a trace written by a newer capture still parses here (it
    // loses the unknown member on re-serialization, which is the honest
    // outcome — this build cannot vouch for what it did not understand).
    // Verified against the real pre-`method` build before `method` was
    // added: it parsed a trace carrying one and dropped it.
    let line = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","targetPath":"/mcp","headers":{}}"#;
    let event: TraceEvent = serde_json::from_str(line).unwrap();
    let EventBody::Http { method, .. } = &event.body else {
        panic!("expected an http event");
    };
    assert_eq!(method.as_deref(), Some("POST"));
    assert!(
        !serde_json::to_string(&event)
            .unwrap()
            .contains("targetPath")
    );
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
