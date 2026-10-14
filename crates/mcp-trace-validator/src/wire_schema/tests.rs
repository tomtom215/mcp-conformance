// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_conformance_core::revision::{ProtocolRevision, REVISION_2025_11_25, REVISION_2026_07_28};
use mcp_conformance_core::wire_schema::for_revision;
use serde_json::{Value, json};

use super::{MAX_ERRORS, check};
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};
use crate::report::{Outcome, WireSchemaReport};

/// One trace line per `(direction, payload)`, numbered from 0.
fn trace(messages: &[(&str, Value)]) -> String {
    messages
        .iter()
        .enumerate()
        .map(|(seq, (direction, payload))| {
            json!({
                "seq": seq, "direction": direction, "transport": "stdio",
                "kind": "message", "payload": payload,
            })
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn judge(revision: ProtocolRevision, messages: &[(&str, Value)]) -> WireSchemaReport {
    let events = parse_trace(&trace(messages), &Limits::default()).unwrap();
    check(revision, &TraceContext::new(&events)).unwrap()
}

const C: &str = "client-to-server";
const S: &str = "server-to-client";

fn initialize() -> Vec<(&'static str, Value)> {
    vec![
        (
            C,
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {"elicitation": {}},
            "clientInfo": {"name": "c", "version": "1"}}}),
        ),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 1, "result": {
            "protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
            "serverInfo": {"name": "s", "version": "1"}}}),
        ),
        (
            C,
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ),
    ]
}

#[test]
fn a_conforming_session_passes_and_counts_every_message() {
    let report = judge(REVISION_2025_11_25, &initialize());
    assert_eq!(report.outcome, Outcome::Pass, "{:#?}", report.findings);
    assert_eq!(report.messages, 3);
    assert!(report.schema.ends_with("/schema/2025-11-25/schema.json"));
    assert_eq!(report.source.section, "basic#schema");
}

#[test]
fn a_result_missing_a_required_member_fails_at_its_path() {
    let mut messages = initialize();
    messages[1].1["result"]
        .as_object_mut()
        .unwrap()
        .remove("serverInfo");
    let report = judge(REVISION_2025_11_25, &messages);
    assert_eq!(report.outcome, Outcome::Fail);
    assert_eq!(report.findings.len(), 1);
    let finding = &report.findings[0];
    assert_eq!(
        (finding.check.as_str(), finding.seq),
        ("wire-schema", Some(1))
    );
    assert_eq!(
        finding.detail,
        "the server's message is not a valid InitializeResult under the 2025-11-25 schema: \
         /result: \"serverInfo\" is a required property"
    );
}

#[test]
fn a_wrong_type_and_an_enum_violation_are_both_reported() {
    let messages = [(
        S,
        json!({"jsonrpc": "2.0", "method": "notifications/message",
        "params": {"level": "chatty", "data": "x", "logger": 7}}),
    )];
    let report = judge(REVISION_2025_11_25, &messages);
    let detail = &report.findings[0].detail;
    assert!(detail.contains("LoggingMessageNotification"), "{detail}");
    assert!(detail.contains("/params/level"), "{detail}");
    assert!(
        detail.contains("/params/logger: 7 is not of type \"string\""),
        "{detail}"
    );
}

/// The erratum the core crate records: `schema.ts` admits a float answer to a
/// `NumberSchema` field; the upstream JSON Schema says `integer`.
#[test]
fn a_float_in_an_elicitation_answer_is_valid_as_schema_ts_says() {
    let mut messages = initialize();
    messages.push((
        S,
        json!({"jsonrpc": "2.0", "id": 9, "method": "elicitation/create",
        "params": {"mode": "form", "message": "score?", "requestedSchema": {"type": "object",
            "properties": {"score": {"type": "number"}}}}}),
    ));
    messages.push((
        C,
        json!({"jsonrpc": "2.0", "id": 9,
        "result": {"action": "accept", "content": {"score": 95.5}}}),
    ));
    let report = judge(REVISION_2025_11_25, &messages);
    assert_eq!(report.outcome, Outcome::Pass, "{:#?}", report.findings);

    // Without the erratum the same answer fails: the correction is load-bearing.
    let upstream: Value =
        serde_json::from_str(for_revision(REVISION_2025_11_25).unwrap().json).unwrap();
    let validator = jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build_map(&upstream)
        .unwrap();
    let elicit = validator.get("#/$defs/ElicitResult").unwrap();
    assert!(!elicit.is_valid(&json!({"action": "accept", "content": {"score": 95.5}})));
    assert!(elicit.is_valid(&json!({"action": "accept", "content": {"score": 95}})));
}

