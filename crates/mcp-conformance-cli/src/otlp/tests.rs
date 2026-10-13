// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The OTLP/HTTP JSON encoding, held to the protobuf JSON mapping as OTLP
//! specifies it: lowerCamelCase field names, ids as lowercase hex (not
//! base64), enums as integers, 64-bit integers as decimal strings, attribute
//! values as a one-member `AnyValue` object naming their type.

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mcp_conformance_core::trace::{Direction, TransportKind};
use mcp_trace_validator::report::Outcome;
use serde_json::{Value, json};

use super::*;
use crate::live::{Ending, Exchange, LiveFinding};

const TRACE: [u8; 16] = [
    0x4b, 0xf9, 0x2f, 0x35, 0x77, 0xb3, 0x4d, 0xa6, 0xa3, 0xce, 0x92, 0x9d, 0x0e, 0x0e, 0x47, 0x36,
];
const SPAN: [u8; 8] = [0x00, 0xf0, 0x67, 0xaa, 0x0b, 0xa9, 0x02, 0xb7];

fn at(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(1_700_000_000_000 + millis)
}

fn exchange(method: &str, id: Option<&str>) -> Exchange {
    let mut live = crate::live::Live::new().unwrap();
    let id_member = id.map_or_else(String::new, |id| format!(r#","id":{id}"#));
    let request = format!(
        r#"{{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0"{id_member},"method":"{method}","params":{{"name":"echo","uri":"file:///r"}}}}}}"#
    );
    let pushed = live.push(request.as_bytes(), at(0));
    if id.is_none() {
        return pushed.completed.into_iter().next().unwrap();
    }
    let response = format!(
        r#"{{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0"{id_member},"result":{{}}}}}}"#
    );
    live.push(response.as_bytes(), at(1500))
        .completed
        .into_iter()
        .next()
        .unwrap()
}

fn encoded(span: &Span) -> Value {
    serde_json::to_value(span).unwrap()
}

/// The attribute `key`'s `AnyValue`, which must appear once.
fn attribute(span: &Value, key: &str) -> Option<Value> {
    let matching: Vec<&Value> = span["attributes"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|attribute| attribute["key"] == key)
        .collect();
    assert!(matching.len() <= 1, "{key} twice");
    matching.first().map(|attribute| attribute["value"].clone())
}

#[test]
fn a_span_uses_the_mapping_hex_ids_integer_enums_and_string_times() {
    let span = encoded(&span(&exchange("ping", Some("1")), TRACE, SPAN, None));
    assert_eq!(span["traceId"], "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(span["spanId"], "00f067aa0ba902b7");
    assert_eq!(span["kind"], json!(3), "SPAN_KIND_CLIENT, as an integer");
    assert_eq!(span["startTimeUnixNano"], "1700000000000000000");
    assert_eq!(span["endTimeUnixNano"], "1700000001500000000");
    assert_eq!(span["name"], "ping");
    assert!(
        span.get("status").is_none(),
        "unset is omitted, as proto3 omits a default"
    );
    assert!(span.get("events").is_none());
    assert!(span.get("parentSpanId").is_none(), "a root span");
    for attribute in span["attributes"].as_array().unwrap() {
        let value = attribute["value"].as_object().unwrap();
        assert_eq!(
            value.len(),
            1,
            "AnyValue holds exactly one member: {value:?}"
        );
    }
    assert_eq!(
        attribute(&span, "mcp.method.name"),
        Some(json!({"stringValue": "ping"}))
    );
    assert_eq!(
        attribute(&span, "jsonrpc.request.id"),
        Some(json!({"stringValue": "1"}))
    );
    assert_eq!(
        attribute(&span, "network.transport"),
        Some(json!({"stringValue": "pipe"}))
    );
    assert_eq!(
        attribute(&span, "jsonrpc.protocol.version"),
        None,
        "only when not 2.0"
    );
    assert_eq!(
        attribute(&span, "server.address"),
        None,
        "stdio has no server address"
    );
}

#[test]
fn the_name_carries_the_tool_or_prompt_and_the_attributes_name_them() {
    let call = encoded(&span(
        &exchange("tools/call", Some(r#""a""#)),
        TRACE,
        SPAN,
        None,
    ));
    assert_eq!(call["name"], "tools/call echo");
    assert_eq!(
        attribute(&call, "gen_ai.tool.name"),
        Some(json!({"stringValue": "echo"}))
    );
    assert_eq!(
        attribute(&call, "gen_ai.operation.name"),
        Some(json!({"stringValue": "execute_tool"}))
    );
    assert_eq!(
        attribute(&call, "jsonrpc.request.id"),
        Some(json!({"stringValue": "a"})),
        "a string id without its JSON quotes"
    );
    let prompt = encoded(&span(
        &exchange("prompts/get", Some("2")),
        TRACE,
        SPAN,
        None,
    ));
    assert_eq!(prompt["name"], "prompts/get echo");
    assert_eq!(
        attribute(&prompt, "gen_ai.prompt.name"),
        Some(json!({"stringValue": "echo"}))
    );
    assert_eq!(attribute(&prompt, "gen_ai.operation.name"), None);
    let read = encoded(&span(
        &exchange("resources/read", Some("3")),
        TRACE,
        SPAN,
        None,
    ));
    assert_eq!(
        read["name"], "resources/read",
        "a URI is not a low-cardinality target"
    );
    assert_eq!(
        attribute(&read, "mcp.resource.uri"),
        Some(json!({"stringValue": "file:///r"}))
    );
    let notified = encoded(&span(
        &exchange("notifications/initialized", None),
        TRACE,
        SPAN,
        None,
    ));
    assert_eq!(
        attribute(&notified, "jsonrpc.request.id"),
        None,
        "no id, no attribute"
    );
    assert_eq!(notified["startTimeUnixNano"], notified["endTimeUnixNano"]);
    let null = encoded(&span(&exchange("ping", Some("null")), TRACE, SPAN, None));
    assert_eq!(
        attribute(&null, "jsonrpc.request.id"),
        None,
        "a null id is not captured"
    );
}

#[test]
fn errors_set_the_error_attributes_and_an_error_status() {
    let mut failed = exchange("tools/call", Some("1"));
    failed.ending = Ending::Answered {
        error: Some((Some("-32602".to_owned()), Some("Unknown tool".to_owned()))),
        tool_error: false,
    };
    let span = encoded(&span(&failed, TRACE, SPAN, None));
    assert_eq!(
        attribute(&span, "error.type"),
        Some(json!({"stringValue": "-32602"}))
    );
    assert_eq!(
        attribute(&span, "rpc.response.status_code"),
        Some(json!({"stringValue": "-32602"}))
    );
    assert_eq!(
        span["status"],
        json!({"code": 2, "message": "Unknown tool"})
    );

    failed.ending = Ending::Answered {
        error: Some((None, None)),
        tool_error: false,
    };
    let span = encoded(&super::span(&failed, TRACE, SPAN, None));
    assert_eq!(
        attribute(&span, "error.type"),
        Some(json!({"stringValue": "_OTHER"}))
    );
    assert_eq!(attribute(&span, "rpc.response.status_code"), None);
    assert_eq!(span["status"], json!({"code": 2}));

    failed.ending = Ending::Answered {
        error: None,
        tool_error: true,
    };
    let span = encoded(&super::span(&failed, TRACE, SPAN, None));
    assert_eq!(
        attribute(&span, "error.type"),
        Some(json!({"stringValue": "tool_error"}))
    );

    failed.ending = Ending::Unanswered;
    let span = encoded(&super::span(&failed, TRACE, SPAN, None));
    assert_eq!(
        attribute(&span, "error.type"),
        Some(json!({"stringValue": "unanswered"}))
    );
    assert_eq!(span["status"]["code"], json!(2));
}

#[test]
fn an_http_exchange_names_its_transport_and_for_a_client_request_its_server() {
    let mut http = exchange("initialize", Some("1"));
    http.transport = TransportKind::StreamableHttp;
    http.session_id = Some("191c4850af6c49e08843a3f6c80e5046".to_owned());
    http.jsonrpc = Some("1.0".to_owned());
    let peer = Peer::new("example.com", 8443);
    let span = encoded(&span(&http, TRACE, SPAN, Some(&peer)));
    assert_eq!(
        attribute(&span, "network.transport"),
        Some(json!({"stringValue": "tcp"}))
    );
    assert_eq!(
        attribute(&span, "network.protocol.name"),
        Some(json!({"stringValue": "http"}))
    );
    assert_eq!(
        attribute(&span, "network.protocol.version"),
        Some(json!({"stringValue": "1.1"}))
    );
    assert_eq!(
        attribute(&span, "server.address"),
        Some(json!({"stringValue": "example.com"}))
    );
    assert_eq!(
        attribute(&span, "server.port"),
        Some(json!({"intValue": "8443"})),
        "an int attribute is an intValue, an int64 written as a string"
    );
    assert_eq!(
        attribute(&span, "mcp.session.id"),
        Some(json!({"stringValue": "191c4850af6c49e08843a3f6c80e5046"}))
    );
    assert_eq!(
        attribute(&span, "jsonrpc.protocol.version"),
        Some(json!({"stringValue": "1.0"}))
    );
    http.initiator = Direction::ServerToClient;
    let asked = encoded(&super::span(&http, TRACE, SPAN, Some(&peer)));
    assert_eq!(
        attribute(&asked, "server.address"),
        None,
        "the server asked: no server peer"
    );
}

fn finding(seq: Option<u64>) -> LiveFinding {
    let mut live = crate::live::Live::new().unwrap();
    // A tool name outside the allowed characters: TOOL-006, at seq 4.
    for line in [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"s","version":"0"}}}}"#,
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
        r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#,
        r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"bad name!","inputSchema":{"type":"object"}}]}}}"#,
    ] {
        live.push(line.as_bytes(), at(0));
    }
    let mut found = live
        .judge()
        .new
        .into_iter()
        .find(|finding| finding.requirement == "TOOL-006")
        .unwrap();
    found.seq = seq;
    found
}

