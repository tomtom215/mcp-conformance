// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the projection and the claim parser.
//!
//! The claim parser has its own tests beside it in `claims.rs`. The end-to-end
//! wiring — that the committed `corpus/README.md` block matches the committed
//! reports, and that every claim in the shipped Markdown agrees with them — is
//! not asserted here but *run*: `cargo xtask draft-coverage --check` is a step
//! of `cargo xtask ci`, against the real files. A test that re-read those files
//! would be the same check with a worse failure message.

use super::*;

fn report(rows: &[(&str, &str)]) -> GoldenReport {
    report_excluding(0, rows)
}

/// A golden with `excluded` in its totals — where the count now comes from,
/// the rows themselves having moved to the revision's exclusion ledger.
fn report_excluding(excluded: u32, rows: &[(&str, &str)]) -> GoldenReport {
    GoldenReport {
        totals: GoldenTotals { excluded },
        requirements: rows
            .iter()
            .map(|(id, outcome)| GoldenRow {
                id: (*id).to_owned(),
                outcome: (*outcome).to_owned(),
            })
            .collect(),
    }
}

#[test]
fn only_judged_outcomes_are_counted() {
    // The un-judgeable outcomes are the point: a clause whose check was not
    // compiled in, and one gated on a capability the session never negotiated,
    // are absent from both sets, so neither the numerator nor the denominator
    // can be inflated by them. The registry's exclusions are un-judgeable on
    // the same terms and are not rows at all — the non-zero total here must not
    // reach any count below.
    let capture = tally(
        "c".to_owned(),
        &report_excluding(
            3,
            &[
                ("BASE-001", "pass"),
                ("BASE-002", "pass"),
                ("BASE-003", "fail"),
                ("BASE-004", "warn"),
                ("BASE-005", "not-observed"),
                ("BASE-007", "unsupported"),
                ("BASE-008", "not-applicable"),
            ],
        ),
    );
    assert_eq!((capture.pass, capture.fail, capture.warn), (2, 1, 1));
    assert_eq!(capture.judged(), 4, "pass, fail and warn are all judged");
    assert_eq!(capture.not_observed.len(), 1);
    assert_eq!(capture.judgeable(), 5);
    assert_eq!(capture.excluded, 3);
}

#[test]
fn the_excluded_count_is_read_from_totals_and_not_from_rows() {
    // A golden carries no `excluded` rows — the revision owns that set, in
    // `corpus/golden/exclusions/` (ADR-0013). Counting rows would report 0 and
    // every "148 excluded" verdict quoted in `corpus/README.md` would stop
    // matching a committed report, silently, since the claim check treats an
    // absent quote as nothing to disagree with.
    let capture = tally(
        "c".to_owned(),
        &report_excluding(148, &[("BASE-001", "pass")]),
    );
    assert_eq!(capture.excluded, 148);
    assert_eq!(
        capture.judged(),
        1,
        "an exclusion is not a clause the capture evidenced"
    );
    assert_eq!(
        tally("empty".to_owned(), &report(&[])).excluded,
        0,
        "and a revision that excludes nothing reports nothing"
    );
}

#[test]
fn the_union_is_of_clauses_and_not_of_counts() {
    // Two captures judging the *same* clause evidence one clause between them,
    // not two. Summing the per-capture counts would say three here, which is
    // exactly the overstatement this whole task exists to prevent.
    let first = tally(
        "a".to_owned(),
        &report(&[
            ("BASE-001", "pass"),
            ("BASE-002", "pass"),
            ("BASE-003", "not-observed"),
        ]),
    );
    let second = tally(
        "b".to_owned(),
        &report(&[
            ("BASE-001", "pass"),
            ("BASE-002", "not-observed"),
            ("BASE-003", "not-observed"),
        ]),
    );
    let summary = Summary::of(&[first, second]);
    assert_eq!(summary.observed, 2);
    assert_eq!(summary.judgeable, 3);
    assert_eq!(
        summary.never,
        vec!["BASE-003".to_owned()],
        "unevidenced by *either* capture, not merely by one"
    );
    assert_eq!(
        summary.allowed(),
        BTreeSet::from([1, 2]),
        "the union and each capture's judged count"
    );
}

#[test]
fn a_clause_judged_by_one_capture_is_not_unevidenced() {
    let summary = Summary::of(&[
        tally("a".to_owned(), &report(&[("BASE-001", "not-observed")])),
        tally("b".to_owned(), &report(&[("BASE-001", "fail")])),
    ]);
    assert_eq!(summary.observed, 1);
    assert!(summary.never.is_empty(), "{:?}", summary.never);
}

#[test]
fn the_table_carries_every_capture_and_the_union() {
    let captures = vec![
        tally(
            "alpha".to_owned(),
            &report(&[("BASE-001", "pass"), ("BASE-002", "not-observed")]),
        ),
        tally("beta".to_owned(), &report(&[("BASE-002", "fail")])),
    ];
    let summary = Summary::of(&captures);
    let table = render(&captures, &summary);
    assert!(
        table.contains("| `alpha` | 1 | 1 | 0 | 0 | 1 |\n"),
        "{table}"
    );
    assert!(
        table.contains("| `beta` | 1 | 0 | 1 | 0 | 0 |\n"),
        "{table}"
    );
    // The union row's "not observed" is the count no capture reached, which is
    // zero here even though `alpha` did not observe `BASE-002`.
    assert!(
        table.contains("| **Union** | **2** | | | | **0** |\n"),
        "{table}"
    );
    assert!(
        table.contains("**2 of the 2 judgeable clauses**"),
        "the sentence uses the phrasing the claim check parses: {table}"
    );
    assert!(table.contains("No clause goes unevidenced."), "{table}");
}

#[test]
fn unevidenced_clauses_are_named_rather_than_counted() {
    // A count alone cannot be acted on; the ids are what turns "15 remain" into
    // a work list.
    let captures = vec![tally(
        "alpha".to_owned(),
        &report(&[("BASE-001", "pass"), ("TRAN-079", "not-observed")]),
    )];
    let summary = Summary::of(&captures);
    let table = render(&captures, &summary);
    assert!(
        table.contains("The 1 clause no capture reaches: `TRAN-079`."),
        "{table}"
    );
}

#[test]
fn the_block_replaces_only_what_lies_between_the_markers() {
    let readme = format!("before\n\n{BEGIN}\nstale\n{END}\n\nafter\n");
    let expected = format!("before\n\n{BEGIN}\nfresh\n{END}\n\nafter\n");
    assert_eq!(splice(&readme, "fresh\n"), Some(expected));
}

#[test]
fn a_readme_without_markers_is_an_error_rather_than_an_append() {
    assert!(splice("no markers here\n", "fresh\n").is_none());
    // Half a pair is still a missing pair: appending after `BEGIN` with no
    // `END` would swallow the rest of the file.
    assert!(splice(&format!("{BEGIN}\nbody\n"), "fresh\n").is_none());
}