/// The `2026-07-28` erratum: `JSONValue` admits `null` and non-integers.
#[test]
fn null_and_float_json_values_are_valid_at_2026_07_28() {
    let messages = [(
        C,
        json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {
        "_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "c", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities":
                {"experimental": {"x": {"ratio": 0.5, "none": null}}}}}}),
    )];
    let report = judge(REVISION_2026_07_28, &messages);
    assert_eq!(report.outcome, Outcome::Pass, "{:#?}", report.findings);
}

#[test]
fn an_input_required_result_is_held_to_its_own_definition() {
    let messages = [
        (
            C,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {"_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "c", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities": {}}}}),
        ),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 1, "result": {"resultType": "input_required", "requestState": 7}}),
        ),
    ];
    let report = judge(REVISION_2026_07_28, &messages);
    assert_eq!(report.outcome, Outcome::Fail);
    assert_eq!(report.findings.len(), 1, "{:#?}", report.findings);
    assert_eq!(report.findings[0].seq, Some(1));
    assert!(
        report.findings[0]
            .detail
            .contains("not a valid InputRequiredResult"),
        "{:?}",
        report.findings
    );
}

#[test]
fn a_task_augmented_call_is_answered_by_a_create_task_result() {
    let mut messages = initialize();
    messages.push((
        C,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "t", "arguments": {}, "task": {"ttl": 1000}}}),
    ));
    messages.push((
        S,
        json!({"jsonrpc": "2.0", "id": 2, "result": {"task": {
        "taskId": "a", "status": "working", "createdAt": "2026-10-13T00:00:00Z",
        "lastUpdatedAt": "2026-10-13T00:00:00Z", "ttl": 1000}}}),
    ));
    let report = judge(REVISION_2025_11_25, &messages);
    assert_eq!(report.outcome, Outcome::Pass, "{:#?}", report.findings);
}

/// Where the schema names no definition, only the envelope is judged — a
/// missed finding rather than a false one.
#[test]
fn unknown_methods_codes_and_unpaired_results_meet_the_envelope_only() {
    let messages = [
        (
            C,
            json!({"jsonrpc": "2.0", "id": 1, "method": "x-vendor/thing", "params": {"a": 1}}),
        ),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 1, "error": {"code": -31999, "message": "no"}}),
        ),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 77, "result": {"anything": [1, 2]}}),
        ),
        (C, json!({"jsonrpc": "2.0", "method": "x-vendor/note"})),
    ];
    let report = judge(REVISION_2025_11_25, &messages);
    assert_eq!(report.outcome, Outcome::Pass, "{:#?}", report.findings);
    assert_eq!(report.messages, 4);
}

#[test]
fn a_batch_is_not_a_message_at_either_revision() {
    for revision in [REVISION_2025_11_25, REVISION_2026_07_28] {
        let messages = [(
            C,
            json!([{"jsonrpc": "2.0", "method": "notifications/initialized"}]),
        )];
        let report = judge(revision, &messages);
        assert_eq!(report.outcome, Outcome::Fail, "{revision}");
        assert!(report.findings[0].detail.contains("JSONRPCMessage"));
    }
}

#[test]
fn many_errors_are_capped_and_counted() {
    let arguments: serde_json::Map<String, Value> =
        (0..10).map(|i| (format!("k{i}"), json!(i))).collect();
    let messages = [(
        C,
        json!({"jsonrpc": "2.0", "id": 1, "method": "prompts/get",
        "params": {"name": 5, "arguments": arguments}}),
    )];
    let detail = &judge(REVISION_2025_11_25, &messages).findings[0].detail;
    // Eleven errors, sorted by path: the first six listed, the other five counted.
    assert_eq!(
        detail.matches(" is not of type ").count(),
        MAX_ERRORS,
        "{detail}"
    );
    assert!(detail.ends_with("; and 5 more"), "{detail}");
    assert!(
        detail.contains("/params/arguments/k0: 0 is not of type \"string\""),
        "{detail}"
    );
}

#[test]
fn an_overlong_error_is_cut() {
    let long = "x".repeat(5_000);
    let messages = [(
        S,
        json!({"jsonrpc": "2.0", "method": "notifications/message",
        "params": {"level": long, "data": "x"}}),
    )];
    let detail = &judge(REVISION_2025_11_25, &messages).findings[0].detail;
    assert!(detail.len() < 600, "{} bytes", detail.len());
    assert!(detail.contains('…'), "{detail}");
}

#[test]
fn a_trace_without_messages_is_not_observed_and_an_unvendored_revision_has_no_report() {
    let events = parse_trace(
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"lifecycle","event":"transport-open"}"#,
        &Limits::default(),
    )
    .unwrap();
    let context = TraceContext::new(&events);
    assert_eq!(
        check(REVISION_2025_11_25, &context).unwrap().outcome,
        Outcome::NotObserved
    );
    assert!(check(ProtocolRevision::new(2025, 6, 18).unwrap(), &context).is_none());
}

