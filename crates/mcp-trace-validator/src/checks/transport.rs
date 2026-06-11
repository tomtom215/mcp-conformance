// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` transport requirements (`TRAN-*`).
//!
//! stdio checks judge message validity per direction; Streamable HTTP checks judge
//! the security-relevant headers recorded on [`EventBody::Http`] events — session IDs
//! and the `MCP-Protocol-Version` header. HTTP request events are client-to-server
//! `Http` observations; response events are server-to-client ones.
//!
//! [`EventBody::Http`]: mcp_conformance_core::trace::EventBody

use std::collections::BTreeMap;

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::{Direction, EventBody, TransportKind};

use super::FindingSink;
use crate::context::TraceContext;

/// `TRAN-004`: nothing on the server's stdout that is not a valid MCP message.
pub(super) fn stdio_server_output_valid(context: &TraceContext<'_>, sink: &mut FindingSink) {
    stdio_messages_valid(context, sink, Direction::ServerToClient, "stdout");
}

/// `TRAN-005`: nothing on the server's stdin that is not a valid MCP message.
pub(super) fn stdio_client_input_valid(context: &TraceContext<'_>, sink: &mut FindingSink) {
    stdio_messages_valid(context, sink, Direction::ClientToServer, "stdin");
}

fn stdio_messages_valid(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    direction: Direction,
    stream: &str,
) {
    for (event, kind, _) in context.messages() {
        if event.transport != TransportKind::Stdio || event.direction != direction {
            continue;
        }
        if let MessageKind::Invalid { reason } = kind {
            sink.push(
                Some(event.seq),
                format!("message on {stream} is not a valid MCP message: {reason}"),
            );
        }
    }
}

/// The headers of every HTTP event in the given direction, in trace order.
fn http_headers<'a>(
    context: &TraceContext<'a>,
    direction: Direction,
) -> impl Iterator<Item = (u64, &'a BTreeMap<String, String>)> {
    context
        .events()
        .iter()
        .filter(move |event| event.direction == direction)
        .filter_map(|event| match &event.body {
            EventBody::Http { headers, .. } => Some((event.seq, headers)),
            _ => None,
        })
}

/// `TRAN-011`: session IDs must contain only visible ASCII (0x21–0x7E).
pub(super) fn session_id_visible_ascii(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, headers) in http_headers(context, Direction::ServerToClient) {
        if let Some(session_id) = headers.get("mcp-session-id")
            && !session_id.bytes().all(|byte| (0x21..=0x7E).contains(&byte))
        {
            sink.push(
                    Some(seq),
                    format!(
                        "session ID {session_id:?} contains characters outside visible ASCII (0x21-0x7E)"
                    ),
                );
        }
    }
}

/// The first server-assigned session ID in the trace, with the seq it appeared at.
fn assigned_session_id<'a>(context: &TraceContext<'a>) -> Option<(u64, &'a str)> {
    http_headers(context, Direction::ServerToClient).find_map(|(seq, headers)| {
        headers
            .get("mcp-session-id")
            .map(|session_id| (seq, session_id.as_str()))
    })
}

/// `TRAN-013`: once the server returns an `MCP-Session-Id`, every subsequent client
/// HTTP request must carry it.
pub(super) fn session_id_echoed(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((assigned_seq, session_id)) = assigned_session_id(context) else {
        return;
    };
    for (seq, headers) in http_headers(context, Direction::ClientToServer) {
        if seq < assigned_seq {
            continue;
        }
        match headers.get("mcp-session-id") {
            None => sink.push(
                Some(seq),
                format!(
                    "client HTTP request lacks the MCP-Session-Id header; the server assigned {session_id:?} at seq {assigned_seq}"
                ),
            ),
            Some(echoed) if echoed != session_id => sink.push(
                Some(seq),
                format!(
                    "client echoed session ID {echoed:?}, but the server assigned {session_id:?}"
                ),
            ),
            Some(_) => {}
        }
    }
}

/// `TRAN-017`: after initialization, every client HTTP request must carry
/// `MCP-Protocol-Version`.
pub(super) fn protocol_version_header(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((negotiated_seq, _)) = negotiated_version(context) else {
        return;
    };
    for (seq, headers) in http_headers(context, Direction::ClientToServer) {
        if seq <= negotiated_seq {
            continue; // Initialization traffic itself precedes "subsequent requests".
        }
        if !headers.contains_key("mcp-protocol-version") {
            sink.push(
                Some(seq),
                "client HTTP request after initialization lacks the MCP-Protocol-Version header"
                    .to_owned(),
            );
        }
    }
}

