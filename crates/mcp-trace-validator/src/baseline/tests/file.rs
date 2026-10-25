// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The date, the file's reader and its writer.

use super::*;
use crate::report::Acceptance;

#[test]
fn a_date_is_exactly_yyyy_mm_dd_naming_a_day_that_exists() {
    for good in [
        "2026-10-25",
        "2028-02-29",
        "0001-01-01",
        "9999-12-31",
        "2000-02-29",
    ] {
        assert_eq!(good.parse::<Date>().unwrap().to_string(), good);
    }
    for bad in [
        "2026-10-25 ",
        " 2026-10-25",
        "2026-1-5",
        "26-10-25",
        "2026/10/25",
        "2026-13-01",
        "2026-00-10",
        "2026-04-31",
        "2027-02-29",
        "1900-02-29",
        "0000-01-01",
        "+026-10-25",
        "2026-10-2x",
        "",
        "tomorrow",
    ] {
        let error = bad.parse::<Date>().unwrap_err();
        assert!(error.to_string().contains(&format!("{bad:?}")), "{error}");
    }
    assert!(day("2026-10-25") < day("2026-10-26"));
    assert!(day("2026-12-31") < day("2027-01-01"));
    assert!(day("2026-02-28") < day("2026-03-01"));
    assert!(Date::new(2024, 2, 29).is_some() && Date::new(2100, 2, 29).is_none());
}

#[test]
fn unix_days_convert_to_the_calendar_both_ways_across_the_range() {
    let known = [
        (0, "1970-01-01"),
        (20_751, "2026-10-25"),
        (11_016, "2000-02-29"),
        (-1, "1969-12-31"),
        (-719_162, "0001-01-01"),
        (2_932_896, "9999-12-31"),
    ];
    for (days, text) in known {
        assert_eq!(
            Date::from_unix_days(days).unwrap().to_string(),
            text,
            "{days}"
        );
    }
    assert_eq!(Date::from_unix_days(-719_163), None, "before year 1");
    assert_eq!(Date::from_unix_days(2_932_897), None, "after year 9999");
    assert_eq!(Date::from_unix_days(i64::MAX), None);
    assert_eq!(Date::from_unix_days(i64::MIN), None);
    // Consecutive days are consecutive dates, over a leap day and a century.
    let mut previous = Date::from_unix_days(11_000).unwrap();
    for days in 11_001..11_800 {
        let next = Date::from_unix_days(days).unwrap();
        assert!(previous < next, "{previous} then {next}");
        previous = next;
    }
}

fn file(findings: &Value) -> String {
    json!({
        "version": 1,
        "fingerprint_key": "mcpConformanceFinding/v2",
        "findings": findings,
    })
    .to_string()
}

#[test]
fn a_file_with_every_member_reads_back_as_written() {
    let text = file(&json!([
        {"fingerprint": "B:1", "reason": "later", "expires": "2027-03-31"},
        {"fingerprint": "A:1", "reason": "first"},
    ]));
    let baseline = Baseline::parse(&text).unwrap();
    assert_eq!(baseline.entries().len(), 2);
    assert_eq!(baseline.entries()[0].expires, Some(day("2027-03-31")));
    assert_eq!(baseline.entries()[1].expires, None);
    assert!(baseline.has_expiry());
    // Written sorted by fingerprint, one trailing newline, and read back equal.
    let written = baseline.to_json();
    assert!(
        written.ends_with("}\n") && !written.ends_with("\n\n"),
        "{written}"
    );
    assert!(written.find("\"A:1\"").unwrap() < written.find("\"B:1\"").unwrap());
    let again = Baseline::parse(&written).unwrap();
    assert_eq!(again.to_json(), written);
    assert!(!Baseline::new(vec![Entry::new("A:1", "r")]).has_expiry());
    // An empty baseline is a valid file: a clean run records one.
    assert_eq!(
        Baseline::parse(&file(&json!([]))).unwrap().entries().len(),
        0
    );
}

fn rejected(text: &str) -> BaselineError {
    Baseline::parse(text).unwrap_err()
}

#[test]
fn a_blank_reason_or_fingerprint_is_refused_and_names_the_entry() {
    let blank = rejected(&file(&json!([
        {"fingerprint": "A:1", "reason": "ok"},
        {"fingerprint": "B:1", "reason": " \t\n"},
    ])));
    assert!(matches!(
        &blank,
        BaselineError::Entry {
            index: 2,
            problem: EntryProblem::NoReason,
            ..
        }
    ));
    assert!(blank.to_string().contains("entry 2 (\"B:1\")"), "{blank}");
    assert!(blank.to_string().contains("say why"), "{blank}");

    let empty_reason = rejected(&file(&json!([{"fingerprint": "A:1", "reason": ""}])));
    assert!(matches!(
        empty_reason,
        BaselineError::Entry {
            problem: EntryProblem::NoReason,
            ..
        }
    ));
    let no_fingerprint = rejected(&file(&json!([{"fingerprint": "", "reason": "r"}])));
    assert!(matches!(
        no_fingerprint,
        BaselineError::Entry {
            index: 1,
            problem: EntryProblem::NoFingerprint,
            ..
        }
    ));
}

