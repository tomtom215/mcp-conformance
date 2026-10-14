// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Wire-schema agreement proofs, on fixtures: the runner's report shape is
//! read as the published package writes it, messages join by content, the
//! two sides agree on a message both flag and on a session neither does, and
//! a divergence is either explained by a ledger entry or fails the gate —
//! with stale and malformed entries failing too.

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::requirement::Registry;
use mcp_trace_validator::reader::{Limits, parse_trace};
use serde_json::json;

use super::*;
use crate::agreement::schema_ledger::{self, Ledger};

/// A `2025-11-25` session whose `resources/list` result (seq 4) lacks the
/// `uri` every `Resource` requires — the validator's one finding.
const INVALID_TRACE: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0.1.0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"resources":{}},"serverInfo":{"name":"s","version":"0.1.0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"resources/list"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"resources":[{"name":"readme"}]}}}"#;

/// The seq-4 message as the runner writes it back: same content, other key
/// order — the join must not care.
fn invalid_message() -> Value {
    json!({"result": {"resources": [{"name": "readme"}]}, "id": 2, "jsonrpc": "2.0"})
}

fn report_for(trace: &str) -> (Vec<TraceEvent>, Report) {
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let registry = Registry::builtin_2025_11_25().unwrap();
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    (events, report)
}

/// One scenario's `checks.json`, shaped as the runner's `wire-schema-valid`.
fn suite_checks(violations: &[(&str, Value)]) -> Vec<Value> {
    let violations: Vec<Value> = violations
        .iter()
        .map(|(origin, message)| {
            json!({"origin": origin, "specVersion": "2025-11-25", "context": "c",
                   "errors": ["must have required property 'uri'"], "message": message})
        })
        .collect();
    let status = if violations.is_empty() {
        "SUCCESS"
    } else {
        "FAILURE"
    };
    vec![
        json!({"id": "server-info", "status": "INFO"}),
        json!({"id": SUITE_CHECK, "status": status,
               "details": {"messagesValidated": 5, "violations": violations}}),
    ]
}

fn suite(violations: &[(&str, Value)]) -> SuiteSide {
    let mut side = SuiteSide::default();
    read_checks(
        &mut side,
        "server-resources-list",
        &suite_checks(violations),
    )
    .unwrap();
    side.flagged_messages = side.flagged.len();
    side
}

fn ledger(entries: &str) -> Ledger {
    schema_ledger::parse(&format!(r#"{{"policy":"p","divergences":[{entries}]}}"#)).unwrap()
}

fn agreement(trace: &str, suite: SuiteSide, ledger: &Ledger) -> schema_ledger::SchemaAgreement {
    let (events, report) = report_for(trace);
    let sessions = [Session {
        trace: "001-session.jsonl",
        events: &events,
        report: &report,
    }];
    let comparison = compare(&SERVER_LEG, &suite, &sessions);
    schema_ledger::reconcile(&SERVER_LEG, ledger, suite, comparison)
}

const ENTRY: &str = r#"{"leg":"server","kind":"validator-only","message_contains":"\"name\":\"readme\"","class":"schema-pin-skew","upstream":"https://example.com/1","reason":"r"}"#;

#[test]
fn canonical_sorts_keys_and_writes_integral_floats_as_integers() {
    let a: Value = serde_json::from_str(r#"{"b":1.0,"a":[2.5,{"d":3,"c":null}]}"#).unwrap();
    let b: Value = serde_json::from_str(r#"{"a":[2.5,{"c":null,"d":3.0}],"b":1}"#).unwrap();
    assert_eq!(canonical(&a), canonical(&b));
    assert_eq!(canonical(&a), r#"{"a":[2.5,{"c":null,"d":3}],"b":1}"#);
}

#[test]
fn reads_both_checks_and_rejects_shapes_it_does_not_know() {
    let mut side = SuiteSide::default();
    let mut checks = suite_checks(&[("implementation", invalid_message())]);
    checks.push(json!({"id": SUITE_HARNESS_CHECK, "status": "FAILURE",
        "details": {"violations": [{"origin": "harness", "errors": [], "message": {"x": 1}}]}}));
    read_checks(&mut side, "s", &checks).unwrap();
    assert_eq!(
        (side.scenarios, side.failing, side.messages_validated),
        (1, 1, 5)
    );
    assert_eq!(side.flagged.len(), 2);
    assert_eq!(side.flagged[1].origin, Origin::Harness);

    // FAILURE must mean "some violation", and the reverse.
    let bad = json!([{"id": SUITE_CHECK, "status": "FAILURE", "details": {"violations": []}}]);
    let error = read_checks(&mut side, "s", bad.as_array().unwrap()).unwrap_err();
    assert!(error.contains("FAILURE"), "{error}");
    let bad = json!([{"id": SUITE_CHECK, "status": "SUCCESS"}]);
    let error = read_checks(&mut side, "s", bad.as_array().unwrap()).unwrap_err();
    assert!(error.contains("violations"), "{error}");
    let bad = json!([{"id": SUITE_CHECK, "status": "FAILURE",
        "details": {"violations": [{"origin": "other", "message": {}}]}}]);
    assert!(read_checks(&mut side, "s", bad.as_array().unwrap()).is_err());
}

#[test]
fn agree_when_both_flag_the_same_message() {
    let result = agreement(
        INVALID_TRACE,
        suite(&[("implementation", invalid_message())]),
        &ledger(""),
    );
    assert!(result.unexplained.is_empty() && result.explained.is_empty());
    assert_eq!(result.sessions[0].validator, "fail");
    assert_eq!(result.sessions[0].suite, "fail");
    assert!(result.sessions[0].agree);
    assert!(result.gate_error().is_none());
}

#[test]
fn agree_when_neither_flags_anything() {
    let clean = INVALID_TRACE.replace(
        r#"{"name":"readme"}"#,
        r#"{"name":"readme","uri":"file:///r"}"#,
    );
    let result = agreement(&clean, suite(&[]), &ledger(""));
    assert_eq!(result.sessions[0].messages, 5);
    assert_eq!(
        (result.sessions[0].validator, result.sessions[0].suite),
        ("pass", "pass")
    );
    assert!(result.gate_error().is_none());
}

#[test]
fn validator_only_divergence_unexplained_fails_the_gate() {
    let result = agreement(INVALID_TRACE, suite(&[]), &ledger(""));
    assert_eq!(result.unexplained.len(), 1);
    let divergence = &result.unexplained[0];
    assert_eq!(
        (divergence.kind, divergence.origin),
        (Kind::ValidatorOnly, Origin::Implementation)
    );
    assert_eq!(divergence.seq, Some(4));
    assert!(!result.sessions[0].agree);
    let message = result.gate_error().unwrap();
    assert!(message.contains("validator-only"), "{message}");
    assert!(message.contains("seq 4"), "{message}");
}

#[test]
fn suite_only_divergences_name_the_session_or_its_absence() {
    // The runner flags the initialize result (we pass it) and a message the
    // tap never recorded; both fail the gate, the second with no session.
    let initialize = report_for(INVALID_TRACE).0[1]
        .message_payload()
        .unwrap()
        .clone();
    let result = agreement(
        INVALID_TRACE,
        suite(&[
            ("implementation", invalid_message()),
            ("implementation", initialize),
            (
                "implementation",
                json!({"jsonrpc": "2.0", "id": 99, "result": {}}),
            ),
        ]),
        &ledger(""),
    );
    assert_eq!(result.unexplained.len(), 2);
    assert!(result.unexplained.iter().all(|d| d.kind == Kind::SuiteOnly));
    assert_eq!(result.unexplained[0].seq, Some(1));
    assert_eq!(result.unexplained[0].scenarios, ["server-resources-list"]);
    assert_eq!(result.unexplained[1].trace, None);
    // Both sides say "fail" for the session, but the messages differ: the
    // message level is what gates.
    assert!(result.sessions[0].agree);
    assert!(result.gate_error().unwrap().contains("no tapped session"));
}

#[test]
fn a_harness_message_joins_only_as_a_harness_message() {
    // The client's notifications/initialized, flagged as the implementation's,
    // is not the same message as the harness's: it surfaces as suite-only with
    // no session carrying it.
    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let result = agreement(
        INVALID_TRACE,
        suite(&[
            ("implementation", invalid_message()),
            ("implementation", initialized.clone()),
        ]),
        &ledger(""),
    );
    assert_eq!(result.unexplained.len(), 1);
    assert_eq!(result.unexplained[0].trace, None);
    let result = agreement(
        INVALID_TRACE,
        suite(&[
            ("implementation", invalid_message()),
            ("harness", initialized),
        ]),
        &ledger(""),
    );
    assert_eq!(result.unexplained[0].origin, Origin::Harness);
    assert_eq!(result.unexplained[0].seq, Some(2));
}

#[test]
fn a_ledger_entry_explains_a_divergence_and_the_gate_passes() {
    let result = agreement(INVALID_TRACE, suite(&[]), &ledger(ENTRY));
    assert_eq!(result.explained.len(), 1);
    assert!(result.unexplained.is_empty());
    assert_eq!(result.stale_ledger_entries, Vec::<String>::new());
    assert!(result.gate_error().is_none());
}

#[test]
fn an_entry_that_explains_nothing_is_stale_and_fails_the_gate() {
    let result = agreement(
        INVALID_TRACE,
        suite(&[("implementation", invalid_message())]),
        &ledger(ENTRY),
    );
    assert_eq!(result.stale_ledger_entries.len(), 1);
    assert!(result.gate_error().unwrap().contains("stale"));
    // An entry for another leg is neither applied nor stale here.
    let other = ENTRY.replace(r#""leg":"server""#, r#""leg":"client""#);
    let result = agreement(INVALID_TRACE, suite(&[]), &ledger(&other));
    assert_eq!(result.unexplained.len(), 1);
    assert_eq!(result.stale_ledger_entries, Vec::<String>::new());
}

#[test]
fn malformed_entries_are_rejected_with_the_field_named() {
    let parse = |entry: &str| {
        schema_ledger::parse(&format!(r#"{{"policy":"p","divergences":[{entry}]}}"#)).unwrap_err()
    };
    for (from, to, field) in [
        (r#""leg":"server""#, r#""leg":"sevrer""#, "leg"),
        (r#""kind":"validator-only""#, r#""kind":"both""#, "kind"),
        (
            r#""message_contains":"\"name\":\"readme\"""#,
            r#""message_contains":"""#,
            "message_contains",
        ),
        (
            r#""class":"schema-pin-skew""#,
            r#""class":"wontfix""#,
            "class",
        ),
        (
            r#""upstream":"https://example.com/1""#,
            r#""upstream":"see notes""#,
            "upstream",
        ),
        (r#""reason":"r""#, r#""reason":" ""#, "reason"),
        (
            r#""reason":"r""#,
            r#""reason":"r","origin":"server""#,
            "origin",
        ),
        (
            r#""reason":"r""#,
            r#""reason":"r","trace_containz":"x""#,
            "trace_containz",
        ),
    ] {
        let error = parse(&ENTRY.replace(from, to));
        assert!(error.contains(field), "{field}: {error}");
    }
}

#[test]
fn the_committed_ledger_parses() {
    schema_ledger::parse(include_str!(
        "../../../../conformance/wire-schema-divergences.json"
    ))
    .unwrap();
}

#[test]
fn without_a_runner_check_validator_findings_stand_against_its_green_run() {
    let dir = std::env::temp_dir().join(format!("schema-agreement-{}", std::process::id()));
    let scenario = dir.join("results/server-ping");
    std::fs::create_dir_all(&scenario).unwrap();
    std::fs::create_dir_all(dir.join("conformance")).unwrap();
    std::fs::write(
        dir.join(schema_ledger::LEDGER),
        r#"{"policy":"p","divergences":[]}"#,
    )
    .unwrap();
    std::fs::write(
        scenario.join("checks.json"),
        r#"[{"id":"ping","status":"SUCCESS"}]"#,
    )
    .unwrap();
    let clean = INVALID_TRACE.replace(
        r#"{"name":"readme"}"#,
        r#"{"name":"readme","uri":"file:///r"}"#,
    );
    let run = |trace: &str| {
        let (events, report) = report_for(trace);
        let sessions = [Session {
            trace: "t",
            events: &events,
            report: &report,
        }];
        schema_ledger::check(&dir, &SERVER_LEG, &dir.join("results"), &sessions).unwrap()
    };
    let clean = run(&clean);
    let invalid = run(INVALID_TRACE);
    std::fs::remove_dir_all(&dir).unwrap();
    assert!(!clean.compared);
    assert!(clean.note.as_deref().unwrap().contains("alpha.11"));
    assert!(clean.gate_error().is_none());
    assert_eq!(invalid.unexplained.len(), 1);
    assert_eq!(invalid.unexplained[0].kind, Kind::ValidatorOnly);
    assert!(invalid.gate_error().is_some());
}

#[test]
fn without_a_runner_check_suite_only_entries_are_not_stale() {
    let suite_only = ENTRY.replace("validator-only", "suite-only");
    let result = agreement(
        INVALID_TRACE
            .lines()
            .take(4)
            .collect::<Vec<_>>()
            .join("\n")
            .as_str(),
        SuiteSide::default(),
        &ledger(&suite_only),
    );
    assert!(!result.compared);
    assert_eq!(result.stale_ledger_entries, Vec::<String>::new());
    assert!(result.gate_error().is_none());
}