/// `TRAN-018`: the protocol version the client sends should be the negotiated one.
pub(super) fn protocol_version_negotiated(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some((negotiated_seq, negotiated)) = negotiated_version(context) else {
        return;
    };
    for (seq, headers) in http_headers(context, Direction::ClientToServer) {
        if seq <= negotiated_seq {
            continue;
        }
        if let Some(sent) = headers.get("mcp-protocol-version")
            && sent != negotiated
        {
            sink.push(
                    Some(seq),
                    format!(
                        "client sent MCP-Protocol-Version {sent:?}, but {negotiated:?} was negotiated at seq {negotiated_seq}"
                    ),
                );
        }
    }
}

/// The protocol version the server's `initialize` result stated, when present and
/// well-typed, with the seq it was negotiated at.
fn negotiated_version<'a>(context: &TraceContext<'a>) -> Option<(u64, &'a str)> {
    let (seq, result) = context.initialize().result?;
    let version = result.get("protocolVersion")?.as_str()?;
    Some((seq, version))
}

/// `TRAN-025`/`TRAN-039`: every client HTTP request to the MCP endpoint must
/// carry an `Accept` header listing `text/event-stream`.
///
/// This enforces the floor the two clauses share. POST requests must list
/// both `application/json` and `text/event-stream`; GET requests must list
/// `text/event-stream`. Recorded events carry no request method, so the
/// POST-only half (`application/json` present) is not separately enforceable
/// offline — but no request form may omit `text/event-stream`, so flagging
/// that omission is sound for every recorded request.
pub(super) fn client_accept_header(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, headers) in http_headers(context, Direction::ClientToServer) {
        match headers.get("accept") {
            None => sink.push(
                Some(seq),
                "client HTTP request has no Accept header; every request to the MCP \
                 endpoint must list text/event-stream (and POST requests application/json)"
                    .to_owned(),
            ),
            Some(accept) if !accept.to_ascii_lowercase().contains("text/event-stream") => sink
                .push(
                    Some(seq),
                    format!("client Accept header {accept:?} does not list text/event-stream"),
                ),
            Some(_) => {}
        }
    }
}

