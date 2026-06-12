// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reconciliation-policy proofs: baseline entries explain by requirement and
//! optional trace filter, stale entries gate, and malformed baselines are
//! rejected with the offending field named.

#![allow(clippy::unwrap_used)]

use super::*;

fn failure(trace: &str, requirement: &str) -> ValidatorFailure {
    ValidatorFailure {
        trace: trace.to_owned(),
        requirement: requirement.to_owned(),
        detail: String::new(),
    }
}

fn entry(requirement: &str, trace_contains: Option<&str>) -> ExplainedDivergence {
    ExplainedDivergence {
        requirement: requirement.to_owned(),
        class: "suite-bug".to_owned(),
        upstream: "https://github.com/example/issues/1".to_owned(),
        trace_contains: trace_contains.map(ToOwned::to_owned),
        note: None,
    }
}

fn baseline(divergences: Vec<ExplainedDivergence>) -> DivergenceBaseline {
    DivergenceBaseline {
        policy: "p".to_owned(),
        divergences,
    }
}

#[test]
fn baseline_entry_explains_by_requirement_and_optional_trace_filter() {
    let entry = entry("LIFE-009", Some("003-"));
    assert!(explains(&entry, &failure("003-abc.jsonl", "LIFE-009")));
    assert!(!explains(&entry, &failure("004-abc.jsonl", "LIFE-009")));
    assert!(!explains(&entry, &failure("003-abc.jsonl", "TOOL-001")));
}

#[test]
fn reconcile_partitions_and_finds_no_stale_when_baseline_is_live() {
    let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
    let failures = [
        failure("003-dns-rebinding.jsonl", "LIFE-003"),
        failure("001-init.jsonl", "TOOL-001"),
    ];
    let result = reconcile(&baseline, &failures);
    assert_eq!(result.explained.len(), 1);
    assert_eq!(result.unexplained.len(), 1);
    assert_eq!(result.unexplained[0].requirement, "TOOL-001");
    assert!(result.stale.is_empty());
    // Unexplained failures gate; the message carries the triage path.
    let message = gate_error(&result, "test-baseline.json").unwrap();
    assert!(message.contains("unexplained"), "{message}");
    assert!(message.contains("TOOL-001"), "{message}");
}

#[test]
fn reconcile_flags_entries_that_explain_nothing_as_stale() {
    // The divergence this entry described was fixed upstream: nothing in
    // the run matches it, and the gate must demand its removal rather
    // than leave a pattern lying in wait for the next LIFE-003 failure.
    let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
    let result = reconcile(&baseline, &[]);
    assert!(result.explained.is_empty());
    assert!(result.unexplained.is_empty());
    assert_eq!(result.stale, ["LIFE-003 (trace_contains \"dns\")"]);
    let message = gate_error(&result, "test-baseline.json").unwrap();
    assert!(message.contains("stale"), "{message}");
    assert!(message.contains("LIFE-003"), "{message}");
}

#[test]
fn stale_entry_does_not_mask_a_new_failure_outside_its_filter() {
    // Same requirement, different session: the filtered entry explains
    // nothing (stale) and the new failure must surface as unexplained —
    // both directions reported in one gate message.
    let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
    let failures = [failure("001-init.jsonl", "LIFE-003")];
    let result = reconcile(&baseline, &failures);
    assert!(result.explained.is_empty());
    assert_eq!(result.unexplained.len(), 1);
    assert_eq!(result.stale.len(), 1);
    let message = gate_error(&result, "test-baseline.json").unwrap();
    assert!(message.contains("unexplained"), "{message}");
    assert!(message.contains("stale"), "{message}");
}

#[test]
fn unfiltered_entries_match_any_trace_and_stay_live_across_sessions() {
    // No trace_contains: the entry explains the requirement wherever it
    // fails, so it is live as long as any session still fails it.
    let baseline = baseline(vec![entry("LIFE-003", None)]);
    let failures = [failure("anything.jsonl", "LIFE-003")];
    let result = reconcile(&baseline, &failures);
    assert_eq!(result.explained.len(), 1);
    assert!(result.stale.is_empty());
    assert!(gate_error(&result, "test-baseline.json").is_none());
}

#[test]
fn baseline_rejects_unknown_class_and_non_url_upstream() {
    let dir = std::env::temp_dir().join(format!("agreement-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("baseline.json");

    std::fs::write(
        &path,
        r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"wontfix","upstream":"https://e.example"}]}"#,
    )
    .unwrap();
    assert!(load_baseline(&path).unwrap_err().contains("class"));

    std::fs::write(
        &path,
        r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"our-bug","upstream":"see notes"}]}"#,
    )
    .unwrap();
    assert!(load_baseline(&path).unwrap_err().contains("upstream"));

    std::fs::remove_dir_all(&dir).unwrap();
}
