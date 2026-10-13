// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use mcp_trace_validator::reader;
use serde_json::{Value, json};

use super::{CONNECTION, convert};

fn events(lines: &[Value]) -> Vec<mcp_conformance_core::trace::TraceEvent> {
    let mut text = String::new();
    for line in lines {
        text.push_str(&line.to_string());
        text.push('\n');
    }
    reader::parse_trace(&text, &reader::Limits::default()).unwrap()
}

fn post(seq: u64, headers: &Value) -> Value {
    json!({"seq": seq, "direction": "client-to-server", "transport": "streamable-http",
           "kind": "http", "method": "POST", "headers": headers})
}

fn status(seq: u64, code: u16, media: Option<&str>) -> Value {
    let mut event = json!({"seq": seq, "direction": "server-to-client",
                           "transport": "streamable-http", "kind": "http", "status": code});
    if let Some(media) = media {
        event["headers"] = json!({"content-type": media});
    }
    event
}

fn message(seq: u64, client: bool, transport: &str, payload: &Value) -> Value {
    let direction = if client {
        "client-to-server"
    } else {
        "server-to-client"
    };
    json!({"seq": seq, "direction": direction, "transport": transport, "kind": "message",
           "payload": payload})
}

fn request(id: u64, method: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": method})
}

fn result(id: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {}})
}

#[test]
fn stdio_converts_every_message_and_only_messages() {
    let trace = events(&[
        json!({"seq": 0, "direction": "client-to-server", "transport": "stdio",
               "kind": "lifecycle", "event": "transport-open"}),
        message(1, true, "stdio", &request(1, "ping")),
        message(2, false, "stdio", &result(&json!(1))),
        message(3, true, "stdio", &json!([request(2, "ping")])),
    ]);
    let capture = convert("good/x", &trace);
    assert_eq!(
        capture.lines.len(),
        4,
        "meta plus three messages; the lifecycle event has no counterpart"
    );
    assert_eq!(capture.lines[0]["direction"], "meta");
    assert_eq!(capture.lines[1]["direction"], "c2s");
    assert_eq!(capture.lines[1]["raw"], request(1, "ping"));
    assert_eq!(capture.lines[2]["direction"], "s2c");
    // A batch stays one frame on stdio, as mcpsnoop's shim records the line.
    assert!(capture.lines[3]["raw"].is_array());
    assert!(capture.lines[3].get("batch").is_none());
    for (index, line) in capture.lines.iter().enumerate() {
        assert_eq!(line["seq"], json!(index + 1));
        assert_eq!(line["transport"], "stdio");
        assert!(line.get("conn_id").is_none());
    }
    let seqs: Vec<Option<u64>> = capture.frames.iter().map(|frame| frame.seq).collect();
    assert_eq!(seqs, [None, Some(1), Some(2), Some(3)]);
    assert!(capture.frames.iter().all(|frame| frame.framed));
}

#[test]
fn http_carries_the_request_headers_and_the_response_status() {
    let headers = json!({"accept": "application/json, text/event-stream",
        "content-type": "application/json", "mcp-protocol-version": "2026-07-28",
        "mcp-method": "tools/call", "mcp-name": "echo", "mcp-param-region": "eu",
        "host": "localhost"});
    let trace = events(&[
        post(0, &headers),
        message(1, true, "streamable-http", &request(1, "tools/call")),
        status(2, 200, Some("application/json")),
        message(3, false, "streamable-http", &result(&json!(1))),
        post(4, &json!({"content-type": "application/json"})),
        message(
            5,
            true,
            "streamable-http",
            &json!({"jsonrpc": "2.0", "method": "notifications/x"}),
        ),
        status(6, 202, None),
    ]);
    let capture = convert("draft/good/x", &trace);
    let lines = &capture.lines;
    assert_eq!(
        lines.len(),
        5,
        "meta, request, response, notification, the bodiless 202"
    );
    assert_eq!(lines[1]["mcp_method"], "tools/call");
    assert_eq!(lines[1]["mcp_name"], "echo");
    assert_eq!(lines[1]["mcp_protocol_version"], "2026-07-28");
    assert_eq!(
        lines[1]["mcp_param_headers"],
        json!([{"name": "Mcp-Param-Region", "value": "eu"}])
    );
    assert_eq!(
        lines[1]["transport_headers"],
        json!({"accept": "application/json, text/event-stream", "content_type": "application/json"}),
        "mcpsnoop records no Host header"
    );
    assert_eq!(lines[2]["status"], 200);
    assert_eq!(lines[2]["exchange"], lines[1]["exchange"]);
    assert_eq!(
        lines[2]["transport_headers"],
        json!({"content_type": "application/json"})
    );
    assert_eq!(lines[4]["status"], 202);
    assert!(lines[4].get("raw").is_none());
    assert_eq!(lines[4]["exchange"], lines[3]["exchange"]);
    assert!(
        lines
            .iter()
            .skip(1)
            .all(|line| line["conn_id"] == CONNECTION)
    );
    assert!(capture.frames.iter().all(|frame| frame.framed));
}