#[test]
fn a_finding_is_an_event_with_its_requirement_level_outcome_and_spec_url() {
    let mut span = span(&exchange("tools/list", Some("2")), TRACE, SPAN, None);
    span.add_finding(&finding(Some(4)), at(2000));
    let event = &encoded(&span)["events"][0];
    assert_eq!(event["name"], FINDING_EVENT);
    assert_eq!(event["timeUnixNano"], "1700000002000000000");
    assert_eq!(
        attribute(event, "mcp_conformance.requirement.id"),
        Some(json!({"stringValue": "TOOL-006"}))
    );
    assert_eq!(
        attribute(event, "mcp_conformance.outcome"),
        Some(json!({"stringValue": "warn"}))
    );
    assert!(attribute(event, "mcp_conformance.requirement.level").is_some());
    assert_eq!(
        attribute(event, "mcp_conformance.spec.url").unwrap()["stringValue"]
            .as_str()
            .unwrap()
            .split('#')
            .next(),
        Some("https://modelcontextprotocol.io/specification/2025-11-25/server/tools")
    );
    assert_eq!(
        attribute(event, "mcp_conformance.trace.seq"),
        Some(json!({"intValue": "4"}))
    );
    assert_eq!(
        attribute(event, "mcp_conformance.revision"),
        Some(json!({"stringValue": "2025-11-25"}))
    );
    assert_eq!(span.findings(), 1);
    let mut failure = finding(None);
    failure.outcome = Outcome::Fail;
    span.add_finding(&failure, at(2000));
    let event = &encoded(&span)["events"][1];
    assert_eq!(
        attribute(event, "mcp_conformance.outcome"),
        Some(json!({"stringValue": "fail"}))
    );
    assert_eq!(attribute(event, "mcp_conformance.trace.seq"), None);
}

