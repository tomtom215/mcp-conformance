// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `validate --area` and `--level` (ADR-0025): what the human report lists, and
//! nothing it decides.
//!
//! A filter that could change a verdict would be a way to turn a failing run
//! green. These tests pin that it cannot — the verdict line, the totals and the
//! exit status are those of the unfiltered run — that the report says how much
//! the filter hides, and that `requirements` parses the same flags the same way.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap()
}

fn corpus(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
        .to_str()
        .unwrap()
        .to_owned()
}

/// A recording with many findings across many areas: 37 findings in 11 clauses
/// at `2026-07-28`, a deliberately sloppy client's.
fn probe() -> String {
    corpus("draft/captured/probe-2026-07-28-http.jsonl")
}

/// The IDs of the clause rows a human report lists (`  FAIL  BASE-030 (MUST)`).
fn listed(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let marker = words.next()?;
            let id = words.next()?;
            (["FAIL", "WARN"].contains(&marker) && id.contains('-') && line.starts_with("  "))
                .then(|| id.to_owned())
        })
        .collect()
}

fn line_starting<'a>(text: &'a str, start: &str) -> &'a str {
    text.lines().find(|line| line.starts_with(start)).unwrap()
}

#[test]
fn a_filter_lists_fewer_clauses_and_changes_nothing_the_report_decides() {
    let plain = run(&["validate", &probe()]);
    let filtered = run(&["validate", &probe(), "--area", "BASE", "--level", "should"]);
    assert_eq!(code(&plain), 1);
    assert_eq!(
        code(&filtered),
        1,
        "the exit status is the unfiltered run's"
    );
    let (all, some) = (stdout(&plain), stdout(&filtered));

    assert_eq!(listed(&some), ["BASE-033"], "{some}");
    assert!(listed(&all).len() > 5, "{all}");
    // The totals, the wire-schema line and the verdict are word for word the same.
    for start in ["totals: ", "wire schema: ", "verdict: "] {
        assert_eq!(line_starting(&some, start), line_starting(&all, start));
    }
    // And the report says what it left out, at the top and above the totals.
    // 37 findings in all: 11 are BASE-033's, one is the wire schema's, which is
    // not a clause and so is not "in another clause"; 25 are left.
    let note = "listing only BASE clauses at SHOULD (a display filter): 25 findings in other clauses are not shown.";
    assert_eq!(some.matches(note).count(), 2, "{some}");
    assert!(some.contains("The totals and the verdict count every clause and every finding."));
    assert_eq!(
        all.matches("] seq ").count(),
        37,
        "the unfiltered report lists every finding"
    );
}

#[test]
fn a_filter_that_matches_nothing_lists_nothing_and_still_fails_the_run() {
    // `ELIC` is an area of the judged revision, and the recording has no finding in it.
    let output = run(&["validate", &probe(), "--area", "ELIC"]);
    assert_eq!(
        code(&output),
        1,
        "a filter can never turn a failing run green"
    );
    let text = stdout(&output);
    assert_eq!(listed(&text), Vec::<String>::new(), "{text}");
    assert!(
        text.contains("findings in other clauses are not shown"),
        "{text}"
    );
    assert!(text.ends_with("verdict: fail\n"), "{text}");
}

#[test]
fn the_wire_schema_is_not_a_clause_and_no_filter_hides_it() {
    let text = stdout(&run(&["validate", &probe(), "--area", "ELIC"]));
    assert!(text.contains("FAIL  wire schema"), "{text}");
}

#[test]
fn both_spellings_of_a_list_mean_the_same_and_case_does_not_matter() {
    let repeated = stdout(&run(&[
        "validate",
        &probe(),
        "--area",
        "BASE",
        "--area",
        "TRAN",
        "--level",
        "must",
    ]));
    let comma = stdout(&run(&[
        "validate",
        &probe(),
        "--area",
        "base,tran",
        "--level",
        "MUST",
    ]));
    assert_eq!(repeated, comma);
    assert_eq!(
        listed(&repeated),
        ["BASE-030", "TRAN-071", "TRAN-072", "TRAN-079", "TRAN-130"]
    );
    let words = stdout(&run(&["validate", &probe(), "--level", "Must,SHOULD"]));
    assert_eq!(
        listed(&words),
        listed(&stdout(&run(&["validate", &probe()])))
    );
}

