// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;
use crate::reader::{Limits, parse_trace};

fn events(document: &str) -> Vec<TraceEvent> {
    parse_trace(document, &Limits::default()).unwrap()
}

fn rev(revision: &str) -> ProtocolRevision {
    revision.parse().unwrap()
}

const HANDSHAKE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;

#[test]
fn the_handshake_states_the_revision_from_both_ends() {
    assert_eq!(declared_revisions(&events(HANDSHAKE)), ["2025-11-25"]);
    assert!(mismatch(rev("2025-11-25"), &events(HANDSHAKE)).is_none());
}

#[test]
fn a_stateless_session_states_it_per_request() {
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}"#;
    assert_eq!(declared_revisions(&events(document)), ["2026-07-28"]);
    assert_eq!(
        mismatch(rev("2025-11-25"), &events(document)),
        Some(vec!["2026-07-28".to_owned()])
    );
}

#[test]
fn the_http_header_states_it_too() {
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"2025-11-25"}}"#;
    assert_eq!(declared_revisions(&events(document)), ["2025-11-25"]);
}

#[test]
fn a_session_that_touched_the_registrys_revision_draws_no_note() {
    // Proposed one revision, negotiated another: judging it against either
    // is a fair question, so neither draws a warning.
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
    assert_eq!(
        declared_revisions(&events(document)),
        ["2025-11-25", "2026-07-28"]
    );
    assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
    assert!(mismatch(rev("2026-07-28"), &events(document)).is_none());
}

#[test]
fn a_session_that_declares_nothing_draws_no_note() {
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
    assert!(declared_revisions(&events(document)).is_empty());
    assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
}

#[test]
fn a_malformed_version_is_not_evidence_of_a_revision() {
    // LIFE-006's subject, not this module's: a value that is not a dated
    // revision says nothing about which revision the session belongs to.
    let document = r#"{"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"draft","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
    assert!(declared_revisions(&events(document)).is_empty());
    assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
}

#[test]
fn a_version_this_build_cannot_judge_is_not_a_declaration() {
    // Well-formed, but no registry ships for it, so `--revision 1900-01-01`
    // would be advice with nothing behind it.
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"1900-01-01"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}}"#;
    assert!(declared_revisions(&events(document)).is_empty());
    assert!(mismatch(rev("2025-11-25"), &events(document)).is_none());
}

#[test]
fn a_refused_request_states_no_revision() {
    // The `vers-008` corpus trace: a legacy client's `initialize` reaches a
    // server that no longer implements one. The session ran under no
    // revision, and `VERS-008` is the clause with something to say about
    // it — this module must not add "you used the wrong registry" on top.
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}}"#;
    assert!(declared_revisions(&events(document)).is_empty());
    assert!(mismatch(rev("2026-07-28"), &events(document)).is_none());
}

#[test]
fn a_headers_only_capture_still_states_its_revision() {
    // A recording that began mid-session has no handshake to read, but
    // every request still carries the header.
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"2025-11-25"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"method":"tools/list"}}
{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"result":{"tools":[]}}}"#;
    assert_eq!(declared_revisions(&events(document)), ["2025-11-25"]);
}

#[test]
fn a_refused_request_takes_its_own_header_down_with_it() {
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"mcp-protocol-version":"2025-11-25"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"method":"tools/list"}}
{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":9,"error":{"code":-32022,"message":"Unsupported protocol version"}}}"#;
    assert!(declared_revisions(&events(document)).is_empty());
}

#[test]
fn a_non_string_version_is_ignored_rather_than_stringified() {
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":20251125}}}"#;
    assert!(declared_revisions(&events(document)).is_empty());
}

#[test]
fn declarations_are_deduplicated_and_ordered() {
    let document = format!(
        "{HANDSHAKE}\n{}",
        r#"{"seq":2,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}"#
    );
    assert_eq!(
        declared_revisions(&events(&document)),
        ["2025-11-25", "2026-07-28"]
    );
}

const STATELESS: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}"#;

#[test]
fn select_judges_the_revision_the_trace_declares() {
    let selection = select(BUILTIN_REVISIONS, &events(STATELESS)).unwrap();
    assert_eq!(selection.revisions, [rev("2026-07-28")]);
    assert_eq!(selection.source, RevisionSource::Declared);
    let selection = select(BUILTIN_REVISIONS, &events(HANDSHAKE)).unwrap();
    assert_eq!(selection.revisions, [rev("2025-11-25")]);
}

#[test]
fn select_judges_every_supported_revision_a_session_touched() {
    let document = format!(
        "{HANDSHAKE}\n{}",
        STATELESS
            .replace("\"seq\":0", "\"seq\":2")
            .replace("\"id\":1", "\"id\":2")
    );
    let selection = select(BUILTIN_REVISIONS, &events(&document)).unwrap();
    assert_eq!(selection.revisions, [rev("2025-11-25"), rev("2026-07-28")]);
    assert_eq!(selection.source, RevisionSource::Declared);
}

