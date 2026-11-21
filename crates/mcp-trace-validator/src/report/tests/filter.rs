// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The display filter: which clauses it lists, how it words what it hides, and
//! that it changes nothing a report decides.

use super::super::*;
use crate::fixtures::{baselined_sdk_session, sdk_session};

fn filter(areas: &[&str], levels: &[&str]) -> Filter {
    Filter::new(
        areas.iter().map(|&a| a.to_owned()).collect(),
        levels.iter().map(|&l| l.to_owned()).collect(),
    )
}

#[test]
fn an_area_is_the_id_prefix_and_an_empty_list_selects_everything() {
    assert_eq!(area_of("TOOL-006"), "TOOL");
    assert_eq!(area_of("no-dash-here"), "no");
    assert_eq!(area_of("nodash"), "");
    let f = filter(&["BASE", "TOOL"], &[]);
    assert!(f.matches("BASE-086", "SHOULD") && f.matches("TOOL-001", "MUST NOT"));
    assert!(!f.matches("LIFE-001", "MUST"));
    assert!(
        !f.matches("BASE", "MUST"),
        "an ID without a number has no area to match"
    );
    let both = filter(&["BASE"], &["MUST", "MUST NOT"]);
    assert!(both.matches("BASE-001", "MUST NOT"));
    assert!(!both.matches("BASE-086", "SHOULD"));
    assert!(!both.matches("TOOL-001", "MUST"));
    assert!(Filter::default().is_empty() && Filter::default().matches("X-1", "MAY"));
    assert!(!f.is_empty());
}

#[test]
fn it_describes_itself_and_words_how_many_findings_it_leaves_out() {
    assert_eq!(
        filter(&["BASE", "TOOL"], &["MUST", "SHOULD"]).describe(),
        "BASE, TOOL clauses at MUST, SHOULD"
    );
    assert_eq!(filter(&[], &["MAY"]).describe(), "all clauses at MAY");
    assert_eq!(filter(&["LIFE"], &[]).describe(), "LIFE clauses");
    let f = filter(&["BASE"], &[]);
    assert!(f.note(0).contains("no finding is in the other clauses."));
    assert!(
        f.note(1)
            .contains("1 finding in another clause is not shown.")
    );
    assert!(
        f.note(31)
            .contains("31 findings in other clauses are not shown.")
    );
    assert!(
        f.note(2)
            .contains("The totals and the verdict count every clause and every finding.")
    );
}

#[test]
fn a_filtered_report_lists_the_selected_rows_and_keeps_the_totals_and_the_verdict() {
    let judgment = sdk_session();
    let report = &judgment.reports[0];
    let options = HumanOptions::default().all(false);
    let plain = report.render_with(&options);
    let kept = report.render_with_filter(&options, &filter(&["BASE"], &[]));
    let dropped = report.render_with_filter(&options, &filter(&["TOOL"], &[]));
    assert!(kept.contains("WARN  BASE-086") && !dropped.contains("WARN  BASE-086"));
    for text in [&kept, &dropped] {
        assert!(text.contains("totals: "), "{text}");
        assert!(text.ends_with("verdict: pass-with-warnings\n"), "{text}");
    }
    assert!(
        dropped.contains("2 findings in other clauses are not shown."),
        "{dropped}"
    );
    assert!(
        kept.contains("no finding is in the other clauses."),
        "{kept}"
    );
    // The default filter is the report as ever.
    assert_eq!(
        report.render_with_filter(&options, &Filter::default()),
        plain
    );
    assert!(!plain.contains("display filter"));
}

#[test]
fn a_filter_does_not_hide_what_a_baseline_accepted_in_the_clauses_it_lists() {
    let judgment = baselined_sdk_session("known");
    let options = HumanOptions::default().all(false);
    let text = judgment.reports[0].render_with_filter(&options, &filter(&["BASE"], &["SHOULD"]));
    assert!(text.contains("[accepted]"), "{text}");
    assert!(
        text.ends_with("verdict (baseline applied): pass\n"),
        "{text}"
    );
}