#[test]
fn a_malformed_date_and_a_repeated_fingerprint_are_refused_and_name_the_entry() {
    let bad_date = rejected(&file(&json!([
        {"fingerprint": "A:1", "reason": "r", "expires": "2027-02-30"}
    ])));
    assert!(matches!(
        &bad_date,
        BaselineError::Entry {
            problem: EntryProblem::BadDate(_),
            ..
        }
    ));
    assert!(
        bad_date.to_string().contains("\"2027-02-30\""),
        "{bad_date}"
    );

    let twice = rejected(&file(&json!([
        {"fingerprint": "A:1", "reason": "r"},
        {"fingerprint": "B:1", "reason": "r"},
        {"fingerprint": "A:1", "reason": "again"},
    ])));
    assert!(matches!(
        &twice,
        BaselineError::Entry {
            index: 3,
            problem: EntryProblem::Duplicate { first: 1 },
            ..
        }
    ));
    assert!(
        twice.to_string().contains("same finding as entry 1"),
        "{twice}"
    );
}

#[test]
fn an_unknown_member_a_wrong_version_or_scheme_and_non_json_are_refused() {
    // A misspelt `expires` that was ignored would accept a finding for ever.
    let typo = file(&json!([{"fingerprint": "A:1", "reason": "r", "expire": "2027-01-01"}]));
    assert!(matches!(rejected(&typo), BaselineError::Malformed(_)));
    assert!(
        rejected(&typo).to_string().contains("expire"),
        "{}",
        rejected(&typo)
    );
    let top = json!({"version": 1, "fingerprint_key": KEY_V2, "findings": [], "note": "x"});
    assert!(matches!(
        rejected(&top.to_string()),
        BaselineError::Malformed(_)
    ));

    let version = json!({"version": 2, "fingerprint_key": KEY_V2, "findings": []}).to_string();
    assert_eq!(rejected(&version), BaselineError::Version(2));
    assert!(rejected(&version).to_string().contains("reads 1"));

    let scheme =
        json!({"version": 1, "fingerprint_key": "mcpConformanceFinding/v3", "findings": []});
    let error = rejected(&scheme.to_string());
    assert_eq!(
        error,
        BaselineError::Scheme("mcpConformanceFinding/v3".to_owned())
    );
    assert!(error.to_string().contains("--write-baseline"), "{error}");

    for text in ["", "not json", "[]", r#"{"version": 1}"#, "null"] {
        assert!(
            matches!(rejected(text), BaselineError::Malformed(_)),
            "{text:?}"
        );
    }
    assert!(
        rejected("{")
            .to_string()
            .starts_with("not a baseline file: ")
    );
}

const KEY_V2: &str = "mcpConformanceFinding/v2";

#[test]
fn an_entry_lapses_the_day_after_its_last_and_not_before() {
    let entry = Entry::new("A:1", "r").expiring(day("2026-10-25"));
    assert!(!entry.lapsed(day("2026-10-24")));
    assert!(
        !entry.lapsed(day("2026-10-25")),
        "the last day still accepts"
    );
    assert!(entry.lapsed(day("2026-10-26")));
    assert!(
        !Entry::new("A:1", "r").lapsed(day("9999-12-31")),
        "no date, no lapse"
    );
}

#[test]
fn the_generated_reason_is_what_marks_an_entry_unreviewed() {
    assert!(Entry::new("A:1", GENERATED_REASON).unreviewed());
    assert!(!Entry::new("A:1", "the SDK's generator").unreviewed());
    assert!(!Entry::new("A:1", format!("{GENERATED_REASON} (checked)")).unreviewed());
    assert!(!GENERATED_REASON.trim().is_empty() && !GENERATED_REASON.contains("TODO"));
    assert!(Acceptance::new(GENERATED_REASON).unreviewed());
    assert!(!Acceptance::new("because").unreviewed());
}

#[test]
fn long_fingerprints_are_cut_in_an_error_and_short_ones_are_not() {
    let long = "x".repeat(200);
    let error = rejected(&file(&json!([
        {"fingerprint": long, "reason": ""}
    ])));
    let text = error.to_string();
    assert!(text.contains('…') && text.len() < 200, "{text}");
    let short = rejected(&file(&json!([{"fingerprint": "A:1", "reason": ""}])));
    assert!(short.to_string().contains("\"A:1\"") && !short.to_string().contains('…'));
}
