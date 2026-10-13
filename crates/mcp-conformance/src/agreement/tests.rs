// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Reconciliation-policy proofs: baseline entries explain by requirement and
//! optional trace filter, stale entries break agreement, and malformed
//! baselines are rejected with the offending field named. (Moved here from
//! `cargo xtask conformance`, which now calls this module.)

use super::*;

fn failure(trace: &str, requirement: &str) -> ValidatorFailure {
    ValidatorFailure::new(trace, requirement, "")
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

fn baseline(divergences: Vec<ExplainedDivergence>) -> Baseline {
    Baseline {
        policy: "p".to_owned(),
        divergences,
    }
}

#[test]
fn baseline_entry_explains_by_requirement_and_optional_trace_filter() {
    let entry = entry("LIFE-009", Some("003-"));
    assert!(entry.explains(&failure("003-abc.jsonl", "LIFE-009")));
    assert!(!entry.explains(&failure("004-abc.jsonl", "LIFE-009")));
    assert!(!entry.explains(&failure("003-abc.jsonl", "TOOL-001")));
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
    assert_eq!(result.stale, Vec::<String>::new());
    assert!(!result.holds());
    // Unexplained failures gate; the message carries the triage path.
    let message = result.gate_error("test-baseline.json").unwrap();
    assert!(message.contains("1 unexplained"), "{message}");
    assert!(message.contains("TOOL-001 on 001-init.jsonl"), "{message}");
    assert!(message.contains("test-baseline.json"), "{message}");
    assert!(!message.contains("stale"), "{message}");
}

#[test]
fn reconcile_flags_entries_that_explain_nothing_as_stale() {
    // The divergence this entry described was fixed upstream: nothing in
    // the run matches it, and the gate must demand its removal rather
    // than leave a pattern lying in wait for the next LIFE-003 failure.
    let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
    let result = reconcile(&baseline, &[]);
    assert_eq!(result.explained.len(), 0);
    assert_eq!(result.unexplained.len(), 0);
    assert_eq!(result.stale, ["LIFE-003 (trace_contains \"dns\")"]);
    assert!(!result.holds());
    let message = result.gate_error("test-baseline.json").unwrap();
    assert!(message.contains("1 stale"), "{message}");
    assert!(message.contains("LIFE-003"), "{message}");
    assert!(!message.contains("unexplained"), "{message}");
}

#[test]
fn stale_entry_does_not_mask_a_new_failure_outside_its_filter() {
    // Same requirement, different session: the filtered entry explains
    // nothing (stale) and the new failure must surface as unexplained —
    // both directions reported in one gate message.
    let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
    let failures = [failure("001-init.jsonl", "LIFE-003")];
    let result = reconcile(&baseline, &failures);
    assert_eq!(result.explained.len(), 0);
    assert_eq!(result.unexplained.len(), 1);
    assert_eq!(result.stale.len(), 1);
    let message = result.gate_error("test-baseline.json").unwrap();
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
    assert_eq!(result.stale, Vec::<String>::new());
    assert!(result.holds());
    assert!(result.gate_error("test-baseline.json").is_none());
}

#[test]
fn an_empty_baseline_holds_only_without_failures() {
    let empty = Baseline::default();
    assert!(reconcile(&empty, &[]).holds());
    assert!(!reconcile(&empty, &[failure("a.jsonl", "BASE-001")]).holds());
}

#[test]
fn baseline_rejects_unknown_class_and_non_url_upstream() {
    let error = Baseline::parse(
        r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"wontfix","upstream":"https://e.example"}]}"#,
    )
    .unwrap_err();
    assert!(error.contains("class"), "{error}");
    assert!(error.contains("X-001"), "{error}");

    let error = Baseline::parse(
        r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"our-bug","upstream":"see notes"}]}"#,
    )
    .unwrap_err();
    assert!(error.contains("upstream"), "{error}");

    for upstream in ["https://e.example", "http://e.example"] {
        let text = format!(
            r#"{{"policy":"p","divergences":[{{"requirement":"X-001","class":"spec-ambiguity","upstream":"{upstream}","trace_contains":"dns","_note":"n"}}]}}"#
        );
        let parsed = Baseline::parse(&text).unwrap();
        assert_eq!(
            parsed.divergences[0].describe(),
            "X-001 (trace_contains \"dns\")"
        );
        assert_eq!(parsed.divergences[0].note.as_deref(), Some("n"));
    }

    // A typo cannot widen an entry's reach.
    let error = Baseline::parse(
        r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"our-bug","upstream":"https://e.example","trace_containz":"x"}]}"#,
    )
    .unwrap_err();
    assert!(error.contains("trace_containz"), "{error}");
}

#[test]
fn this_repositorys_baselines_parse() {
    for path in [
        "../../conformance/agreement-divergences.json",
        "../../conformance/client-agreement-divergences.json",
    ] {
        let text =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
                .unwrap();
        Baseline::parse(&text).unwrap();
    }
}

/// A failing clause becomes a failure carrying its first finding; totals add
/// across reports.
#[test]
fn failures_and_totals_come_from_the_report() {
    use mcp_conformance_core::requirement::Registry;
    use mcp_trace_validator::reader::{Limits, parse_trace};

    // An initialize answered, then a request id reused: BASE-003 fails.
    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#;
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let registry = Registry::builtin_2025_11_25().unwrap();
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    let found = failures("t.jsonl", &report);
    assert!(!found.is_empty(), "{}", report.render_human());
    assert!(found.iter().all(|failure| failure.trace == "t.jsonl"));
    assert!(
        found
            .iter()
            .any(|failure| failure.requirement == "BASE-003" && !failure.detail.is_empty()),
        "{found:?}"
    );
    assert_eq!(
        u32::try_from(found.len()).unwrap(),
        report.totals.fail,
        "one failure per failing clause"
    );

    // Distinct, non-zero counts in every field, so each sum is its own.
    let mut counted = report;
    counted.totals.pass = 1;
    counted.totals.fail = 2;
    counted.totals.warn = 3;
    counted.totals.excluded = 4;
    counted.totals.unsupported = 5;
    counted.totals.not_applicable = 6;
    counted.totals.not_observed = 7;
    let mut totals = Totals::default();
    totals.add(&counted);
    totals.add(&counted);
    assert_eq!(
        [
            totals.pass,
            totals.fail,
            totals.warn,
            totals.excluded,
            totals.unsupported,
            totals.not_applicable,
            totals.not_observed
        ],
        [2, 4, 6, 8, 10, 12, 14]
    );
}

#[test]
fn the_reconciliation_serializes_as_the_agreement_artifact_has_it() {
    let reconciliation = reconcile(
        &baseline(vec![entry("LIFE-003", None)]),
        &[failure("a.jsonl", "TOOL-001")],
    );
    let json = serde_json::to_value(&reconciliation).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "unexplained": [{"trace": "a.jsonl", "requirement": "TOOL-001", "detail": ""}],
            "explained": [],
            "stale_baseline_entries": ["LIFE-003"],
        })
    );
}
