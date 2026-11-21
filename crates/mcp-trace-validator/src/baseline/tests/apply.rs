// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Applying a baseline: what it accepts, what it leaves, and what it says.

use super::*;
use crate::report::{BaselineTally, Finding, Report};

/// The fingerprint of the BASE-086 finding for tool `name`.
fn print(judgment: &Judgment, name: &str) -> String {
    let recorded = record(&[(Some("t.jsonl"), &judgment.reports)], None, None);
    recorded
        .baseline
        .entries()
        .iter()
        .find(|entry| entry.fingerprint.contains(&format!("tool \"{name}\"")))
        .unwrap()
        .fingerprint
        .clone()
}

fn only(fingerprint: &str, reason: &str) -> Baseline {
    Baseline::new(vec![Entry::new(fingerprint, reason)])
}

#[test]
fn an_accepted_finding_stays_in_the_report_marked_and_stops_counting() {
    let mut judgment = sdk_session();
    let baseline = accepting(&judgment, "t.jsonl", "the SDK's generator emits draft-07");
    let applied = apply_to(&baseline, &mut judgment, "t.jsonl", None).unwrap();
    assert!(applied.notes.is_empty() && !applied.notes.fails());
    assert_eq!(
        applied.tally,
        BaselineTally {
            accepted: 2,
            unaccepted: 0,
            unreviewed: 0
        }
    );

    let report = &judgment.reports[0];
    assert!(report.baseline_applied);
    // The facts of the trace are untouched…
    assert_eq!(report.totals.warn, 1);
    assert_eq!(report.verdict(), Verdict::PassWithWarnings);
    let row = report
        .requirements
        .iter()
        .find(|row| row.id == "BASE-086")
        .unwrap();
    assert_eq!(row.findings.len(), 2, "both findings are still listed");
    // …each finding says why it does not count…
    for finding in &row.findings {
        let acceptance = finding.baselined.as_ref().unwrap();
        assert_eq!(acceptance.reason, "the SDK's generator emits draft-07");
        assert_eq!(acceptance.expires, None);
        assert!(!report.counts(finding));
    }
    // …and only the verdict stops counting them.
    assert_eq!(report.effective_verdict(), Verdict::Pass);
    assert_eq!(report.baseline_tally(), applied.tally);
}

#[test]
fn a_new_finding_is_never_hidden_by_a_baseline_for_its_neighbours() {
    // The baseline was written when tools a and b had the defect…
    let before = sdk_session();
    let baseline = accepting(&before, "t.jsonl", "known");
    // …and a third tool now has it too.
    let mut now = judged(&[("a", DRAFT_07), ("b", DRAFT_07), ("c", DRAFT_07)]);
    let applied = apply_to(&baseline, &mut now, "t.jsonl", None).unwrap();
    assert_eq!(
        applied.tally,
        BaselineTally {
            accepted: 2,
            unaccepted: 1,
            unreviewed: 0
        }
    );
    let report = &now.reports[0];
    let row = report
        .requirements
        .iter()
        .find(|row| row.id == "BASE-086")
        .unwrap();
    let unaccepted: Vec<&str> = row
        .findings
        .iter()
        .filter(|finding| !finding.is_baselined())
        .map(|finding| finding.detail.as_str())
        .collect();
    assert_eq!(unaccepted.len(), 1);
    assert!(unaccepted[0].contains(r#"tool "c""#), "{unaccepted:?}");
    assert_eq!(report.effective_verdict(), Verdict::PassWithWarnings);
    assert!(report.counts(row.findings.iter().find(|f| !f.is_baselined()).unwrap()));
}

#[test]
fn a_must_level_finding_the_baseline_does_not_name_still_fails() {
    let mut judgment = judge::judge(&format!(
        "{}\n{}",
        session(&[("a", DRAFT_07)]),
        event(
            5,
            "client-to-server",
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
        )
    ))
    .unwrap();
    let reused = judgment.reports[0]
        .requirements
        .iter()
        .find(|row| row.id == "BASE-003")
        .map(|row| row.findings.len());
    assert_eq!(reused, Some(1), "the reused id is a MUST-level finding");
    let baseline = only(&print(&judgment, "a"), "known");
    let applied = apply_to(&baseline, &mut judgment, "t.jsonl", None).unwrap();
    assert_eq!(applied.tally.accepted, 1);
    assert!(applied.tally.unaccepted >= 1);
    assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Fail);
}

#[test]
fn an_entry_matching_nothing_is_stale_and_fails_unless_allowed() {
    let mut judgment = sdk_session();
    let gone = Entry::new(
        "BASE-086:base.schemas-use-2020-12:the server sent tool \"gone\":1",
        "was fixed",
    );
    let mut entries = accepting(&judgment, "t.jsonl", "known").entries().to_vec();
    entries.push(gone.clone());
    let applied = apply_to(&Baseline::new(entries), &mut judgment, "t.jsonl", None).unwrap();
    assert_eq!(applied.notes.stale, [gone]);
    assert_eq!(applied.notes.lapsed, []);
    assert!(applied.notes.fails());
    assert!(!applied.notes.clone().allowing_stale(true).fails());
    assert!(
        !applied.notes.allowing_stale(true).is_empty(),
        "still listed"
    );
    // Staleness does not touch the verdict: that is the findings' alone.
    assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Pass);
}

