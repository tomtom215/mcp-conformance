// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The importers against the formats' own documents: mcpsnoop's captures of
//! sessions this toolkit recorded at the same time, and Inspector exports
//! written to the shape its web client's code gives them.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};
use mcp_trace_validator::reader::{Limits, parse_trace};
use serde_json::{Value, json};

use super::*;

fn native(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/mcpsnoop/native")
        .join(name);
    std::fs::read_to_string(path).unwrap()
}

fn events(trace: &str) -> Vec<TraceEvent> {
    parse_trace(trace, &Limits::default()).unwrap()
}

fn messages(events: &[TraceEvent]) -> Vec<(Direction, Value)> {
    events
        .iter()
        .filter_map(|event| {
            event
                .message_payload()
                .map(|p| (event.direction, p.clone()))
        })
        .collect()
}

#[test]
fn formats_are_detected_by_their_first_record() {
    assert_eq!(
        detect(&native("stdio-2025-11-25.mcpsnoop.jsonl")),
        Some(Format::Mcpsnoop)
    );
    assert_eq!(
        detect(&native("stdio-2025-11-25.trace.jsonl")),
        Some(Format::Trace)
    );
    assert_eq!(
        detect(r#"  [{"origin":"client","message":{},"direction":"request"}]"#),
        Some(Format::Inspector)
    );
    for other in [
        "",
        "[]",
        "[1]",
        "[{\"x\":1}]",
        r#"[{"origin":"client","direction":"request"}]"#,
        r#"[{"message":{},"direction":"request"}]"#,
        r#"{"session_id":"s","ts":"2026-10-13T10:00:00Z"}"#,
        r#"{"direction":"c2s","ts":"2026-10-13T10:00:00Z"}"#,
        r#"{"seq":0,"direction":"client-to-server"}"#,
        r#"{"kind":"message","direction":"client-to-server"}"#,
        "{\"a\":1}",
        "not json",
        "[not json",
    ] {
        assert_eq!(detect(other), None, "{other}");
    }
    for format in [Format::Trace, Format::Mcpsnoop, Format::Inspector] {
        assert_eq!(Format::named(format.name()), Some(format));
    }
    assert_eq!(Format::named("pcap"), None);
}

#[test]
fn a_trace_imports_as_itself() {
    let text = native("stdio-2025-11-25.trace.jsonl");
    let imported = import(&text, Format::Trace, &Options::default()).unwrap();
    assert_eq!(imported.trace, text);
    assert_eq!(imported.not_recorded.len(), 0);
    let error = import("{}", Format::Trace, &Options::default()).unwrap_err();
    assert!(error.to_string().starts_with("not a trace"), "{error}");
}

#[test]
fn a_stdio_capture_imports_to_the_same_messages_the_toolkit_recorded() {
    let imported = mcpsnoop::import(&native("stdio-2025-11-25.mcpsnoop.jsonl")).unwrap();
    let ours = events(&native("stdio-2025-11-25.trace.jsonl"));
    let theirs = events(&imported.trace);
    assert_eq!(messages(&theirs), messages(&ours));
    assert!(
        theirs
            .iter()
            .all(|e| e.transport == TransportKind::Stdio && e.ts.is_some())
    );
    // Ours has its two lifecycle events; mcpsnoop records none, and none is made up.
    assert!(
        theirs
            .iter()
            .all(|e| matches!(e.body, EventBody::Message { .. }))
    );
    assert_eq!(imported.not_recorded.len(), mcpsnoop::NOT_RECORDED.len());
    assert_eq!(
        imported.recorded_at.as_deref(),
        Some("2026-10-13T10:54:11.561495437Z")
    );
}

/// The HTTP request events of a trace: method and headers, in order.
fn requests(
    events: &[TraceEvent],
) -> Vec<(Option<String>, std::collections::BTreeMap<String, String>)> {
    events
        .iter()
        .filter(|event| event.direction == Direction::ClientToServer)
        .filter_map(|event| match &event.body {
            EventBody::Http {
                method, headers, ..
            } => Some((method.clone(), headers.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn an_http_capture_imports_to_the_same_messages_and_a_subset_of_the_headers() {
    for name in ["http-2025-11-25", "http-2026-07-28"] {
        let imported = mcpsnoop::import(&native(&format!("{name}.mcpsnoop.jsonl"))).unwrap();
        let ours = events(&native(&format!("{name}.trace.jsonl")));
        let theirs = events(&imported.trace);
        assert_eq!(messages(&theirs), messages(&ours), "{name}");
        // Every POST we recorded is a POST there, with no header we did not see.
        let posts = |all: &[TraceEvent]| -> Vec<_> {
            requests(all)
                .into_iter()
                .filter(|(method, _)| method.as_deref() == Some("POST"))
                .collect()
        };
        let (our_posts, their_posts) = (posts(&ours), posts(&theirs));
        assert_eq!(our_posts.len(), their_posts.len(), "{name}");
        for ((_, ours), (_, theirs)) in our_posts.iter().zip(&their_posts) {
            for (header, value) in theirs {
                assert_eq!(ours.get(header), Some(value), "{name}: {header}");
            }
            assert!(!theirs.contains_key("host") && !theirs.contains_key("mcp-session-id"));
        }
        // A request without a body is there, its method unstated, never guessed.
        let unstated = requests(&theirs)
            .iter()
            .filter(|(method, _)| method.is_none())
            .count();
        let bodiless = requests(&ours)
            .iter()
            .filter(|(method, _)| matches!(method.as_deref(), Some("GET" | "DELETE")))
            .count();
        assert_eq!(unstated, bodiless, "{name}");
        assert!(
            !requests(&theirs)
                .iter()
                .any(|(m, _)| m.as_deref() == Some("GET"))
        );
        assert!(
            imported
                .not_recorded
                .iter()
                .any(|gap| gap.contains("Mcp-Session-Id"))
        );
    }
}

#[test]
fn a_batch_is_one_message_again() {
    let envelope = |seq: u32, direction: &str, raw: Value, extra: Value| {
        let mut value = json!({"session_id":"s","seq":seq,"ts":"2026-01-01T00:00:00Z","direction":direction,"transport":"http","raw":raw,"conn_id":"c","exchange":1,"batch":true});
        for (k, v) in extra.as_object().unwrap() {
            value[k] = v.clone();
        }
        value.to_string()
    };
    let capture = [
        json!({"session_id":"s","seq":1,"direction":"meta","transport":"http","raw":{"target":"t"}}).to_string(),
        envelope(2, "c2s", json!({"jsonrpc":"2.0","id":1,"method":"a"}), json!({})),
        envelope(3, "c2s", json!({"jsonrpc":"2.0","id":2,"method":"b"}), json!({})),
        envelope(4, "s2c", json!({"jsonrpc":"2.0","id":1,"result":{}}), json!({"status":200,"transport_headers":{"content_type":"application/json"}})),
        envelope(5, "s2c", json!({"jsonrpc":"2.0","id":2,"result":{}}), json!({"status":200})),
    ]
    .join("\n");
    let imported = mcpsnoop::import(&capture).unwrap();
    let all = events(&imported.trace);
    let shapes: Vec<String> = all
        .iter()
        .map(|event| match &event.body {
            EventBody::Http { method, status, .. } => format!("http {method:?} {status:?}"),
            EventBody::Message { payload } => {
                format!("batch of {}", payload.as_array().map_or(0, Vec::len))
            }
            _ => "other".to_owned(),
        })
        .collect();
    assert_eq!(
        shapes,
        [
            "http Some(\"POST\") None",
            "batch of 2",
            "http None Some(200)",
            "batch of 2"
        ]
    );
}

#[test]
fn malformed_captures_are_refused_with_the_line() {
    let meta = r#"{"session_id":"s","seq":1,"direction":"meta","transport":"stdio","raw":{}}"#;
    assert!(
        mcpsnoop::import("")
            .unwrap_err()
            .0
            .contains("meta envelope")
    );
    assert!(
        mcpsnoop::import(r#"{"direction":"c2s"}"#)
            .unwrap_err()
            .0
            .contains("meta envelope")
    );
    let error = mcpsnoop::import(&format!("{meta}\n\nnot json")).unwrap_err();
    assert!(error.0.starts_with("line 3 is not JSON"), "{error}");
    let ws = meta.replace("stdio", "websocket");
    assert!(mcpsnoop::import(&ws).unwrap_err().0.contains("websocket"));
    // An envelope of another direction is not a frame.
    let odd = format!("{meta}\n{{\"direction\":\"x\",\"raw\":{{}}}}\n{{\"direction\":\"c2s\"}}");
    assert_eq!(mcpsnoop::import(&odd).unwrap().trace, "");
}

fn entry(ts: &str, direction: &str, origin: &str, message: &Value) -> Value {
    json!({"id":"x","timestamp":ts,"direction":direction,"origin":origin,"message":message})
}

// One scenario, step by step; each step reads the state the last left.
#[allow(clippy::too_many_lines)]
#[test]
fn an_inspector_export_is_ordered_by_time_with_answers_placed_by_duration() {
    let mut request = entry(
        "2026-10-13T10:00:00.000Z",
        "request",
        "client",
        &json!({"jsonrpc":"2.0","id":0,"method":"tools/call"}),
    );
    request["response"] = json!({"jsonrpc":"2.0","id":0,"result":{"content":[]}});
    request["duration"] = json!(20);
    let mut server_request = entry(
        "2026-10-13T10:00:00.005Z",
        "request",
        "server",
        &json!({"jsonrpc":"2.0","id":0,"method":"sampling/createMessage"}),
    );
    server_request["response"] = json!({"jsonrpc":"2.0","id":0,"result":{}});
    server_request["duration"] = json!(0);
    let export = Value::Array(vec![
        request,
        server_request,
        entry(
            "2026-10-13T10:00:00.005Z",
            "notification",
            "server",
            &json!({"jsonrpc":"2.0","method":"notifications/progress"}),
        ),
        entry(
            "2026-10-13T10:00:00.030Z",
            "response",
            "server",
            &json!({"jsonrpc":"2.0","id":9,"result":{}}),
        ),
    ]);
    let imported = inspector::import(&export.to_string(), TransportKind::StreamableHttp).unwrap();
    let all = events(&imported.trace);
    let read: Vec<(Direction, String, String)> = all
        .iter()
        .map(|event| {
            let payload = event.message_payload().unwrap();
            let what = payload
                .get("method")
                .and_then(Value::as_str)
                .map_or_else(|| format!("answer {}", payload["id"]), ToOwned::to_owned);
            (event.direction, what, event.ts.clone().unwrap())
        })
        .collect();
    let (c, s) = (Direction::ClientToServer, Direction::ServerToClient);
    assert_eq!(
        read,
        [
            (c, "tools/call".into(), "2026-10-13T10:00:00.000Z".into()),
            (
                s,
                "sampling/createMessage".into(),
                "2026-10-13T10:00:00.005Z".into()
            ),
            (c, "answer 0".into(), "2026-10-13T10:00:00.005Z".into()),
            (
                s,
                "notifications/progress".into(),
                "2026-10-13T10:00:00.005Z".into()
            ),
            (s, "answer 0".into(), "2026-10-13T10:00:00.020Z".into()),
            (s, "answer 9".into(), "2026-10-13T10:00:00.030Z".into()),
        ]
    );
    assert!(
        all.iter()
            .all(|e| e.transport == TransportKind::StreamableHttp)
    );
    assert_eq!(
        imported.recorded_at.as_deref(),
        Some("2026-10-13T10:00:00.000Z")
    );
    assert_eq!(imported.not_recorded.len(), inspector::NOT_RECORDED.len());
}

#[test]
fn an_inspector_export_needs_its_transport_stated_and_its_entries_whole() {
    let export = json!([entry(
        "2026-10-13T10:00:00.000Z",
        "notification",
        "client",
        &json!({})
    )])
    .to_string();
    let error = import(&export, Format::Inspector, &Options::default()).unwrap_err();
    assert!(
        error.0.contains("does not record which transport"),
        "{error}"
    );
    let options = Options::default().transport(TransportKind::Stdio);
    assert!(import(&export, Format::Inspector, &options).is_ok());
    for (bad, why) in [
        ("{", "not JSON"),
        ("{}", "JSON array"),
        (
            r#"[{"origin":"client","timestamp":"2026-10-13T10:00:00.000Z"}]"#,
            "has no message",
        ),
        (
            r#"[{"origin":"proxy","message":{},"timestamp":"2026-10-13T10:00:00.000Z"}]"#,
            "origin",
        ),
        (
            r#"[{"origin":"client","message":{},"timestamp":"yesterday"}]"#,
            "timestamp",
        ),
        (
            r#"[{"origin":"client","message":{},"timestamp":"2026-10-13T10:00:00.000Z","response":{}}]"#,
            "no duration",
        ),
    ] {
        let error = inspector::import(bad, TransportKind::Stdio).unwrap_err();
        assert!(error.0.contains(why), "{bad}: {error}");
    }
    assert_eq!(ImportError("x".into()).to_string(), "x");
}

#[test]
fn an_http_capture_keeps_exactly_the_headers_it_recorded() {
    // Each HTTP event's direction, status and headers.
    type Shown = (Direction, Option<u16>, Vec<(String, String)>);
    let capture = [
        json!({"session_id":"s","seq":1,"direction":"meta","transport":"http","raw":{"target":"t"}}),
        json!({"session_id":"s","seq":2,"direction":"c2s","transport":"http","conn_id":"c","exchange":1,
            "raw":{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo"}},
            "mcp_protocol_version":"2025-11-25","mcp_method":"tools/call","mcp_name":"echo",
            "mcp_param_headers":[{"name":"Mcp-Param-Region","value":"eu"}],
            "transport_headers":{"accept":"application/json, text/event-stream","content_type":"application/json","origin":"http://localhost"}}),
        json!({"session_id":"s","seq":3,"direction":"s2c","transport":"http","conn_id":"c","exchange":1,
            "raw":{"jsonrpc":"2.0","id":1,"result":{}},"status":200,
            "transport_headers":{"accept":"*/*","content_type":"text/event-stream","origin":"x"}}),
    ]
    .map(|envelope| envelope.to_string())
    .join("\n");
    let all = events(&mcpsnoop::import(&capture).unwrap().trace);
    let headers: Vec<Shown> = all
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::Http {
                status, headers, ..
            } => Some((
                event.direction,
                *status,
                headers.clone().into_iter().collect(),
            )),
            _ => None,
        })
        .collect();
    let pairs = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };
    assert_eq!(
        headers,
        [
            (
                Direction::ClientToServer,
                None,
                pairs(&[
                    ("accept", "application/json, text/event-stream"),
                    ("content-type", "application/json"),
                    ("mcp-method", "tools/call"),
                    ("mcp-name", "echo"),
                    ("mcp-param-region", "eu"),
                    ("mcp-protocol-version", "2025-11-25"),
                    ("origin", "http://localhost"),
                ])
            ),
            (
                Direction::ServerToClient,
                Some(200),
                pairs(&[("content-type", "text/event-stream")])
            ),
        ]
    );
}

#[test]
fn a_stdio_capture_has_no_http_whatever_its_frames_carry() {
    let capture = [
        json!({"session_id":"s","seq":1,"direction":"meta","transport":"stdio","raw":{"command":["server"]}}),
        json!({"session_id":"s","seq":2,"direction":"c2s","transport":"stdio","conn_id":"c","exchange":1,"status":200,
            "raw":{"jsonrpc":"2.0","id":1,"method":"ping"}}),
    ]
    .map(|envelope| envelope.to_string())
    .join("\n");
    let all = events(&mcpsnoop::import(&capture).unwrap().trace);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].transport, TransportKind::Stdio);
    assert!(matches!(all[0].body, EventBody::Message { .. }));
}
