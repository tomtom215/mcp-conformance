// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::trace::TraceEvent;

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

const CHECK: &str = "transport.unsupported-version-header-rejected";

/// The findings and subject count the check reports on `lines`.
fn run(lines: &[String]) -> (Vec<String>, u32) {
    let document = lines.join("\n");
    let events: Vec<TraceEvent> = parse_trace(&document, &Limits::default()).unwrap();
    let context = TraceContext::new(&events);
    let outcome = checks::find(CHECK).unwrap().run(&context);
    (
        outcome
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect(),
        outcome.subjects,
    )
}

fn event(seq: u64, direction: &str, rest: &str) -> String {
    format!(r#"{{"seq":{seq},"direction":"{direction}","transport":"streamable-http",{rest}}}"#)
}

/// A handshake where the client asks for `requested` and the server answers
/// `negotiated`, then a `ping` POST with `header`, answered `status`.
fn session(requested: &str, negotiated: &str, header: &str, status: u16) -> Vec<String> {
    vec![
        event(
            0,
            "client-to-server",
            r#""kind":"http","method":"POST","headers":{}"#,
        ),
        event(
            1,
            "client-to-server",
            &format!(
                r#""kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{requested}","capabilities":{{}},"clientInfo":{{"name":"c","version":"0"}}}}}}"#
            ),
        ),
        event(
            2,
            "server-to-client",
            r#""kind":"http","status":200,"headers":{"content-type":"application/json"}"#,
        ),
        event(
            3,
            "server-to-client",
            &format!(
                r#""kind":"message","payload":{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"{negotiated}","capabilities":{{}},"serverInfo":{{"name":"s","version":"0"}}}}}}"#
            ),
        ),
        event(
            4,
            "client-to-server",
            &format!(
                r#""kind":"http","method":"POST","headers":{{"mcp-protocol-version":"{header}"}}"#
            ),
        ),
        event(
            5,
            "client-to-server",
            r#""kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
        ),
        event(
            6,
            "server-to-client",
            &format!(
                r#""kind":"http","status":{status},"headers":{{"content-type":"application/json"}}"#
            ),
        ),
    ]
}

#[test]
fn the_declined_version_must_draw_400() {
    let (findings, subjects) = run(&session("2099-01-01", "2025-11-25", "2099-01-01", 200));
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("HTTP 200"), "{findings:?}");

    let (findings, subjects) = run(&session("2099-01-01", "2025-11-25", "2099-01-01", 400));
    assert_eq!((findings.len(), subjects), (0, 1));
}

#[test]
fn only_the_declined_version_is_judged() {
    // The negotiated version, and a version the trace never discussed, are
    // not known to be unsupported.
    for header in ["2025-11-25", "2025-06-18"] {
        let (findings, subjects) = run(&session("2099-01-01", "2025-11-25", header, 200));
        assert_eq!((findings.len(), subjects), (0, 0), "{header}");
    }
    // A server that accepted the requested version declined nothing.
    let (findings, subjects) = run(&session("2025-11-25", "2025-11-25", "2025-11-25", 200));
    assert_eq!((findings.len(), subjects), (0, 0));
}

#[test]
fn a_notification_post_is_not_a_request() {
    let mut lines = session("2099-01-01", "2025-11-25", "2099-01-01", 202);
    lines[5] = event(
        5,
        "client-to-server",
        r#""kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    );
    let (findings, subjects) = run(&lines);
    assert_eq!((findings.len(), subjects), (0, 0));
}
