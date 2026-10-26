// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared fixtures for the basic-utilities check tests: a `2025-11-25` stdio
//! session, built line by line, and the outcome one check reports on it.

#![allow(clippy::expect_used)]

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

/// What one check reported for a trace: its finding details, in event order,
/// and how many subjects it examined.
pub(super) fn run(check: &str, trace: &str) -> (Vec<String>, u32) {
    let events = parse_trace(trace, &Limits::default()).expect("test trace parses");
    let context = TraceContext::new(&events);
    let outcome = checks::find(check)
        .expect("check is registered")
        .run(&context);
    (
        outcome
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect(),
        outcome.subjects,
    )
}

/// The details one check reports for a trace.
pub(super) fn findings_for(check: &str, trace: &str) -> Vec<String> {
    run(check, trace).0
}

/// How many subjects one check examined in a trace.
pub(super) fn subjects_for(check: &str, trace: &str) -> u32 {
    run(check, trace).1
}

/// One message event over `transport`.
fn line(seq: u64, direction: &str, transport: &str, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"{transport}","kind":"message","payload":{payload}}}"#
    )
}

/// A client message over stdio.
pub(super) fn client(seq: u64, payload: &str) -> String {
    line(seq, "client-to-server", "stdio", payload)
}

/// A server message over stdio.
pub(super) fn server(seq: u64, payload: &str) -> String {
    line(seq, "server-to-client", "stdio", payload)
}

/// A message over Streamable HTTP, from the client when `from_client`.
pub(super) fn http(seq: u64, from_client: bool, payload: &str) -> String {
    let direction = if from_client {
        "client-to-server"
    } else {
        "server-to-client"
    };
    line(seq, direction, "streamable-http", payload)
}

/// Joins records into a trace document.
pub(super) fn trace(lines: &[String]) -> String {
    lines.join("\n")
}

/// A `2025-11-25` handshake at seq 0–2: the client declares `client`, the
/// server `server`, both capability objects given as JSON text.
pub(super) fn handshake(client_capabilities: &str, server_capabilities: &str) -> Vec<String> {
    vec![
        client(
            0,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{client_capabilities},"clientInfo":{{"name":"c","version":"1"}}}}}}"#
            ),
        ),
        server(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{server_capabilities},"serverInfo":{{"name":"s","version":"1"}}}}}}"#
            ),
        ),
        client(
            2,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ),
    ]
}

/// A handshake declaring `tools` on the server and nothing on the client,
/// followed by `rest` (whose records start at seq 3).
pub(super) fn session(rest: &[String]) -> String {
    let mut lines = handshake("{}", r#"{"tools":{}}"#);
    lines.extend_from_slice(rest);
    trace(&lines)
}

/// A request `id` (JSON text) for `method` with `params` (JSON text), from the
/// client when `from_client`.
pub(super) fn request(seq: u64, from_client: bool, id: &str, method: &str, params: &str) -> String {
    let payload = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#);
    if from_client {
        client(seq, &payload)
    } else {
        server(seq, &payload)
    }
}

/// A result `result` (JSON text) answering `id`, from the client when
/// `from_client`.
pub(super) fn result(seq: u64, from_client: bool, id: &str, result: &str) -> String {
    let payload = format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#);
    if from_client {
        client(seq, &payload)
    } else {
        server(seq, &payload)
    }
}

/// A notification of `method` with `params` (JSON text), from the client when
/// `from_client`.
pub(super) fn notify(seq: u64, from_client: bool, method: &str, params: &str) -> String {
    let payload = format!(r#"{{"jsonrpc":"2.0","method":"{method}","params":{params}}}"#);
    if from_client {
        client(seq, &payload)
    } else {
        server(seq, &payload)
    }
}