/// `TRAN-029`/`TRAN-040`: an HTTP 200 from the MCP endpoint must declare
/// `Content-Type: application/json` or `Content-Type: text/event-stream`.
///
/// Both clauses demand one of the same two content types for their success
/// case (POST answering a request; GET opening a stream), so the check is
/// sound without knowing the request method. Non-200 paths (202 accepted,
/// error statuses, 405) carry no such obligation and are not examined.
pub(super) fn success_content_type(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for event in context.events() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        let EventBody::Http {
            status: Some(200),
            headers,
        } = &event.body
        else {
            continue;
        };
        match headers.get("content-type") {
            None => sink.push(
                Some(event.seq),
                "HTTP 200 response carries no Content-Type header; the MCP endpoint \
                 must answer with application/json or text/event-stream"
                    .to_owned(),
            ),
            Some(content_type) => {
                let lowered = content_type.to_ascii_lowercase();
                if !lowered.contains("application/json") && !lowered.contains("text/event-stream") {
                    sink.push(
                        Some(event.seq),
                        format!(
                            "HTTP 200 Content-Type {content_type:?} is neither application/json \
                             nor text/event-stream"
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    use mcp_conformance_core::trace::TraceEvent;

    fn findings_for(check: &str, trace: &str) -> Vec<String> {
        let events: Vec<TraceEvent> = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check)
            .unwrap()
            .run(&context)
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    /// An HTTP session: initialize exchange with headers, then one more request.
    fn http_session(response_headers: &str, followup_request_headers: &str) -> String {
        [
            r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","headers":{"accept":"application/json, text/event-stream"}}"#.to_owned(),
            r#"{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#.to_owned(),
            format!(r#"{{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{response_headers}}}"#),
            r#"{"seq":3,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#.to_owned(),
            format!(r#"{{"seq":4,"direction":"client-to-server","transport":"streamable-http","kind":"http","headers":{followup_request_headers}}}"#),
            r#"{"seq":5,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#.to_owned(),
        ]
        .join("\n")
    }

    #[test]
    fn session_id_checks_judge_assignment_and_echo() {
        // Correct echo: no findings from either check.
        let good = http_session(
            r#"{"mcp-session-id":"abc123"}"#,
            r#"{"mcp-session-id":"abc123","mcp-protocol-version":"2025-11-25"}"#,
        );
        assert!(findings_for("transport.session-id-visible-ascii", &good).is_empty());
        assert!(findings_for("transport.session-id-echoed", &good).is_empty());

        // Wrong echo value.
        let wrong = http_session(
            r#"{"mcp-session-id":"abc123"}"#,
            r#"{"mcp-session-id":"zzz","mcp-protocol-version":"2025-11-25"}"#,
        );
        let findings = findings_for("transport.session-id-echoed", &wrong);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("\"zzz\""), "{findings:?}");

        // No session assigned: the echo check abstains entirely.
        let none = http_session("{}", "{}");
        assert!(findings_for("transport.session-id-echoed", &none).is_empty());
    }

    #[test]
    fn capitalized_header_names_are_still_judged() {
        // On-the-wire casing must not let a bad header slip past: a session ID
        // with an illegal space, recorded under the capitalized `Mcp-Session-Id`,
        // is still caught because trace deserialization lowercases header keys.
        let bad = http_session(r#"{"Mcp-Session-Id":"has space"}"#, "{}");
        assert_eq!(
            findings_for("transport.session-id-visible-ascii", &bad).len(),
            1,
            "capitalized header name evaded the visible-ASCII check"
        );
        // And a wrong protocol version under `Mcp-Protocol-Version` is flagged
        // by TRAN-018 rather than silently passing.
        let wrong_version = http_session(
            r#"{"mcp-session-id":"abc"}"#,
            r#"{"mcp-session-id":"abc","Mcp-Protocol-Version":"2024-11-05"}"#,
        );
        let findings = findings_for("transport.protocol-version-negotiated", &wrong_version);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("2024-11-05"), "{findings:?}");
    }

    #[test]
    fn session_id_ascii_boundaries_are_exact() {
        // 0x20 (space) and non-ASCII are out; 0x21 and 0x7E are in.
        let bad = http_session(r#"{"mcp-session-id":"has space"}"#, "{}");
        assert_eq!(
            findings_for("transport.session-id-visible-ascii", &bad).len(),
            1
        );
        let edges = http_session(r#"{"mcp-session-id":"!~"}"#, r#"{"mcp-session-id":"!~"}"#);
        assert!(findings_for("transport.session-id-visible-ascii", &edges).is_empty());
    }

    #[test]
    fn protocol_version_checks_scope_to_requests_after_negotiation() {
        // The pre-initialize request (seq 0) must not be flagged; the follow-up
        // without the header must be.
        let missing = http_session("{}", r#"{"mcp-session-id":"x"}"#);
        let findings = findings_for("transport.protocol-version-header", &missing);
        assert_eq!(findings.len(), 1, "{findings:?}");

        let mismatched = http_session("{}", r#"{"mcp-protocol-version":"2024-11-05"}"#);
        let findings = findings_for("transport.protocol-version-negotiated", &mismatched);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("2024-11-05"), "{findings:?}");
        // The mismatched header satisfies presence.
        assert!(findings_for("transport.protocol-version-header", &mismatched).is_empty());
    }

    #[test]
    fn stdio_validity_checks_are_direction_scoped() {
        let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"hello":"world"}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":[1,2,3]}"#;
        let server = findings_for("transport.stdio-server-output-valid", trace);
        assert_eq!(server.len(), 1, "{server:?}");
        assert!(server[0].contains("stdout"), "{server:?}");
        let client = findings_for("transport.stdio-client-input-valid", trace);
        assert_eq!(client.len(), 1, "{client:?}");
        assert!(client[0].contains("not a JSON object"), "{client:?}");
    }

    #[test]
    fn client_accept_header_requires_event_stream_on_every_request() {
        // Followup request with no Accept at all.
        let missing = http_session("{}", r#"{"mcp-protocol-version":"2025-11-25"}"#);
        let findings = findings_for("transport.client-accept-header", &missing);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("no Accept header"), "{findings:?}");

        // Accept present but without text/event-stream.
        let json_only = http_session("{}", r#"{"accept":"application/json"}"#);
        let findings = findings_for("transport.client-accept-header", &json_only);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("text/event-stream"), "{findings:?}");

        // Either order and extra parameters are fine; matching is case-insensitive.
        let fine = http_session(
            "{}",
            r#"{"accept":"TEXT/EVENT-STREAM;q=0.9, application/json"}"#,
        );
        assert!(findings_for("transport.client-accept-header", &fine).is_empty());
    }

    #[test]
    fn success_content_type_judges_only_200_responses() {
        // 200 with a content type outside the two allowed.
        let html = http_session(r#"{"content-type":"text/html"}"#, "{}");
        let findings = findings_for("transport.success-content-type", &html);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("text/html"), "{findings:?}");

        // 200 with no content type at all.
        let none = http_session("{}", "{}");
        let findings = findings_for("transport.success-content-type", &none);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("no Content-Type"), "{findings:?}");

        // SSE and JSON answers both pass; a 202 is not examined.
        for ok in [
            r#"{"content-type":"text/event-stream"}"#,
            r#"{"content-type":"application/json; charset=utf-8"}"#,
        ] {
            assert!(
                findings_for("transport.success-content-type", &http_session(ok, "{}")).is_empty(),
                "{ok}"
            );
        }
        let accepted = r#"{"seq":0,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":202}"#;
        assert!(findings_for("transport.success-content-type", accepted).is_empty());
    }
}
