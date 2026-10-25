// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The schema's `format` annotations: which this engine asserts.
//!
//! Both vendored schemas use three formats — `byte` (image and audio `data`,
//! resource `blob`), `uri` and `uri-template`. `byte` is asserted: the spec's
//! prose says the same thing ("base64-encoded") and the alphabet has one
//! definition, so a value failing it is a wire defect and nothing else. `uri`
//! and `uri-template` stay annotations (ADR-0020): asserting RFC 3986 would
//! fail URIs the prose permits, and the official suite's `uri` check is a
//! regex of its own that the prose does not name.

use mcp_conformance_core::revision::{REVISION_2025_11_25, REVISION_2026_07_28};
use serde_json::json;

use super::{C, S, initialize, judge};
use crate::report::Outcome;

/// A `tools/call` answered with one image whose `data` is `data`.
fn image_result(data: &str) -> Vec<(&'static str, serde_json::Value)> {
    let mut messages = initialize();
    messages.push((
        C,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
               "params": {"name": "render", "arguments": {}}}),
    ));
    messages.push((
        S,
        json!({"jsonrpc": "2.0", "id": 2, "result": {"content": [
            {"type": "image", "data": data, "mimeType": "image/png"}]}}),
    ));
    messages
}

#[test]
fn an_image_whose_data_is_not_base64_fails_the_byte_format() {
    let report = judge(REVISION_2025_11_25, &image_result("not base64!"));
    assert_eq!(report.outcome, Outcome::Fail, "{:#?}", report.findings);
    assert_eq!(report.findings.len(), 1);
    let detail = &report.findings[0].detail;
    assert!(detail.contains("/content/0/data"), "{detail}");
    assert!(detail.contains("byte"), "{detail}");
    assert_eq!(report.findings[0].seq, Some(4));
}

#[test]
fn the_draft_schema_asserts_byte_too() {
    let call = [
        (
            C,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
                "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28",
                          "io.modelcontextprotocol/clientCapabilities": {}},
                "name": "render", "arguments": {}}}),
        ),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 1, "result": {"resultType": "complete", "content": [
                {"type": "image", "data": "not base64!", "mimeType": "image/png"}]}}),
        ),
    ];
    let report = judge(REVISION_2026_07_28, &call);
    assert_eq!(report.outcome, Outcome::Fail, "{:#?}", report.findings);
    assert!(report.findings[0].detail.contains("byte"), "{report:#?}");
}

#[test]
fn base64_with_every_padding_and_the_empty_string_is_valid() {
    // RFC 4648 §4: groups of four, the last padded with `=` or `==`; the
    // empty string encodes nothing and is well-formed (ajv-formats agrees).
    for data in ["AAAA", "AAA=", "AA==", "", "iVBORw0KGgo="] {
        let report = judge(REVISION_2025_11_25, &image_result(data));
        assert_eq!(
            report.outcome,
            Outcome::Pass,
            "{data:?}: {:#?}",
            report.findings
        );
    }
    // Mis-padding and characters outside the alphabet are not.
    for data in ["A===", "AAA", "AA=A", "AAAA\n!!!!", "AAAA AAAA"] {
        let report = judge(REVISION_2025_11_25, &image_result(data));
        assert_eq!(report.outcome, Outcome::Fail, "{data:?} should fail");
    }
}

#[test]
fn uri_and_uri_template_remain_annotations() {
    // Deliberate, and pinned so that asserting them is a decision rather than
    // a side effect of asserting `byte`.
    let mut messages = initialize();
    messages.push((
        C,
        json!({"jsonrpc": "2.0", "id": 2, "method": "resources/templates/list"}),
    ));
    messages.push((
        S,
        json!({"jsonrpc": "2.0", "id": 2, "result": {"resourceTemplates": [
            {"name": "t", "uriTemplate": "not a template {"}]}}),
    ));
    messages.push((
        C,
        json!({"jsonrpc": "2.0", "id": 3, "method": "resources/list"}),
    ));
    messages.push((
        S,
        json!({"jsonrpc": "2.0", "id": 3, "result": {"resources": [
            {"name": "r", "uri": "not a uri"}]}}),
    ));
    let report = judge(REVISION_2025_11_25, &messages);
    assert_eq!(report.outcome, Outcome::Pass, "{:#?}", report.findings);
}