#[test]
fn an_empty_baseline_accepts_nothing_and_marks_the_report_as_baselined() {
    let mut judgment = sdk_session();
    let applied = apply_to(&Baseline::default(), &mut judgment, "t.jsonl", None).unwrap();
    assert!(applied.notes.is_empty());
    assert_eq!(applied.tally.accepted, 0);
    assert_eq!(applied.tally.unaccepted, 2);
    assert!(judgment.reports[0].baseline_applied);
    assert_eq!(
        judgment.reports[0].effective_verdict(),
        Verdict::PassWithWarnings
    );
}

#[test]
fn an_entry_accepts_through_its_last_day_and_not_after() {
    for (today, accepted) in [
        ("2026-10-24", true),
        ("2026-10-25", true),
        ("2026-10-26", false),
    ] {
        let mut judgment = sdk_session();
        let fingerprint = print(&judgment, "a");
        let baseline = Baseline::new(vec![
            Entry::new(fingerprint, "until the SDK ships").expiring(day("2026-10-25")),
        ]);
        let applied = apply_to(&baseline, &mut judgment, "t.jsonl", Some(day(today))).unwrap();
        let row = judgment.reports[0]
            .requirements
            .iter()
            .find(|r| r.id == "BASE-086")
            .unwrap();
        let first = &row.findings[0];
        assert_eq!(first.is_baselined(), accepted, "on {today}");
        if accepted {
            assert_eq!(
                first.baselined.as_ref().unwrap().expires.as_deref(),
                Some("2026-10-25")
            );
            assert!(applied.notes.lapsed.is_empty(), "{today}");
        } else {
            // The finding counts as it would without the entry, and the entry
            // is named: it matched something, so it is lapsed and not stale.
            assert_eq!(applied.notes.lapsed.len(), 1, "{today}");
            assert!(applied.notes.stale.is_empty(), "{today}");
            assert!(
                !applied.notes.fails(),
                "a lapse is the finding's cost, not a second one"
            );
            assert!(judgment.reports[0].counts(first));
        }
    }
}

#[test]
fn a_lapsed_entry_that_matches_nothing_is_stale_not_lapsed() {
    let mut judgment = sdk_session();
    let baseline = Baseline::new(vec![
        Entry::new("BASE-086:no:such:1", "old").expiring(day("2020-01-01")),
    ]);
    let applied = apply_to(&baseline, &mut judgment, "t.jsonl", Some(day("2026-10-25"))).unwrap();
    assert_eq!(applied.notes.stale.len(), 1);
    assert_eq!(applied.notes.lapsed, []);
}

