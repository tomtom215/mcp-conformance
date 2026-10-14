// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;
use crate::workbench::fixture;

#[test]
fn a_session_puts_each_finding_at_its_event() {
    let (workbench, _dir) = fixture::workbench("view");
    let id = "base-001-request-id-boolean";
    let events = workbench.events(id).unwrap();
    let view = session(workbench.entry(id).unwrap(), &events, 0);
    assert_eq!(view["total_events"], 4);
    assert_eq!(view["next"], Value::Null);
    let findings = view["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 2);
    assert_eq!(findings[0]["key"], "f1");
    assert_eq!(findings[0]["requirement"], "BASE-001");
    assert_eq!(findings[0]["seq"], 3);
    assert!(findings[0]["quote"].as_str().unwrap().contains("MUST"));
    assert!(findings[0]["url"].as_str().unwrap().starts_with("https://"));
    assert_eq!(findings[0]["outcome"], "fail");
    assert_eq!(findings[1]["requirement"], "wire schema");
    assert_eq!(findings[1]["outcome"], "fail");
    assert_eq!(view["totals"][0]["verdict"], "fail");
    let marked = &view["events"][3];
    assert_eq!(marked["kind"], "request");
    assert_eq!(marked["method"], "ping");
    assert_eq!(marked["id"], "true");
    assert_eq!(marked["marks"].as_array().unwrap().len(), 2);
    assert_eq!(marked["marks"][0]["key"], "f1");
    assert!(marked["payload"].as_str().unwrap().contains("\"id\": true"));
    assert_eq!(view["events"][0]["answered_by"], 1);
    assert_eq!(view["events"][1]["answers"], 0);
    assert_eq!(view["events"][1]["method"], "initialize");
    assert_eq!(view["attention"], 1);
}

#[test]
fn every_kind_of_event_reads_as_the_report_reads_it() {
    let (workbench, _dir) = fixture::workbench("kinds");
    let id = "everything-http";
    let events = workbench.events(id).unwrap();
    let view = session(workbench.entry(id).unwrap(), &events, 0);
    let kinds: Vec<&str> = view["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"http") && kinds.contains(&"result"));
    let http = view["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "http")
        .unwrap();
    assert_eq!(http["http_method"], "POST");
    assert!(http["headers"]["accept"].is_string());
    let hostile = workbench.events("hostile-markup").unwrap();
    let view = session(workbench.entry("hostile-markup").unwrap(), &hostile, 0);
    assert_eq!(view["events"][6]["kind"], "error");
    assert_eq!(view["events"][6]["error"], "error -32602");
    let trace = "{\"seq\":0,\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"lifecycle\",\"event\":\"transport-open\"}\n";
    let events = mcp_trace_validator::reader::parse_trace(
        trace,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .unwrap();
    let view = session(workbench.entry("everything-stdio").unwrap(), &events, 0);
    assert_eq!(view["events"][0]["lifecycle"], "transport opened");
    let trace = "{\"seq\":0,\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n{\"seq\":1,\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"},{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}]}\n";
    let events = mcp_trace_validator::reader::parse_trace(
        trace,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .unwrap();
    let view = session(workbench.entry("everything-stdio").unwrap(), &events, 0);
    let notification = &view["events"][0];
    assert_eq!(notification["kind"], "notification");
    assert_eq!(notification["method"], "notifications/initialized");
    assert_eq!(notification["id"], Value::Null);
    let batch = &view["events"][1];
    assert_eq!(batch["kind"], "batch");
    assert_eq!(batch["batch"], 2);
}

#[test]
fn pages_start_where_asked() {
    let (workbench, _dir) = fixture::workbench("pages");
    let id = "sdk-go-2025-11-25";
    let events = workbench.events(id).unwrap();
    let view = session(workbench.entry(id).unwrap(), &events, 38);
    assert_eq!(view["events"].as_array().unwrap().len(), 2);
    assert_eq!(view["events"][0]["seq"], 38);
    let past = session(workbench.entry(id).unwrap(), &events, 400);
    assert_eq!(past["events"], json!([]));
}

#[test]
fn latency_is_read_from_timestamps_when_both_ends_have_one() {
    assert_eq!(ts_millis("1970-01-01T00:00:01.5Z"), Some(1_500));
    assert_eq!(ts_millis("1970-01-02T00:00:00Z"), Some(86_400_000));
    assert_eq!(ts_millis("1970-01-02T01:02:03.004Z"), Some(90_123_004));
    assert_eq!(
        ts_millis("2026-10-13T10:54:29.018982679Z").map(|ms| ms % 1000),
        Some(18)
    );
    assert_eq!(ts_millis("nonsense"), None);
    let trace = "{\"seq\":0,\"ts\":\"2026-01-01T00:00:00.100Z\",\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}}\n{\"seq\":1,\"ts\":\"2026-01-01T00:00:00.142Z\",\"direction\":\"server-to-client\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}}\n";
    let events = mcp_trace_validator::reader::parse_trace(
        trace,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .unwrap();
    let pairs = timeline::pair(&events);
    assert_eq!(latency(&events, &pairs).get(&0), Some(&42));
}
