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

fn held(trace: &str, requirement: &str, party: Party) -> ValidatorFailure {
    failure(trace, requirement).held_against(party)
}

/// The audit's finding 1.1, as policy: the suite's own client's failures are
/// shown and never counted for a server under test, and the other way round.
#[test]
fn reconcile_for_sets_aside_the_other_ends_failures_and_counts_the_rest() {
    let failures = [
        held("002.jsonl", "LIFE-009", Party::Client),
        held("003.jsonl", "TOOL-001", Party::Server),
        failure("004.jsonl", "BASE-001"),
    ];
    let none = Baseline::default();

    let server = reconcile_for(&none, &failures, Party::Server);
    assert_eq!(server.subject(), Some(Party::Server));
    assert_eq!(server.set_aside, [failures[0].clone()]);
    assert_eq!(
        server.unexplained,
        [failures[1].clone(), failures[2].clone()],
        "the server's own, and the one held against nobody, count"
    );
    assert!(!server.holds());

    let client = reconcile_for(&none, &failures, Party::Client);
    assert_eq!(client.set_aside, [failures[1].clone()]);
    assert_eq!(
        client.unexplained,
        [failures[0].clone(), failures[2].clone()]
    );

    // Without a subject nothing is set aside: every failure counts, as the
    // repository's gate counts them.
    let everyone = reconcile(&none, &failures);
    assert_eq!(everyone.subject(), None);
    assert_eq!(everyone.set_aside, Vec::<ValidatorFailure>::new());
    assert_eq!(everyone.unexplained.len(), 3);

    // Only the other end's failures: agreement holds.
    let only = reconcile_for(&none, &failures[..1], Party::Server);
    assert!(only.holds());
    assert!(only.gate_error("b.json").is_none());
}

/// A baseline entry explains a failure of either end, so the file serves the
/// gate (both ends) and `test` (the subject's), and an entry is stale only
/// when it explains nothing at all — a set-aside failure included.
#[test]
fn a_baseline_entry_explains_either_ends_failure_and_is_live_while_it_explains_any() {
    let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
    let failures = [held("031-dns.jsonl", "LIFE-003", Party::Client)];
    let server = reconcile_for(&baseline, &failures, Party::Server);
    assert_eq!(server.explained.len(), 1, "{server:?}");
    assert_eq!(server.set_aside, Vec::<ValidatorFailure>::new());
    assert!(
        server.stale.is_empty(),
        "explains a failure of the other end"
    );
    assert!(server.holds());

    // Once nothing matches it, it is stale, whichever end would have been.
    let gone = reconcile_for(&baseline, &[], Party::Server);
    assert_eq!(gone.stale, ["LIFE-003 (trace_contains \"dns\")"]);
    assert!(!gone.holds());

    // A different trace of the same clause is not explained by the filtered
    // entry: the other end's one is set aside, the subject's one counts, and
    // the entry is stale.
    let elsewhere = [
        held("001-init.jsonl", "LIFE-003", Party::Client),
        held("002-init.jsonl", "LIFE-003", Party::Server),
    ];
    let result = reconcile_for(&baseline, &elsewhere, Party::Server);
    assert_eq!(result.set_aside, [elsewhere[0].clone()]);
    assert_eq!(result.unexplained, [elsewhere[1].clone()]);
    assert_eq!(result.stale.len(), 1);
}

#[test]
fn a_scoped_runs_listing_says_whose_each_failure_is_and_an_unscoped_one_does_not() {
    let failures = [
        held("a.jsonl", "TOOL-001", Party::Server),
        failure("b.jsonl", "BASE-001"),
    ];
    let scoped = reconcile_for(&Baseline::default(), &failures, Party::Server)
        .gate_error("b.json")
        .unwrap();
    assert!(
        scoped.contains("  TOOL-001 on a.jsonl [server]: "),
        "{scoped}"
    );
    assert!(
        scoped.contains("  BASE-001 on b.jsonl [unattributed]: "),
        "{scoped}"
    );
    let unscoped = reconcile(&Baseline::default(), &failures[1..])
        .gate_error("b.json")
        .unwrap();
    assert!(unscoped.contains("  BASE-001 on b.jsonl: "), "{unscoped}");
}

#[test]
fn the_set_aside_serializes_only_when_there_is_one() {
    let with = reconcile_for(
        &Baseline::default(),
        &[held("a.jsonl", "LIFE-009", Party::Client)],
        Party::Server,
    );
    assert_eq!(
        serde_json::to_value(&with).unwrap(),
        serde_json::json!({
            "unexplained": [],
            "explained": [],
            "set_aside": [{
                "trace": "a.jsonl", "requirement": "LIFE-009", "detail": "", "party": "client"
            }],
            "stale_baseline_entries": [],
        })
    );
    // Not scoped, nothing aside: the shape the repository's gate has always had.
    let without = serde_json::to_value(reconcile(&Baseline::default(), &[])).unwrap();
    assert!(without.get("set_aside").is_none(), "{without}");
}

/// One failing clause becomes one failure for each party its findings are held
/// against, with that party's first finding as the detail.
#[test]
fn a_failing_clause_is_one_failure_for_each_party_it_is_held_against() {
    use mcp_conformance_core::requirement::Registry;
    use mcp_trace_validator::reader::{Limits, parse_trace};

    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}
{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"ping"}}
{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{}}}"#;
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let registry = Registry::builtin_2025_11_25().unwrap();
    let mut report = mcp_trace_validator::engine::validate(&registry, &events);

    // The engine holds the reused id against the client (it sent the request).
    let found = failures_by_party("t.jsonl", &report);
    let reused: Vec<_> = found
        .iter()
        .filter(|failure| failure.requirement == "BASE-003")
        .collect();
    assert_eq!(reused.len(), 1, "{found:?}");
    assert_eq!(reused[0].party, Some(Party::Client));
    assert_ne!(reused[0].detail, "");
    // One failure per failing clause, as `failures` has it, while each is
    // held against one party.
    assert_eq!(found.len(), failures("t.jsonl", &report).len());

    // The same clause failing on both ends, and once against nobody.
    let row = report
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-003")
        .unwrap();
    let mut server = row.findings[0].clone();
    server.party = Some(Party::Server);
    server.detail = "the server's".to_owned();
    let mut nobody = row.findings[0].clone();
    nobody.party = None;
    nobody.detail = "nobody's".to_owned();
    let mut second_client = row.findings[0].clone();
    second_client.detail = "a later client finding".to_owned();
    row.findings.extend([server, nobody, second_client]);
    let found: Vec<_> = failures_by_party("t.jsonl", &report)
        .into_iter()
        .filter(|failure| failure.requirement == "BASE-003")
        .collect();
    let parties: Vec<_> = found.iter().map(|failure| failure.party).collect();
    assert_eq!(parties, [Some(Party::Client), Some(Party::Server), None]);
    assert_eq!(found[1].detail, "the server's");
    assert_eq!(found[2].detail, "nobody's");
    assert_ne!(
        found[0].detail, "a later client finding",
        "the first of the party's findings"
    );

    // A failing row with no finding to attribute is still a failure.
    let row = report
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-003")
        .unwrap();
    row.findings.clear();
    let found = failures_by_party("t.jsonl", &report);
    let bare = found
        .iter()
        .find(|failure| failure.requirement == "BASE-003")
        .unwrap();
    assert_eq!((bare.party, bare.detail.as_str()), (None, ""));
}