#[test]
fn an_entry_with_a_date_cannot_be_applied_without_knowing_today() {
    let mut judgment = sdk_session();
    let baseline = Baseline::new(vec![Entry::new("A:1", "r").expiring(day("2999-01-01"))]);
    let error = apply_to(&baseline, &mut judgment, "t.jsonl", None).unwrap_err();
    assert_eq!(error, BaselineError::NoDate);
    assert!(error.to_string().contains("today's date"), "{error}");
    // Nothing was marked on the way to the refusal.
    assert!(!judgment.reports[0].baseline_applied);
    // A baseline with no dates needs none.
    assert!(apply_to(&only("A:1", "r"), &mut judgment, "t.jsonl", None).is_ok());
}

#[test]
fn the_generated_reason_is_counted_as_unreviewed() {
    let mut judgment = sdk_session();
    let mut entries = record(&[(None, &judgment.reports)], None, None)
        .baseline
        .entries()
        .to_vec();
    entries[1].reason = "reviewed by hand".to_owned();
    let applied = apply_to(&Baseline::new(entries), &mut judgment, "t.jsonl", None).unwrap();
    // The traces are named differently from the recording above, which does not
    // matter: a trace's name is no part of a fingerprint.
    assert_eq!(
        applied.tally,
        BaselineTally {
            accepted: 2,
            unaccepted: 0,
            unreviewed: 1
        }
    );
    let text = applied.tally.describe();
    assert!(
        text.contains("2 findings accepted, 0 not accepted"),
        "{text}"
    );
    assert!(
        text.contains("1 of the accepted carry the reason --write-baseline wrote"),
        "{text}"
    );
    assert!(
        !BaselineTally {
            accepted: 1,
            ..BaselineTally::default()
        }
        .describe()
        .contains(';')
    );
    assert!(
        BaselineTally {
            accepted: 1,
            ..BaselineTally::default()
        }
        .describe()
        .starts_with("1 finding accepted")
    );
}

#[test]
fn one_entry_accepts_the_same_finding_in_every_trace_and_is_stale_in_none() {
    let mut first = sdk_session();
    let mut second = sdk_session();
    let baseline = accepting(&first, "a.jsonl", "known");
    let mut traces = [
        TraceReports {
            name: Some("a.jsonl"),
            reports: &mut first.reports,
        },
        TraceReports {
            name: Some("b.jsonl"),
            reports: &mut second.reports,
        },
    ];
    let applied = baseline.apply(&mut traces, None).unwrap();
    assert_eq!(applied.notes.stale, []);
    assert_eq!(
        applied.tally.accepted, 4,
        "two findings in each of two traces"
    );
    assert_eq!(first.reports[0].effective_verdict(), Verdict::Pass);
    assert_eq!(second.reports[0].effective_verdict(), Verdict::Pass);
}

#[test]
fn an_entry_is_stale_only_if_it_matches_in_no_trace_of_the_run() {
    let with = sdk_session();
    let without = judged(&[("c", DRAFT_2020)]);
    let mut with = with;
    let mut without = without;
    let baseline = accepting(&with, "a.jsonl", "known");
    let mut traces = [
        TraceReports {
            name: Some("clean.jsonl"),
            reports: &mut without.reports,
        },
        TraceReports {
            name: Some("a.jsonl"),
            reports: &mut with.reports,
        },
    ];
    let applied = baseline.apply(&mut traces, None).unwrap();
    assert!(applied.notes.stale.is_empty(), "{:?}", applied.notes);
    // Against the clean trace alone, every entry is stale.
    let mut clean = judged(&[("c", DRAFT_2020)]);
    let applied = apply_to(&baseline, &mut clean, "clean.jsonl", None).unwrap();
    assert_eq!(applied.notes.stale.len(), 2);
}

#[test]
fn a_party_view_and_a_baseline_each_take_findings_out_of_the_verdict() {
    let mut judgment = sdk_session();
    judgment.reports[0].party_view = Some(Party::Client);
    // Both findings are the server's, so the client's view already sets them aside.
    assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Pass);
    judgment.reports[0].party_view = Some(Party::Server);
    assert_eq!(
        judgment.reports[0].effective_verdict(),
        Verdict::PassWithWarnings
    );
    let baseline = accepting(&judgment, "t.jsonl", "known");
    apply_to(&baseline, &mut judgment, "t.jsonl", None).unwrap();
    assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Pass);
    // `verdict_for` is the party view alone, and says so.
    assert_eq!(
        judgment.reports[0].verdict_for(Party::Server),
        Verdict::PassWithWarnings
    );
}

