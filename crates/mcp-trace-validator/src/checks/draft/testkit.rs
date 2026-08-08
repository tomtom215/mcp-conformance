// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Shared fixtures for the `2026-07-28` check tests.
//!
//! The `2025-11-25` modules each carry their own copy of `findings_for`; the
//! draft area has six modules judging one revision, so the helper lives once
//! here and every module's `tests` sibling uses it. Private to [`super`], which
//! its descendants can still reach.

#![allow(clippy::expect_used)]

use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

/// The `_meta` envelope a conforming `2026-07-28` request carries.
pub(super) const META: &str = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;

/// Parses a JSON Lines trace, panicking with the reason if it is malformed.
pub(super) fn events(trace: &str) -> Vec<TraceEvent> {
    parse_trace(trace, &Limits::default()).expect("test trace parses")
}

/// The details one check reports for a trace, in event order.
pub(super) fn findings_for(check: &str, trace: &str) -> Vec<String> {
    let events = events(trace);
    let context = TraceContext::new(&events);
    checks::find(check)
        .expect("check is registered")
        .run(&context)
        .into_iter()
        .map(|finding| finding.detail)
        .collect()
}

/// A client message event.
pub(super) fn client(seq: u64, payload: &str) -> String {
    line(seq, "client-to-server", "message", payload)
}

/// A server message event.
pub(super) fn server(seq: u64, payload: &str) -> String {
    line(seq, "server-to-client", "message", payload)
}

/// One JSON Lines record over Streamable HTTP.
fn line(seq: u64, direction: &str, kind: &str, rest: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"{direction}","transport":"streamable-http","kind":"{kind}","payload":{rest}}}"#
    )
}

/// A client POST's HTTP event, carrying `headers` verbatim as a JSON object.
pub(super) fn post(seq: u64, headers: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"http","headers":{headers}}}"#
    )
}

/// A server response's HTTP event with a status and no headers.
pub(super) fn status(seq: u64, status: u16) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"http","status":{status}}}"#
    )
}

/// A server error response for `id` carrying `code`.
pub(super) fn error(seq: u64, id: &str, code: i64) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":{code},"message":"x"}}}}"#),
    )
}

/// Joins record lines into a trace document.
pub(super) fn trace(lines: &[String]) -> String {
    lines.join("\n")
}
