// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A baseline applied to a trace judged under several revisions: the
//! per-revision reports are marked, and the aligned report adopts the marks.

use mcp_conformance_core::requirement::RegistrySet;

use super::*;
use crate::multi::{self, MultiReport};
use crate::report::Report;

/// The SDK session judged at both revisions: one report each, and the aligned
/// report beside them, as `validate --revision A --revision B` builds them.
fn both() -> (Vec<Report>, MultiReport) {
    let set = RegistrySet::builtin().unwrap();
    let revisions = ["2025-11-25", "2026-07-28"].map(|r| r.parse().unwrap());
    let judgment = judge::judge(&crate::fixtures::session(&[("a", DRAFT_07)])).unwrap();
    let reports: Vec<Report> = revisions
        .iter()
        .map(|revision| {
            crate::engine::validate(&set.registry(*revision).unwrap(), &judgment.events)
        })
        .collect();
    let aligned = multi::validate_revisions(&set, &revisions, &judgment.events).unwrap();
    (reports, aligned)
}

#[test]
fn the_aligned_report_adopts_the_marks_and_each_revisions_verdict() {
    let (mut reports, mut aligned) = both();
    assert!(aligned.baseline_tally().accepted == 0 && !aligned.baseline_applied);
    let before = aligned.verdict();
    assert_ne!(
        before,
        Verdict::Pass,
        "the session has findings at both revisions"
    );
    let recorded = record(&[(Some("t.jsonl"), &reports)], None, None);
    assert!(recorded.added >= 2, "{recorded:?}");
    let mut traces = [TraceReports::new(Some("t.jsonl"), &mut reports)];
    let applied = recorded.baseline.apply(&mut traces, None).unwrap();
    assert!(applied.notes.is_empty());
    aligned.adopt_baseline(&reports);

    assert!(aligned.baseline_applied);
    assert_eq!(aligned.verdict(), Verdict::Pass);
    for (summary, report) in aligned.summaries.iter().zip(&reports) {
        assert_eq!(summary.verdict, report.effective_verdict());
    }
    // Every finding of every revision is on its row, marked.
    let rows: Vec<_> = aligned
        .requirements
        .iter()
        .flat_map(|row| row.findings.iter().flatten())
        .collect();
    assert!(rows.len() >= 2);
    assert!(
        rows.iter().all(|finding| finding.is_baselined()),
        "{rows:#?}"
    );
    // The aligned tally is the reports' own, summed.
    let summed = reports
        .iter()
        .fold(crate::report::BaselineTally::default(), |sum, report| {
            sum.plus(report.baseline_tally())
        });
    assert_eq!(aligned.baseline_tally(), summed);
    assert_eq!(aligned.baseline_tally().unaccepted, 0);
}

#[test]
fn the_aligned_report_says_so_in_the_terminal_and_in_json() {
    let (mut reports, mut aligned) = both();
    let recorded = record(&[(None, &reports)], None, None);
    let mut traces = [TraceReports::new(None, &mut reports)];
    recorded.baseline.apply(&mut traces, None).unwrap();
    aligned.adopt_baseline(&reports);
    let text = aligned.render_with(&crate::report::HumanOptions::default().all(false));
    assert!(text.contains("baseline: "), "{text}");
    assert!(text.contains("[accepted] "), "{text}");
    assert!(text.contains("accepted by baseline ("), "{text}");
    assert!(
        text.ends_with("overall verdict (baseline applied): pass\n"),
        "{text}"
    );
    for line in text
        .lines()
        .filter(|line| line.starts_with("  2025-11-25: ") || line.starts_with("  2026-07-28: "))
    {
        if line.contains("verdict ") {
            assert!(line.ends_with("(baseline applied)"), "{line}");
        }
    }
    let json = serde_json::to_value(&aligned).unwrap();
    assert!(json["baseline"]["accepted"].as_u64().unwrap() >= 2);
    assert_eq!(json["baseline"]["unaccepted"], 0);
    assert_eq!(json["verdict"], "pass");
    // And it reads back as a baselined report.
    let back: MultiReport = serde_json::from_value(json).unwrap();
    assert!(back.baseline_applied);
}

#[test]
fn a_multi_revision_report_with_no_baseline_has_no_baseline_member_or_wording() {
    let (_, aligned) = both();
    let json = serde_json::to_value(&aligned).unwrap();
    assert!(json.get("baseline").is_none());
    let text = aligned.render_with(&crate::report::HumanOptions::default().all(false));
    assert!(
        !text.contains("baseline") && !text.contains("accepted"),
        "{text}"
    );
}
