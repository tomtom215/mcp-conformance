// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared fixtures for the error-handling check tests.

#![allow(clippy::expect_used)]

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

/// What one check made of a trace: how many subjects it examined, and the
/// detail of each finding in event order.
pub(super) fn run(check: &str, lines: &[String]) -> (u32, Vec<String>) {
    let events = parse_trace(&lines.join("\n"), &Limits::default()).expect("test trace parses");
    let context = TraceContext::new(&events);
    let outcome = checks::find(check)
        .expect("check is registered")
        .run(&context);
    (
        outcome.subjects,
        outcome
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect(),
    )
}

/// A client message event over stdio.
pub(super) fn client(seq: u64, payload: &str) -> String {
    line(seq, "client-to-server", payload)
}

/// A server message event over stdio.
pub(super) fn server(seq: u64, payload: &str) -> String {
    line(seq, "server-to-client", payload)
}

fn line(seq: u64, direction: &str, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
    )
}

/// A client request `id` for `method` with `params` (a JSON object's text).
pub(super) fn request(seq: u64, id: u64, method: &str, params: &str) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#),
    )
}

/// A server result for `id`.
pub(super) fn result(seq: u64, id: u64, result: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{result}}}"#),
    )
}

/// A server error for `id` carrying `code` (any JSON value's text).
pub(super) fn error(seq: u64, id: u64, code: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":{code},"message":"x"}}}}"#),
    )
}

/// The `2025-11-25` handshake (seq 0–2, request id 1) of a server declaring
/// `capabilities` (a JSON object's text).
pub(super) fn handshake(capabilities: &str) -> Vec<String> {
    vec![
        request(
            0,
            1,
            "initialize",
            r#"{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}"#,
        ),
        result(
            1,
            1,
            &format!(
                r#"{{"protocolVersion":"2025-11-25","capabilities":{capabilities},"serverInfo":{{"name":"s","version":"0"}}}}"#
            ),
        ),
        client(
            2,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ),
    ]
}

/// `base` followed by `more`.
pub(super) fn with(mut base: Vec<String>, more: &[String]) -> Vec<String> {
    base.extend_from_slice(more);
    base
}