#[test]
fn overlapping_posts_are_left_unframed() {
    let headers = json!({"content-type": "application/json", "mcp-method": "ping"});
    let trace = events(&[
        post(0, &headers),
        post(1, &headers),
        message(2, true, "streamable-http", &request(1, "ping")),
        message(3, true, "streamable-http", &request(2, "ping")),
        status(4, 200, Some("application/json")),
        status(5, 500, None),
        message(6, false, "streamable-http", &result(&json!(1))),
    ]);
    let capture = convert("x", &trace);
    for line in capture
        .lines
        .iter()
        .skip(1)
        .filter(|line| line.get("raw").is_some())
    {
        assert!(line.get("exchange").is_none(), "{line}");
        assert!(line.get("mcp_method").is_none(), "{line}");
        assert!(line.get("status").is_none(), "{line}");
    }
    assert_eq!(
        capture.frames.iter().filter(|frame| !frame.framed).count(),
        3
    );
}

#[test]
fn overlapping_exchanges_with_one_status_between_them_share_it() {
    let trace = events(&[
        json!({"seq": 0, "direction": "client-to-server", "transport": "streamable-http",
               "kind": "http", "method": "GET", "headers": {"accept": "text/event-stream"}}),
        post(1, &json!({"content-type": "application/json"})),
        message(2, true, "streamable-http", &request(1, "tools/list")),
        status(3, 200, Some("text/event-stream")),
        status(4, 200, Some("text/event-stream")),
        message(5, false, "streamable-http", &result(&json!(1))),
    ]);
    let capture = convert("x", &trace);
    let response = capture
        .lines
        .iter()
        .find(|line| line["direction"] == "s2c")
        .unwrap();
    assert_eq!(
        response["status"], 200,
        "either assignment reads the same, so neither is invented"
    );
    assert_eq!(
        response["transport_headers"]["content_type"],
        "text/event-stream"
    );
    let request = capture
        .lines
        .iter()
        .find(|line| line["direction"] == "c2s")
        .unwrap();
    assert_eq!(response["exchange"], request["exchange"]);
}

#[test]
fn a_null_id_answer_rides_the_exchange_it_followed() {
    let trace = events(&[
        post(0, &json!({"content-type": "application/json"})),
        message(1, true, "streamable-http", &request(1, "tools/call")),
        status(2, 200, Some("application/json")),
        message(
            3,
            false,
            "streamable-http",
            &json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32602, "message": "x"}}),
        ),
    ]);
    let capture = convert("x", &trace);
    assert_eq!(capture.lines[2]["exchange"], capture.lines[1]["exchange"]);
    assert_eq!(capture.lines[2]["status"], 200);
}

