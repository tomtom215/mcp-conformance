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
            "set_aside": [],
        }},
        "sessions": {"unjudged": []},
        "verdict": "pass",
    }))
}

/// `base`, with `change` applied to its JSON before it is read.
fn amended(base: &serde_json::Value, change: impl FnOnce(&mut serde_json::Value)) -> Report {
    let mut json = base.clone();
    change(&mut json);
    Report::from_json(&json)
}

fn quiet() -> serde_json::Value {
    json!({
        "suite": {"scenarios": [], "not_run": []},
        "agreement": {"divergences": {"explained": [], "unexplained": [], "set_aside": []}},
        "sessions": {"unjudged": []},
        "verdict": "pass",
    })
}

fn wire(findings: &str) -> WireFindings {
    serde_json::from_str(&format!(r#"{{"policy": "p", "findings": {findings}}}"#)).unwrap()
}

/// A committed file with all three lists.
fn wire_all(set_aside: &str, unjudged: &str) -> WireFindings {
    serde_json::from_str(&format!(
        r#"{{"policy": "p", "findings": [], "set_aside": {set_aside}, "unjudged": {unjudged}}}"#
    ))
    .unwrap()
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

/// The audit's A09: `test` exited 0 or 1 and this leg accepted both, never
/// reading what verdict the product reported. Every leg is a green run, so the
/// product's verdict is `pass`, and anything else is the leg failing.
#[test]
fn a_run_whose_verdict_is_not_a_pass_disagrees() {
    let gate = gate(&[], &[]);
    for verdict in ["fail", "incomplete"] {
        let run = amended(&quiet(), |json| json["verdict"] = json!(verdict));
        let error = check(Leg::Http, &gate, &run, &wire("[]")).unwrap_err();
        assert!(
            error.contains("verdict") && error.contains(verdict),
            "{error}"
        );
    }
    check(Leg::Http, &gate, &amended(&quiet(), |_| {}), &wire("[]")).unwrap();
}

/// What `test` sets aside as held against the suite's own end is committed, so
/// a new one is noticed, and one that stops occurring is removed: the
/// staleness gate the wire findings had.
#[test]
fn a_finding_set_aside_must_be_listed_and_a_listed_one_must_occur() {
    let gate = gate(&[], &[]);
    let run = amended(&quiet(), |json| {
        json["agreement"]["divergences"]["set_aside"] =
            json!([{"trace": "initialize/001.jsonl", "requirement": "BASE-004"}]);
    });
    let error = check(Leg::Client, &gate, &run, &wire_all("[]", "[]")).unwrap_err();
    assert!(
        error.contains("sets aside") && error.contains("BASE-004 on initialize/001.jsonl"),
        "{error}"
    );

    let entry = r#"[{"leg": "client", "requirement": "BASE-004", "trace_contains": "initialize/", "why": "w"}]"#;
    let listed = wire_all(entry, "[]");
    check(Leg::Client, &gate, &run, &listed).unwrap();
    listed.all_observed().unwrap();

    // The entry belongs to its leg and to its requirement.
    let other_leg = wire_all(
        r#"[{"leg": "http", "requirement": "BASE-004", "why": "w"}]"#,
        "[]",
    );
    assert!(check(Leg::Client, &gate, &run, &other_leg).is_err());

    // Nothing is set aside any more: the entry is stale.
    let listed = wire_all(entry, "[]");
    check(Leg::Client, &gate, &amended(&quiet(), |_| {}), &listed).unwrap();
    let stale = listed.all_observed().unwrap_err();
    assert!(
        stale.contains("no longer occur") && stale.contains("BASE-004"),
        "{stale}"
    );
}

/// A session `test` could not judge is expected, by name and leg, or the leg
/// fails; an expected one that goes unjudged no longer is stale.
#[test]
fn an_unjudged_session_must_be_listed_and_a_listed_one_must_occur() {
    let gate = gate(&[], &[]);
    let run = amended(&quiet(), |json| {
        json["sessions"]["unjudged"] = json!([{"trace": "/w/sse-retry/001.jsonl", "name": "sse-retry/001.jsonl", "class": "informational", "error": "e"}]);
    });
    let error = check(Leg::Client, &gate, &run, &wire_all("[]", "[]")).unwrap_err();
    assert!(
        error.contains("could not judge") && error.contains("sse-retry/001.jsonl"),
        "{error}"
    );

    let entry = r#"[{"leg": "client", "name": "sse-retry/001.jsonl", "why": "w"}]"#;
    let listed = wire_all("[]", entry);
    check(Leg::Client, &gate, &run, &listed).unwrap();
    listed.all_observed().unwrap();
    assert_eq!(listed.expected_unjudged("client"), ["sse-retry/001.jsonl"]);
    assert_eq!(listed.expected_unjudged("http"), Vec::<&str>::new());
    assert!(
        check(Leg::Http, &gate, &run, &listed).is_err(),
        "listed for the client leg only"
    );

    let listed = wire_all("[]", entry);
    check(Leg::Client, &gate, &amended(&quiet(), |_| {}), &listed).unwrap();
    let stale = listed.all_observed().unwrap_err();
    assert!(stale.contains("sse-retry/001.jsonl"), "{stale}");
}