#[test]
fn a_request_wraps_spans_in_one_resource_and_scope() {
    let request = serde_json::to_value(Request::new(vec![span(
        &exchange("ping", Some("1")),
        TRACE,
        SPAN,
        None,
    )]))
    .unwrap();
    let resource = &request["resourceSpans"][0];
    assert_eq!(
        resource["resource"]["attributes"][0],
        json!({"key": "service.name", "value": {"stringValue": "mcp-conformance"}})
    );
    let scope = &resource["scopeSpans"][0];
    assert_eq!(scope["scope"]["name"], "mcp-conformance watch");
    assert_eq!(scope["scope"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(scope["spans"][0]["name"], "ping");
    assert_eq!(request.as_object().unwrap().len(), 1);
}

#[test]
fn spans_hold_the_latest_back_and_put_each_finding_on_its_exchange() {
    let mut next = 0_u8;
    let ids = Box::new(move || {
        next = next.wrapping_add(1);
        // The first id is all zeros, which a span id must never be.
        if next == 1 { [0; 8] } else { [next; 8] }
    });
    let mut spans = Spans::new(TRACE, None, ids);
    let sent = spans.pass(&[], &[finding(Some(4))], at(0));
    assert!(sent.is_empty(), "{sent:?}");
    assert_eq!(spans.unattached(), 1, "no exchange yet: counted, not sent");

    let mut list = exchange("tools/list", Some("2"));
    list.request_seq = 3;
    list.response_seq = Some(4);
    let sent = spans.pass(std::slice::from_ref(&list), &[], at(1));
    assert!(sent.is_empty(), "the latest span is held back");

    let mut ping = exchange("ping", Some("3"));
    ping.request_seq = 5;
    ping.response_seq = Some(6);
    // Found after the ping completed, but about seq 4: it goes on tools/list,
    // still unsent.
    let sent = spans.pass(&[ping], &[finding(Some(4))], at(2));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].name, "tools/list");
    assert_eq!(sent[0].findings(), 1);
    assert_ne!(encoded(&sent[0])["spanId"], "0000000000000000");

    // About an exchange already sent: the latest unsent span takes it.
    let sent = spans.finish(&[], &[finding(Some(4)), finding(None)], at(3));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].name, "ping");
    assert_eq!(sent[0].findings(), 2);
    assert_eq!(spans.unattached(), 1);
    assert_eq!(
        encoded(&sent[0])["traceId"],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
}

#[test]
fn the_conventions_commit_is_a_full_sha() {
    assert_eq!(SEMCONV_COMMIT.len(), 40);
    assert!(
        SEMCONV_COMMIT
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
}