#[test]
fn server_notifications_are_never_given_a_stream() {
    let trace = events(&[
        post(0, &json!({"content-type": "application/json"})),
        message(1, true, "streamable-http", &request(1, "tools/call")),
        status(2, 200, Some("text/event-stream")),
        message(
            3,
            false,
            "streamable-http",
            &json!({"jsonrpc": "2.0", "method": "notifications/progress"}),
        ),
        message(4, false, "streamable-http", &result(&json!(1))),
    ]);
    let capture = convert("x", &trace);
    let notification = &capture.lines[2];
    assert!(notification.get("exchange").is_none());
    assert_eq!(notification["conn_id"], CONNECTION);
    assert!(!capture.frames[2].framed);
    assert_eq!(capture.lines[3]["status"], 200);
}

/// The committed native pairs: every message frame mcpsnoop recorded is in the
/// conversion, and wherever the conversion attaches HTTP context it is the
/// context mcpsnoop itself recorded. (That the two draw identical `check`
/// findings needs the binary, and is asserted by every measurement.)
#[test]
fn conversion_matches_mcpsnoop_own_recordings() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/mcpsnoop/native");
    let mut pairs = 0;
    for session in ["stdio-2025-11-25", "http-2025-11-25", "http-2026-07-28"] {
        let trace =
            std::fs::read_to_string(directory.join(format!("{session}.trace.jsonl"))).unwrap();
        let native: Vec<Value> =
            std::fs::read_to_string(directory.join(format!("{session}.mcpsnoop.jsonl")))
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        let events = reader::parse_trace(&trace, &reader::Limits::default()).unwrap();
        let converted = convert(session, &events).lines;
        let messages = |lines: &[Value]| {
            let mut out: Vec<String> = lines
                .iter()
                .filter(|line| line.get("raw").is_some() && line["direction"] != "meta")
                .map(|line| format!("{} {}", line["direction"], line["raw"]))
                .collect();
            out.sort();
            out
        };
        assert_eq!(
            messages(&converted),
            messages(&native),
            "{session}: message frames"
        );
        for line in converted
            .iter()
            .filter(|line| line.get("exchange").is_some() && line.get("raw").is_some())
        {
            let twin = native
                .iter()
                .find(|candidate| {
                    candidate["raw"] == line["raw"] && candidate["direction"] == line["direction"]
                })
                .unwrap();
            for field in [
                "status",
                "transport_headers",
                "mcp_method",
                "mcp_name",
                "mcp_protocol_version",
                "mcp_param_headers",
            ] {
                assert_eq!(
                    line.get(field),
                    twin.get(field),
                    "{session}: {field} of {}",
                    line["raw"]
                );
            }
        }
        pairs += 1;
    }
    assert_eq!(pairs, 3);
}

/// The two directions agree: every committed corpus trace converted to a
/// capture and imported back (`mcp_conformance_cli::import::mcpsnoop`, the
/// workbench's importer) carries the same messages, in the same order.
#[test]
fn conversion_and_import_round_trip_every_corpus_message() {
    use mcp_conformance_cli::import::mcpsnoop;
    use mcp_conformance_core::trace::{Direction, TraceEvent};

    let messages = |events: &[TraceEvent]| -> Vec<(bool, Value)> {
        events
            .iter()
            .filter_map(|event| {
                event.message_payload().map(|payload| {
                    (
                        event.direction == Direction::ClientToServer,
                        payload.clone(),
                    )
                })
            })
            .collect()
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut traces = 0;
    for directory in [
        "corpus/good",
        "corpus/violations",
        "corpus/captured",
        "corpus/draft/good",
    ] {
        for file in std::fs::read_dir(root.join(directory)).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let Ok(events) = reader::parse_trace(&text, &reader::Limits::default()) else {
                continue; // a malformed-trace fixture: nothing to convert
            };
            let mut document = String::new();
            for line in &convert("round-trip", &events).lines {
                document.push_str(&line.to_string());
                document.push('\n');
            }
            let imported = mcpsnoop::import(&document).unwrap();
            let back = reader::parse_trace(&imported.trace, &reader::Limits::default()).unwrap();
            assert_eq!(messages(&back), messages(&events), "{}", path.display());
            traces += 1;
        }
    }
    assert!(traces > 50, "only {traces} traces round-tripped");
}
