// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the baseline: its file, applying it, and recording one.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};

use super::*;
use crate::fixtures::{
    DRAFT_07, DRAFT_2020, event, judged, sdk_session, session, session_answering,
};
use crate::judge::{self, Judgment};
use crate::report::{Party, Verdict};

mod apply;
mod file;
mod multi;
mod record;

/// A baseline accepting every finding of `judgment` for `reason`.
fn accepting(judgment: &Judgment, name: &str, reason: &str) -> Baseline {
    let recorded = record(&[(Some(name), &judgment.reports)], None, None);
    Baseline::new(
        recorded
            .baseline
            .entries()
            .iter()
            .map(|entry| Entry::new(entry.fingerprint.clone(), reason))
            .collect(),
    )
}

/// `baseline` applied to one trace's reports.
fn apply_to(
    baseline: &Baseline,
    judgment: &mut Judgment,
    name: &str,
    today: Option<Date>,
) -> Result<Applied, BaselineError> {
    let mut traces = [TraceReports {
        name: Some(name),
        reports: &mut judgment.reports,
    }];
    baseline.apply(&mut traces, today)
}

fn day(text: &str) -> Date {
    text.parse().unwrap()
}

#[test]
fn the_fixture_session_has_exactly_the_findings_the_tests_count_on() {
    let judgment = sdk_session();
    let report = &judgment.reports[0];
    let findings: Vec<(&str, &str)> = report
        .requirements
        .iter()
        .flat_map(|row| {
            row.findings
                .iter()
                .map(move |f| (row.id.as_str(), f.detail.as_str()))
        })
        .collect();
    assert_eq!(findings.len(), 2, "{findings:#?}");
    assert!(findings.iter().all(|(id, _)| *id == "BASE-086"));
    assert!(findings[0].1.contains(r#"tool "a""#) && findings[1].1.contains(r#"tool "b""#));
    assert_eq!(report.verdict(), Verdict::PassWithWarnings);
    assert_eq!(
        report
            .requirements
            .iter()
            .filter(|r| !r.findings.is_empty())
            .count(),
        1
    );
    assert_eq!(report.wire_schema.as_ref().unwrap().findings, []);
    // And the sender is the server, which holds the finding.
    assert!(
        report
            .requirements
            .iter()
            .flat_map(|r| &r.findings)
            .all(|f| f.party == Some(Party::Server))
    );
}