#[test]
fn every_clause_listing_is_narrowed_under_all_too() {
    let output = run(&[
        "validate",
        &probe(),
        "--all",
        "--area",
        "BASE",
        "--level",
        "must",
    ]);
    let text = stdout(&output);
    let rows: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("  ") && line.contains(" (MUST)"))
        .collect();
    assert!(
        !rows.is_empty() && rows.iter().all(|row| row.contains(" BASE-")),
        "{text}"
    );
    assert!(!text.contains(" (SHOULD)"), "{text}");
}

#[test]
fn a_filter_with_a_machine_format_is_refused_rather_than_ignored() {
    for format in ["json", "junit", "sarif", "html"] {
        let output = run(&["validate", &probe(), "--area", "BASE", "--format", format]);
        assert_eq!(code(&output), 2, "{format}");
        assert!(
            stderr(&output).contains("always carry every clause and every finding"),
            "{}",
            stderr(&output)
        );
        assert_eq!(stdout(&output), "", "{format}: no document is written");
    }
    let level = run(&["validate", &probe(), "--level", "must", "--format", "json"]);
    assert_eq!(code(&level), 2);
    // No filter, no refusal.
    assert_eq!(code(&run(&["validate", &probe(), "--format", "json"])), 1);
}

#[test]
fn an_area_no_judged_revision_has_is_an_error_and_not_an_empty_report() {
    let output = run(&["validate", &probe(), "--area", "TOLL"]);
    assert_eq!(code(&output), 2);
    assert!(
        stderr(&output).contains("no judged revision has an area TOLL"),
        "{}",
        stderr(&output)
    );
    assert_eq!(stdout(&output), "");
    // An area of the other revision is not this revision's either.
    let other = run(&["validate", &probe(), "--area", "LIFE"]);
    assert_eq!(code(&other), 2, "{}", stderr(&other));
}

#[test]
fn a_filter_applies_to_the_multi_revision_listing_and_to_several_traces() {
    let both = run(&[
        "validate",
        &probe(),
        "--revision",
        "2025-11-25",
        "--revision",
        "2026-07-28",
        "--area",
        "BASE",
    ]);
    assert_eq!(code(&both), 1);
    let text = stdout(&both);
    assert!(text.contains("(a display filter)"), "{text}");
    let starts = |prefix: &str| text.lines().filter(|line| line.starts_with(prefix)).count();
    assert!(starts("  BASE-") > 0, "{text}");
    assert_eq!(starts("  TRAN-") + starts("  TOOL-"), 0, "{text}");
    assert!(text.contains("overall verdict: fail"), "{text}");

    let two = run(&[
        "validate",
        &probe(),
        &corpus("good/stdio-full-session.jsonl"),
        "--area",
        "TRAN",
    ]);
    let text = stdout(&two);
    assert_eq!(code(&two), 1);
    // Each trace's report carries the note, twice; the clean trace has nothing
    // hidden, and says so.
    assert_eq!(text.matches("(a display filter)").count(), 4, "{text}");
    assert!(
        text.contains("32 findings in other clauses are not shown"),
        "{text}"
    );
    assert!(
        text.contains("no finding is in the other clauses"),
        "{text}"
    );
}

#[test]
fn requirements_takes_the_same_flags_in_the_same_forms() {
    let ids = |args: &[&str]| -> Vec<String> {
        let mut all = vec![
            "requirements",
            "--revision",
            "2025-11-25",
            "--format",
            "json",
        ];
        all.extend_from_slice(args);
        let json: serde_json::Value = serde_json::from_str(&stdout(&run(&all))).unwrap();
        json["requirements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_str().unwrap().to_owned())
            .collect()
    };
    let repeated = ids(&[
        "--area", "LIFE", "--area", "PAGE", "--level", "should", "--level", "must",
    ]);
    let comma = ids(&["--area", "life,page", "--level", "SHOULD,Must"]);
    assert_ne!(repeated, Vec::<String>::new());
    assert_eq!(repeated, comma);
    assert!(ids(&["--level", "MUST-NOT"]).len() > 5);
    let unknown = run(&[
        "requirements",
        "--revision",
        "2025-11-25",
        "--area",
        "LIFE,NOPE",
    ]);
    assert_eq!(code(&unknown), 2);
    assert!(
        stderr(&unknown).contains("has no area NOPE"),
        "{}",
        stderr(&unknown)
    );
}
