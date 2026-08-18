// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the empty-run refusal.
//!
//! Routed only through the binary, every arm of the condition and both halves
//! of the judged sum survived mutation: the committed corpus happens to contain
//! no trace that separates them, which is exactly what a corpus cannot be
//! relied on to do.

#![allow(clippy::unwrap_used)]

use super::{combined, refusal};
use mcp_trace_validator::multi::MultiReport;
use mcp_trace_validator::report::Totals;

/// `Totals` and `MultiReport` are `#[non_exhaustive]`, so this binary — a
/// separate crate from the library — cannot build them literally. They are
/// deserialized instead, which is no worse a fixture: it is exactly the
/// shape the `--format json` output carries.
fn totals(pass: u32, fail: u32, warn: u32, unsupported: u32, not_observed: u32) -> Totals {
    serde_json::from_str(&format!(
        r#"{{"pass":{pass},"fail":{fail},"warn":{warn},"excluded":0,
             "unsupported":{unsupported},"not_applicable":0,
             "not_observed":{not_observed}}}"#
    ))
    .unwrap()
}

fn multi(first: Totals, second: Totals) -> MultiReport {
    let summary = |revision: &str, totals: Totals| {
        format!(
            r#"{{"revision":"{revision}","totals":{},"verdict":"pass"}}"#,
            serde_json::to_string(&totals).unwrap()
        )
    };
    serde_json::from_str(&format!(
        r#"{{"revisions":["2025-11-25","2026-07-28"],
             "summaries":[{},{}],"requirements":[]}}"#,
        summary("2025-11-25", first),
        summary("2026-07-28", second)
    ))
    .unwrap()
}

#[test]
fn a_run_is_accused_only_when_the_trace_is_what_judged_nothing() {
    // Judged nothing, and clauses were there to judge: the trace is empty.
    assert!(refusal(totals(0, 0, 0, 0, 38), "capture.jsonl").is_some());
    // Each escape, one at a time, so no arm of the condition can be
    // dropped or inverted without a test noticing.
    assert!(
        refusal(totals(1, 0, 0, 0, 37), "c.jsonl").is_none(),
        "a pass is something judged"
    );
    assert!(
        refusal(totals(0, 1, 0, 0, 37), "c.jsonl").is_none(),
        "so is a failure — a trace whose one judged clause failed is not empty"
    );
    assert!(
        refusal(totals(0, 0, 1, 0, 37), "c.jsonl").is_none(),
        "and so is a warning"
    );
    assert!(
        refusal(totals(0, 0, 0, 1, 37), "c.jsonl").is_none(),
        "unsupported checks are the registry's problem, not the recording's"
    );
    assert!(
        refusal(totals(0, 0, 0, 0, 0), "c.jsonl").is_none(),
        "a registry of nothing but exclusions leaves no clause for any trace to reach"
    );
}

#[test]
fn the_diagnostic_names_the_source_the_reader_passed() {
    let message = refusal(totals(0, 0, 0, 0, 1), "capture.jsonl").unwrap();
    assert!(message.contains("capture.jsonl"), "{message}");
    assert!(
        message.contains("judged no requirement at all"),
        "{message}"
    );
    // `-` is a path nobody can look at, so it is named for what it is.
    let piped = refusal(totals(0, 0, 0, 0, 1), "-").unwrap();
    assert!(piped.contains("the trace on stdin"), "{piped}");
    assert!(!piped.contains("- judged"), "{piped}");
}

#[test]
fn combined_totals_sums_every_revision_and_every_field() {
    // Distinct values per field and per revision: a dropped `+=`, or one
    // reading another field, changes the answer.
    let sum = combined(&multi(totals(1, 2, 3, 4, 5), totals(10, 20, 30, 40, 50)));
    assert_eq!(
        (
            sum.pass,
            sum.fail,
            sum.warn,
            sum.unsupported,
            sum.not_observed
        ),
        (11, 22, 33, 44, 55)
    );
    // And the sum is what the guard reads: two revisions that each judged
    // nothing are still one trace that judged nothing.
    let empty = multi(totals(0, 0, 0, 0, 7), totals(0, 0, 0, 0, 9));
    assert!(refusal(combined(&empty), "c.jsonl").is_some());
}
