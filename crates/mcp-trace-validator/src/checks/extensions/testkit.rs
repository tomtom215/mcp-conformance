// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared fixtures for the extension checks' tests: a session written as
//! `(seq, payload)` steps on stdio, judged by one check.

#![allow(clippy::expect_used)]

use serde_json::{Value, json};

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

/// A client request's `params`, with the `_meta` envelope declaring `extensions`.
pub(super) fn params(mut params: Value, extensions: &[&str]) -> Value {
    let declared: serde_json::Map<String, Value> = extensions
        .iter()
        .map(|extension| ((*extension).to_owned(), json!({})))
        .collect();
    params["_meta"] = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {"extensions": declared},
    });
    params
}

/// A client request.
pub(super) fn request(id: u64, method: &str, params: Value) -> (&'static str, Value) {
    let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
    message["params"] = params;
    ("client-to-server", message)
}

/// A server result.
pub(super) fn result(id: u64, result: Value) -> (&'static str, Value) {
    let mut message = json!({"jsonrpc": "2.0", "id": id});
    message["result"] = result;
    ("server-to-client", message)
}

/// A server error.
pub(super) fn error(id: u64, code: i64) -> (&'static str, Value) {
    (
        "server-to-client",
        json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": "x"}}),
    )
}

/// A notification from `direction`.
pub(super) fn notification(
    direction: &'static str,
    method: &str,
    params: Value,
) -> (&'static str, Value) {
    let mut message = json!({"jsonrpc": "2.0", "method": method});
    message["params"] = params;
    (direction, message)
}

/// What `check` reports on the session, as `(subjects, details)`.
pub(super) fn judge(check: &str, steps: &[(&'static str, Value)]) -> (u32, Vec<String>) {
    let document: Vec<String> = steps
        .iter()
        .enumerate()
        .map(|(seq, (direction, payload))| {
            json!({"seq": seq, "direction": direction, "transport": "stdio", "kind": "message",
                   "payload": payload})
            .to_string()
        })
        .collect();
    let events = parse_trace(&document.join("\n"), &Limits::default()).expect("test trace parses");
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