#[test]
fn findings_are_identical_across_runs() {
    let mut messages = initialize();
    messages[1].1["result"] = json!({"capabilities": 3});
    let first = judge(REVISION_2025_11_25, &messages);
    for _ in 0..3 {
        assert_eq!(judge(REVISION_2025_11_25, &messages), first);
    }
}

/// An `anyOf` names no defect; the report names the closest branch's instead.
#[test]
fn a_failed_union_reports_its_closest_branch() {
    let mut messages = initialize();
    messages.push((
        C,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": {"name": "t", "arguments": {}}}),
    ));
    messages.push((
        S,
        json!({"jsonrpc": "2.0", "id": 2,
        "result": {"content": [{"type": "txt", "text": "hi"}]}}),
    ));
    let detail = &judge(REVISION_2025_11_25, &messages).findings[0].detail;
    assert!(
        detail.ends_with(
            "/result/content/0: matches none of 5 alternatives; closest: \
             /result/content/0/type: \"text\" was expected"
        ),
        "{detail}"
    );
}

/// A branch whose discriminator differs is another variant, not a near miss:
/// a form elicitation missing `requestedSchema` is not "closest to `roots/list`".
#[test]
fn a_branch_with_another_discriminator_is_never_the_closest() {
    let messages = [
        (
            C,
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientInfo": {"name": "c", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities": {"elicitation": {}}},
            "name": "t", "arguments": {}}}),
        ),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 1, "result": {"resultType": "input_required",
            "inputRequests": {"login": {"method": "elicitation/create",
                "params": {"mode": "form", "message": "name?"}}}}}),
        ),
    ];
    let detail = &judge(REVISION_2026_07_28, &messages).findings[0].detail;
    assert!(
        detail.contains("\"requestedSchema\" is a required property"),
        "{detail}"
    );
    assert!(!detail.contains("roots/list"), "{detail}");
}

#[test]
fn exactly_the_listed_number_of_errors_counts_no_rest() {
    let errors: Vec<String> = (0..MAX_ERRORS).map(|i| format!("e{i}")).collect();
    assert_eq!(super::summarize(&errors), "e0; e1; e2; e3; e4; e5");
    let more: Vec<String> = (0..=MAX_ERRORS).map(|i| format!("e{i}")).collect();
    assert_eq!(
        super::summarize(&more),
        "e0; e1; e2; e3; e4; e5; and 1 more"
    );
}

#[test]
fn an_unknown_method_with_an_id_is_a_request_and_without_one_a_notification() {
    let detail = |message: Value| {
        judge(REVISION_2025_11_25, &[(C, message)])
            .findings
            .first()
            .map(|finding| finding.detail.clone())
            .unwrap_or_default()
    };
    let request = detail(json!({"jsonrpc": "2.0", "id": null, "method": "x-vendor/thing"}));
    assert!(
        request.starts_with("the client's message is not a valid JSONRPCRequest "),
        "{request}"
    );
    let notification = detail(json!({"jsonrpc": "2.0", "method": "x-vendor/thing", "params": 5}));
    assert!(
        notification.starts_with("the client's message is not a valid JSONRPCNotification "),
        "{notification}"
    );
}

/// `2025-11-25` defines no `InputRequiredResult`, so a result claiming to be
/// one is still held to its method's result.
#[test]
fn a_definition_the_revision_lacks_is_never_the_one_judged() {
    let mut messages = initialize();
    messages.extend([
        (C, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})),
        (
            S,
            json!({"jsonrpc": "2.0", "id": 2, "result": {"resultType": "input_required", "tools": 5}}),
        ),
    ]);
    let report = judge(REVISION_2025_11_25, &messages);
    assert_eq!(report.findings.len(), 1, "{:#?}", report.findings);
    assert!(
        report.findings[0]
            .detail
            .starts_with("the server's message is not a valid ListToolsResult "),
        "{:#?}",
        report.findings
    );
}

#[test]
fn only_the_vendored_revisions_are_covered() {
    assert!(super::covers(REVISION_2025_11_25));
    assert!(super::covers(REVISION_2026_07_28));
    assert!(!super::covers(ProtocolRevision::new(2025, 6, 18).unwrap()));
}

/// A declared schema is checked against its own dialect's metaschema: an
/// array `items` is a tuple before 2020-12 and an error from it on.
#[test]
fn a_declared_schema_is_checked_by_its_own_metaschema() {
    for dialect in [
        "http://json-schema.org/draft-04/schema#",
        "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2019-09/schema",
    ] {
        let schema = json!({"$schema": dialect, "items": [{"type": "string"}]});
        assert_eq!(super::schema_errors(&schema), Some(vec![]), "{dialect}");
    }
    let modern = super::schema_errors(&json!({"items": [{"type": "string"}]})).unwrap();
    assert_ne!(modern, Vec::<String>::new());
}
