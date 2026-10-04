// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the SARIF rendering.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::reader::{Limits, parse_trace};
use mcp_conformance_core::requirement::{Registry, RegistrySet};

const WRONG_VERSION: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"1.0","id":1,"method":"ping"}}"#;

fn log(reports: &[Report], uri: Option<&str>, events: &[TraceEvent]) -> Value {
    serde_json::from_str(&render(reports, Artifact { uri, events })).unwrap()
}

/// Two revisions sharing `BASE-001`, and `ONLY-001` at the later one only.
fn two_revision_set() -> RegistrySet {
    RegistrySet::from_json(
        r#"{
        "revisions": ["2025-11-25", "2026-07-28"],
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic#x", "quote": "MUST be 2.0"},
             "checks": ["base.jsonrpc-version"]},
            {"id": "ONLY-001", "level": "SHOULD", "actor": "both",
             "applies": {"introduced": "2026-07-28"},
             "source": {"section": "basic#y", "quote": "SHOULD be 2.0"},
             "checks": ["base.jsonrpc-version"]}
        ]
    }"#,
    )
    .unwrap()
}

#[test]
fn an_id_judged_under_two_revisions_gets_a_rule_per_revision() {
    let set = two_revision_set();
    let events = parse_trace(WRONG_VERSION, &Limits::default()).unwrap();
    let reports: Vec<Report> = set
        .revisions()
        .iter()
        .map(|revision| crate::engine::validate(&set.registry(*revision).unwrap(), &events))
        .collect();
    let log = log(&reports, Some("t.jsonl"), &events);
    let ids: Vec<&str> = log["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .map(|rule| rule["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        ["BASE-001@2025-11-25", "BASE-001@2026-07-28", "ONLY-001"]
    );
    let results = log["runs"][0]["results"].as_array().unwrap();
    let levels: Vec<(&str, &str, u64)> = results
        .iter()
        .map(|result| {
            (
                result["ruleId"].as_str().unwrap(),
                result["level"].as_str().unwrap(),
                result["ruleIndex"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        levels,
        [
            ("BASE-001@2025-11-25", "error", 0),
            ("BASE-001@2026-07-28", "error", 1),
            ("ONLY-001", "warning", 2)
        ]
    );
    assert_eq!(
        log["runs"][0]["tool"]["driver"]["rules"][2]["defaultConfiguration"]["level"],
        "warning"
    );
    assert_eq!(
        log["runs"][0]["properties"]["revisions"],
        json!(["2025-11-25", "2026-07-28"])
    );
    assert_eq!(
        log["runs"][0]["invocations"][0]["executionSuccessful"],
        true
    );
}

#[test]
fn an_unsupported_clause_is_a_failed_invocation_with_a_notification() {
    let registry = Registry::from_json(
        r#"{"revision": "2025-11-25", "requirements": [
            {"id": "FUTR-001", "level": "MUST", "actor": "both",
             "source": {"section": "future#x", "quote": "MUST do future things"},
             "checks": ["future.not-built-yet"]}]}"#,
    )
    .unwrap();
    let report = crate::engine::validate(&registry, &[]);
    let log = log(&[report], None, &[]);
    let invocation = &log["runs"][0]["invocations"][0];
    assert_eq!(invocation["executionSuccessful"], false);
    assert_eq!(
        invocation["toolExecutionNotifications"][0]["message"]["text"],
        "FUTR-001 (2025-11-25): the registry references checks this build does not \
         implement: future.not-built-yet"
    );
    assert_eq!(log["runs"][0]["results"], json!([]));
}

#[test]
fn a_finding_without_a_seq_or_an_unknown_one_has_no_region() {
    let events = parse_trace(WRONG_VERSION, &Limits::default()).unwrap();
    let artifact = Artifact {
        uri: Some("t.jsonl"),
        events: &events,
    };
    assert_eq!(
        location(artifact, Some(0))[0]["physicalLocation"]["region"]["startLine"],
        1
    );
    assert!(
        location(artifact, None)[0]["physicalLocation"]
            .get("region")
            .is_none()
    );
    assert!(
        location(artifact, Some(9))[0]["physicalLocation"]
            .get("region")
            .is_none()
    );
}

#[test]
fn paths_become_uri_references() {
    assert_eq!(
        uri_reference("corpus/a-b_c.~1.jsonl"),
        "corpus/a-b_c.~1.jsonl"
    );
    assert_eq!(
        uri_reference(r"traces\my run #2.jsonl"),
        "traces/my%20run%20%232.jsonl"
    );
    assert_eq!(uri_reference("é"), "%C3%A9");
    // A colon in a relative path cannot be taken for a scheme.
    assert_eq!(uri_reference("a:b.jsonl"), "a%3Ab.jsonl");
    // Absolute paths are file: URIs, drive letters intact.
    assert_eq!(
        uri_reference("/tmp/my run.jsonl"),
        "file:///tmp/my%20run.jsonl"
    );
    assert_eq!(
        uri_reference(r"D:\a\b c\t.jsonl"),
        "file:///D:/a/b%20c/t.jsonl"
    );
    assert_eq!(uri_reference("c:/t.jsonl"), "file:///c:/t.jsonl");
    // A drive letter needs its separator: `C:` alone is a relative name.
    assert_eq!(uri_reference("C:t"), "C%3At");
}

#[test]
fn numbers_fold_to_one_mark_and_text_stays() {
    assert_eq!(fold_numbers("seq 12: id 3a, 007"), "seq #: id #a, #");
    assert_eq!(fold_numbers("no digits"), "no digits");
    assert_eq!(fold_numbers(""), "");
}

#[test]
fn identical_findings_in_one_trace_get_distinct_fingerprints() {
    // Two messages break the same clause the same way; folded, their details
    // are equal, so only the per-trace occurrence count tells them apart.
    let trace = format!(
        "{WRONG_VERSION}\n{}",
        WRONG_VERSION
            .replace(r#""seq":0"#, r#""seq":1"#)
            .replace(r#""id":1"#, r#""id":2"#)
    );
    let events = parse_trace(&trace, &Limits::default()).unwrap();
    let registry = Registry::builtin_2025_11_25().unwrap();
    let report = crate::engine::validate(&registry, &events);
    let log = log(&[report], Some("t.jsonl"), &events);
    let fingerprints: Vec<&Value> = log["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|result| result["ruleId"] == "BASE-008")
        .map(|result| &result["partialFingerprints"]["mcpConformanceFinding/v2"])
        .collect();
    assert_eq!(fingerprints.len(), 2, "{log}");
    assert_ne!(fingerprints[0], fingerprints[1]);
}