#[test]
fn an_extensions_finding_is_never_accepted_and_an_entry_for_it_is_stale() {
    let (mut reports, _) = crate::fixtures::undeclared_task();
    let extension_print = crate::fingerprint::Fingerprints::of(&[(Some("t.jsonl"), &reports)])
        .found()
        .iter()
        .find(|one| !one.scored)
        .unwrap()
        .fingerprint
        .clone();
    let baseline = only(&extension_print, "optional extension");
    let mut traces = [TraceReports {
        name: Some("t.jsonl"),
        reports: &mut reports,
    }];
    let applied = baseline.apply(&mut traces, None).unwrap();
    assert_eq!(applied.notes.stale.len(), 1);
    assert!(
        reports[0]
            .extensions
            .iter()
            .flat_map(|e| &e.requirements)
            .flat_map(|r| &r.findings)
            .all(|f| !f.is_baselined())
    );
}

#[test]
fn a_wire_schema_finding_is_accepted_like_a_clause_finding() {
    let mut judgment = judge::judge(&session_answering(&json!({"tools": "not a list"}))).unwrap();
    let schema_findings = judgment.reports[0]
        .wire_schema
        .as_ref()
        .unwrap()
        .findings
        .len();
    assert!(
        schema_findings >= 1,
        "{:#?}",
        judgment.reports[0].wire_schema
    );
    let baseline = accepting(&judgment, "t.jsonl", "known");
    assert!(
        baseline
            .entries()
            .iter()
            .any(|e| e.fingerprint.starts_with("wire-schema:")),
        "{:#?}",
        baseline.entries()
    );
    apply_to(&baseline, &mut judgment, "t.jsonl", None).unwrap();
    let schema = judgment.reports[0].wire_schema.as_ref().unwrap();
    assert!(schema.findings.iter().all(Finding::is_baselined));
    // The schema's outcome is the trace's fact; the verdict is what changes.
    assert_eq!(schema.outcome, crate::report::Outcome::Fail);
    assert_eq!(judgment.reports[0].verdict(), Verdict::Fail);
    assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Pass);
}

#[test]
fn a_report_serializes_its_baseline_counts_only_when_one_was_applied() {
    let mut judgment = sdk_session();
    let plain = serde_json::to_value(&judgment.reports[0]).unwrap();
    assert!(plain.get("baseline").is_none(), "no baseline, no member");
    let row = |json: &Value| {
        json["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "BASE-086")
            .unwrap()
            .clone()
    };
    assert!(row(&plain)["findings"][0].get("baselined").is_none());

    let baseline = accepting(&judgment, "t.jsonl", "known");
    apply_to(&baseline, &mut judgment, "t.jsonl", None).unwrap();
    let json = serde_json::to_value(&judgment.reports[0]).unwrap();
    assert_eq!(
        json["baseline"],
        json!({"accepted": 2, "unaccepted": 0, "unreviewed": 0})
    );
    assert_eq!(json["verdict"], "pass");
    assert_eq!(
        row(&json)["findings"][0]["baselined"],
        json!({"reason": "known"})
    );
    // `baseline` precedes the verdict it explains.
    let text = serde_json::to_string(&judgment.reports[0]).unwrap();
    assert!(text.find("\"baseline\"").unwrap() < text.find("\"verdict\"").unwrap());
    // And reads back as a baselined report.
    let back: Report = serde_json::from_value(json).unwrap();
    assert!(back.baseline_applied);
    // From text too, where the member's value has to be read past for the
    // members after it to parse.
    let reread: Report = serde_json::from_str(&text).unwrap();
    assert!(reread.baseline_applied);
    assert_eq!(reread, back);
    assert!(
        back.requirements
            .iter()
            .flat_map(|r| &r.findings)
            .all(|f| f.baselined.is_some() || f.detail.is_empty())
    );
}