#[test]
fn select_defaults_to_the_newest_supported_revision_when_nothing_is_declared() {
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
    let selection = select(BUILTIN_REVISIONS, &events(document)).unwrap();
    assert_eq!(selection.revisions, [rev("2026-07-28")]);
    assert_eq!(selection.source, RevisionSource::Default);
    // "Newest" is by date, not by the order the caller listed them in.
    let reversed = [rev("2026-07-28"), rev("2025-11-25")];
    assert_eq!(
        select(&reversed, &events(document)).unwrap().revisions,
        [rev("2026-07-28")]
    );
}

#[test]
fn select_refuses_a_trace_of_only_unsupported_revisions() {
    let document = STATELESS.replace("2026-07-28", "2027-03-01");
    let error = select(BUILTIN_REVISIONS, &events(&document)).unwrap_err();
    assert_eq!(error.declared, [rev("2027-03-01")]);
    assert_eq!(error.supported, BUILTIN_REVISIONS);
    let message = error.to_string();
    assert!(message.contains("2027-03-01"), "{message}");
    assert!(message.contains("2025-11-25, 2026-07-28"), "{message}");
    // Nothing supported at all is the same refusal, not a panic.
    assert!(select(&[], &events(STATELESS)).is_err());
}

#[test]
fn select_ignores_an_unsupported_declaration_beside_a_supported_one() {
    let document = format!(
        "{STATELESS}\n{}",
        STATELESS
            .replace("\"seq\":0", "\"seq\":1")
            .replace("\"id\":1", "\"id\":2")
            .replace("2026-07-28", "2027-03-01")
    );
    let selection = select(BUILTIN_REVISIONS, &events(&document)).unwrap();
    assert_eq!(selection.revisions, [rev("2026-07-28")]);
}

#[test]
fn select_does_not_count_a_refused_request() {
    // The only declaration was refused, so the trace declares nothing.
    let document = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1900-01-01"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32022,"message":"Unsupported protocol version"}}}"#;
    let selection = select(BUILTIN_REVISIONS, &events(document)).unwrap();
    assert_eq!(selection.source, RevisionSource::Default);
}

/// A dual-era client against a legacy Streamable HTTP server: the
/// `server/discover` probe draws an HTTP 400 whose JSON-RPC error carries
/// `id: null`, and the client falls back to `initialize` (the shape the
/// official Python SDK records against the TypeScript everything server).
const DUAL_ERA_FALLBACK: &str = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"accept":"application/json, text/event-stream","content-type":"application/json","mcp-method":"server/discover","mcp-protocol-version":"2026-07-28"}}
{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}}
{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":400,"headers":{"content-type":"application/json"}}
{"seq":3,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":null,"error":{"code":-32000,"message":"Bad Request: Server not initialized"}}}
{"seq":4,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"accept":"application/json, text/event-stream","content-type":"application/json"}}
{"seq":5,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":6,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{"content-type":"application/json","mcp-session-id":"s1"}}
{"seq":7,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":8,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"accept":"application/json, text/event-stream","content-type":"application/json","mcp-session-id":"s1","mcp-protocol-version":"2025-11-25"}}
{"seq":9,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":10,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":202}"#;

#[test]
fn a_failed_discovery_probe_declares_nothing() {
    let trace = events(DUAL_ERA_FALLBACK);
    assert_eq!(
        failed_discover_probes(&trace)
            .into_iter()
            .collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(declared_revisions(&trace), ["2025-11-25"]);

    // Answered with a result, the probe is a 2026-07-28 session's discovery.
    let answered = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#;
    assert!(failed_discover_probes(&events(answered)).is_empty());
    assert_eq!(declared_revisions(&events(answered)), ["2026-07-28"]);
}

#[test]
fn a_dual_era_fallback_is_a_conforming_handshake_session() {
    let registry = mcp_conformance_core::requirement::Registry::builtin_2025_11_25().unwrap();
    let report = crate::engine::validate(&registry, &events(DUAL_ERA_FALLBACK));
    let outcome = |id: &str| {
        report
            .requirements
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.outcome)
            .unwrap()
    };
    assert_eq!(
        outcome("LIFE-001"),
        crate::report::Outcome::Pass,
        "the probe precedes initialize by design"
    );
    assert_eq!(
        outcome("LIFE-004"),
        crate::report::Outcome::Pass,
        "the probe is not a request in the window"
    );
    assert_eq!(report.totals.fail, 0, "{report:#?}");
}

#[test]
fn a_probe_is_answered_only_by_a_response_carrying_its_own_id() {
    // The server's next response answers an earlier request, not the probe;
    // the probe's own answer is an error, so the probe failed.
    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":5,"method":"ping"}}
{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{}}}
{"seq":2,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":5,"result":{}}}
{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}}"#;
    assert_eq!(
        failed_discover_probes(&events(trace))
            .into_iter()
            .collect::<Vec<_>>(),
        [1]
    );
}
