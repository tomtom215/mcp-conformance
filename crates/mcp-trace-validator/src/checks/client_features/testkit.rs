// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared fixtures for the client-feature check tests: both revisions' ways of
//! asking, so each test can hold one check to both.

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

/// One stdio message event.
fn line(seq: u64, direction: &str, payload: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
    )
}

/// A client message.
pub(super) fn client(seq: u64, payload: &str) -> String {
    line(seq, "client-to-server", payload)
}

/// A server message.
pub(super) fn server(seq: u64, payload: &str) -> String {
    line(seq, "server-to-client", payload)
}

/// Joins records into a trace document.
pub(super) fn trace(lines: &[String]) -> String {
    lines.join("\n")
}

/// A `2025-11-25` handshake whose client declares `capabilities`, at seq 0–2.
pub(super) fn handshake(capabilities: &str) -> Vec<String> {
    vec![
        client(
            0,
            &format!(
                r#"{{"jsonrpc":"2.0","id":0,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{capabilities},"clientInfo":{{"name":"c","version":"1"}}}}}}"#
            ),
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"1"}}}"#,
        ),
        client(
            2,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ),
    ]
}

/// A `2025-11-25` server request `method` with id `"s{id}"` and `params`.
pub(super) fn ask(seq: u64, id: u64, method: &str, params: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":"s{id}","method":"{method}","params":{params}}}"#),
    )
}

/// The client's result for server request `"s{id}"`.
pub(super) fn reply(seq: u64, id: u64, result: &str) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":"s{id}","result":{result}}}"#),
    )
}

/// The client's error for server request `"s{id}"`.
pub(super) fn reply_error(seq: u64, id: u64, code: i64) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":"s{id}","error":{{"code":{code},"message":"x"}}}}"#),
    )
}

/// A `2025-11-25` trace: the handshake, then `rest`.
pub(super) fn session(capabilities: &str, rest: &[String]) -> String {
    let mut lines = handshake(capabilities);
    lines.extend_from_slice(rest);
    trace(&lines)
}

/// The `_meta` a `2026-07-28` request carries, declaring `capabilities`.
fn meta(capabilities: &str) -> String {
    format!(
        r#""_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{capabilities},"io.modelcontextprotocol/clientInfo":{{"name":"c","version":"1"}}}}"#
    )
}

/// A `2026-07-28` `tools/call` of tool `t` with `id`, declaring
/// `capabilities`, with `extra` params (e.g. `"inputResponses":{…},`).
pub(super) fn call(seq: u64, id: u64, capabilities: &str, extra: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"t","arguments":{{}},{extra}{}}}}}"#,
            meta(capabilities)
        ),
    )
}

/// An `input_required` result for request `id`, asking `requests` (the
/// `inputRequests` object's body) with state `"st"`.
pub(super) fn input_required(seq: u64, id: u64, requests: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"input_required","inputRequests":{{{requests}}},"requestState":"st"}}}}"#
        ),
    )
}

/// A `2026-07-28` round: a call declaring `capabilities`, asked `requests`,
/// retried with `responses` (the `inputResponses` object's body) declaring
/// `retry_capabilities`.
pub(super) fn round(
    capabilities: &str,
    requests: &str,
    responses: &str,
    retry_capabilities: &str,
) -> String {
    trace(&[
        call(0, 1, capabilities, ""),
        input_required(1, 1, requests),
        call(
            2,
            2,
            retry_capabilities,
            &format!(r#""inputResponses":{{{responses}}},"requestState":"st","#),
        ),
    ])
}

/// A `2026-07-28` call declaring `capabilities`, answered by an
/// `input_required` result asking `requests`, and no retry.
pub(super) fn asked(capabilities: &str, requests: &str) -> String {
    trace(&[call(0, 1, capabilities, ""), input_required(1, 1, requests)])
}
