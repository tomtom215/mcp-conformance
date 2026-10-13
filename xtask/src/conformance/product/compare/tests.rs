// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use serde_json::json;

use super::*;

fn gate(scenarios: &[(&str, &[&str])], failing: &[&str]) -> Gate {
    Gate {
        scenarios: scenarios
            .iter()
            .map(|(name, statuses)| {
                (
                    (*name).to_owned(),
                    statuses.iter().map(|s| (*s).to_owned()).collect(),
                )
            })
            .collect(),
        failing: failing.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn report(scenarios: &[(&str, &[&str])], not_run: &[&str], failures: &[(&str, &str)]) -> Report {
    Report::from_json(&json!({
        "suite": {
            "scenarios": scenarios.iter().map(|(name, statuses)| json!({
                "name": name,
                "checks": statuses.iter().map(|status| json!({"id": "c", "status": status})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "not_run": not_run.iter().map(|name| json!({"scenario": name, "reason": "r"})).collect::<Vec<_>>(),
        },
        "agreement": {"divergences": {
            "explained": [],
            "unexplained": failures.iter().map(|(trace, requirement)| json!({"trace": trace, "requirement": requirement})).collect::<Vec<_>>(),
        }},
    }))
}

fn wire(findings: &str) -> WireFindings {
    serde_json::from_str(&format!(r#"{{"policy": "p", "findings": {findings}}}"#)).unwrap()
}

#[test]
fn identical_suite_results_and_listed_extras_agree() {
    let gate = gate(&[("ping", &["SUCCESS"])], &["LIFE-003"]);
    let wire = wire(r#"[{"leg": "http", "requirement": "BASE-009", "why": "w"}]"#);
    let report = report(
        &[("ping", &["SUCCESS"])],
        &[],
        &[("002.jsonl", "LIFE-003"), ("001.jsonl", "BASE-009")],
    );
    check(Leg::Http, &gate, &report, &wire).unwrap();
    wire.all_observed().unwrap();
}

#[test]
fn a_different_suite_result_disagrees() {
    let gate = gate(&[("ping", &["SUCCESS"])], &[]);
    let report = report(&[("ping", &["FAILURE"])], &[], &[]);
    let error = check(Leg::Http, &gate, &report, &wire("[]")).unwrap_err();
    assert!(error.contains("differ"), "{error}");
}

#[test]
fn a_failure_the_gate_sees_must_be_in_test_too() {
    let gate = gate(&[], &["LIFE-003"]);
    let error = check(Leg::Client, &gate, &report(&[], &[], &[]), &wire("[]")).unwrap_err();
    assert!(error.contains("LIFE-003"), "{error}");
}

#[test]
fn an_unlisted_extra_disagrees_and_an_unseen_entry_is_stale() {
    let gate = gate(&[], &[]);
    let wire = wire(
        r#"[{"leg": "client", "requirement": "TRAN-017", "trace_contains": "sse-retry/", "why": "w"}]"#,
    );
    let error = check(
        Leg::Client,
        &gate,
        &report(&[], &[], &[("initialize/001.jsonl", "TRAN-017")]),
        &wire,
    )
    .unwrap_err();
    assert!(
        error.contains("TRAN-017 on initialize/001.jsonl"),
        "{error}"
    );
    let stale = wire.all_observed().unwrap_err();
    assert!(stale.contains("client TRAN-017"), "{stale}");
}

#[test]
fn stdio_is_compared_without_the_http_transport_scenarios() {
    let gate = gate(
        &[
            ("ping", &["SUCCESS"]),
            ("dns-rebinding-protection", &["SUCCESS"]),
        ],
        &["LIFE-003"],
    );
    let report = report(
        &[("ping", &["SUCCESS"])],
        &["dns-rebinding-protection"],
        &[],
    );
    check(Leg::Stdio, &gate, &report, &wire("[]")).unwrap();
    // The pending list: what `list` names beyond the default run.
    let listed = report_with_pending();
    pending_is_what_the_default_run_leaves_out(&gate, &listed).unwrap();
    // A scenario `list` names that the default run did not run, and that the
    // pending list does not name: the list is stale.
    let unexpected = report_with(&["extra"]);
    let error = pending_is_what_the_default_run_leaves_out(&gate, &unexpected).unwrap_err();
    assert!(error.contains("stale"), "{error}");
}

fn report_with_pending() -> Report {
    report_with(&[])
}

/// The stdio run of the gate above: `ping` run, the rest of what `list`
/// names (the pending scenarios, the HTTP-transport one, and `extra`) not.
fn report_with(extra: &[&str]) -> Report {
    let mut not_run: Vec<&str> = suite::PENDING_SERVER_SCENARIOS.to_vec();
    not_run.push("dns-rebinding-protection");
    not_run.extend_from_slice(extra);
    report(&[("ping", &["SUCCESS"])], &not_run, &[])
}

#[test]
fn the_committed_wire_findings_load() {
    WireFindings::load(&crate::workspace_root().join(super::super::WIRE_FINDINGS)).unwrap();
}
