// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]
use super::*;

fn row(id: &str, level: &str, outcome: Outcome) -> RequirementReport {
    RequirementReport {
        id: id.to_owned(),
        level: level.to_owned(),
        outcome,
        findings: vec![],
        exclusion: None,
        missing_checks: vec![],
        capability: None,
        source: None,
    }
}

/// One row of every outcome the renderer can produce, so the totals line
/// and the per-row text are pinned against the full set rather than a
/// convenient subset.
fn sample() -> Report {
    let mut failed = row("LIFE-001", "MUST", Outcome::Fail);
    failed.findings = vec![Finding {
        check: "lifecycle.first-interaction-initialize".to_owned(),
        seq: Some(3),
        detail: "first message is \"tools/list\", expected \"initialize\"".to_owned(),
    }];
    failed.source = Some(ClauseSource {
        section: "basic/lifecycle#initialization".to_owned(),
        quote: "The initialization phase MUST be the first interaction".to_owned(),
        url: "https://example.test/basic/lifecycle#initialization".to_owned(),
    });
    let mut excluded = row("TRAN-001", "MUST NOT", Outcome::Excluded);
    excluded.exclusion = Some("enforced at capture time".to_owned());
    let mut not_applicable = row("TOOL-001", "MUST", Outcome::NotApplicable);
    not_applicable.capability = Some("server.tools".to_owned());
    Report {
        revision_mismatch: None,
        revision_source: None,
        revision: "2025-11-25".to_owned(),
        totals: Totals {
            pass: 1,
            fail: 1,
            warn: 0,
            excluded: 1,
            unsupported: 0,
            not_applicable: 1,
            not_observed: 1,
        },
        requirements: vec![
            row("BASE-001", "MUST", Outcome::Pass),
            failed,
            excluded,
            not_applicable,
            row("PAGE-002", "MUST", Outcome::NotObserved),
        ],
    }
}

#[test]
fn verdict_priority_is_unsupported_fail_warn_pass() {
    let mut report = sample();
    assert_eq!(report.verdict(), Verdict::Fail);
    report.totals.unsupported = 1;
    assert_eq!(report.verdict(), Verdict::Unsupported);
    report.totals.unsupported = 0;
    report.totals.fail = 0;
    report.totals.warn = 2;
    assert_eq!(report.verdict(), Verdict::PassWithWarnings);
    report.totals.warn = 0;
    assert_eq!(report.verdict(), Verdict::Pass);
}

#[test]
fn human_rendering_shows_findings_and_totals() {
    let text = sample().render_human();
    assert!(text.contains("FAIL  LIFE-001 (MUST)"), "{text}");
    assert!(text.contains("seq 3:"), "{text}");
    // The clause follows the findings it explains, quoted, then its link — and
    // only on the row that has one.
    assert!(
        text.contains(
            "expected \"initialize\"\n        spec: \"The initialization phase MUST be \
             the first interaction\"\n        see:  \
             https://example.test/basic/lifecycle#initialization\n"
        ),
        "{text}"
    );
    assert_eq!(text.matches("spec: ").count(), 1, "{text}");
    assert!(
        text.contains("excluded: enforced at capture time"),
        "{text}"
    );
    assert!(text.contains("N/A   TOOL-001 (MUST)"), "{text}");
    assert!(
        text.contains("not applicable: capability server.tools was not declared in this session"),
        "{text}"
    );
    // A not-observed row says so in words, like every other non-judged
    // outcome: "NOBS" alone tells an operator nothing about *why*. Pinned
    // as the two lines *together*, and counted: asserting only that the
    // sentence appears somewhere passes just as well when it is attached
    // to every row except the one it describes.
    assert!(
        text.contains(
            "  NOBS  PAGE-002 (MUST)\n        not observed: the session carried none of \
             the traffic this clause binds to\n"
        ),
        "{text}"
    );
    assert_eq!(
        text.matches("not observed:").count(),
        1,
        "exactly the not-observed row carries the reason: {text}"
    );
    // The whole line, anchored at both ends: a `contains` of a prefix would
    // pass while a new outcome went unnamed and the counts stopped summing
    // to the registry's size.
    assert!(
        text.contains(
            "\ntotals: 1 pass, 1 fail, 0 warn, 1 excluded, 0 unsupported, \
             1 not applicable, 1 not observed\n"
        ),
        "{text}"
    );
    assert!(text.contains("verdict: fail"), "{text}");
}

#[test]
fn json_omits_empty_collections() {
    let report = sample();
    let json = serde_json::to_string(&report).unwrap();
    assert!(json.contains("\"revision\":\"2025-11-25\""), "{json}");
    // The verdict the totals imply, between the revision and the totals;
    // absent revision fields are omitted, not null.
    assert!(
        json.starts_with("{\"revision\":\"2025-11-25\",\"verdict\":\"fail\",\"totals\":"),
        "{json}"
    );
    // Passing rows carry no findings/exclusion/missing_checks keys.
    assert!(!json.contains("\"missing_checks\""), "{json}");
}

/// The counts a rendered summary line actually carries, read back out of
/// the text a reader sees rather than off the struct the renderer was
/// handed — the two disagreeing is the whole failure this guards.
fn counts_in(line: &str) -> Vec<u32> {
    // The first number in each comma-separated part is its count; what
    // surrounds it (`totals: ` here, a revision there) carries none.
    line.split(", ")
        .filter_map(|part| part.split_whitespace().find_map(|word| word.parse().ok()))
        .collect()
}

#[test]
fn a_summary_line_accounts_for_every_requirement() {
    let report = sample();
    let text = report.render_human();
    let line = text
        .lines()
        .find(|line| line.starts_with("totals: "))
        .unwrap();
    let counts = counts_in(line);
    assert_eq!(
        counts.len(),
        Totals::default().labelled().len(),
        "every outcome is named: {line}"
    );
    // The invariant the line's own comment claims, asserted rather than
    // left to a reader's arithmetic: what the renderer prints must add up
    // to the rows it printed. The multi-revision line made exactly this
    // claim in prose and silently broke it.
    assert_eq!(
        counts.iter().sum::<u32>() as usize,
        report.requirements.len(),
        "{line}"
    );
}

#[test]
fn every_outcome_has_a_label_and_they_are_distinct() {
    let labels: Vec<&str> = Totals::default()
        .labelled()
        .iter()
        .map(|&(label, _)| label)
        .collect();
    let mut sorted = labels.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), labels.len(), "duplicate label in {labels:?}");
    // Each count sits with its own label: a swapped pair would keep the sum
    // and the label set intact, so the mapping is pinned too.
    let totals = Totals {
        pass: 1,
        fail: 2,
        warn: 3,
        excluded: 4,
        unsupported: 5,
        not_applicable: 6,
        not_observed: 7,
    };
    assert_eq!(
        totals.to_string(),
        "1 pass, 2 fail, 3 warn, 4 excluded, 5 unsupported, 6 not applicable, 7 not observed"
    );
}

#[test]
fn totals_predicates_pin_their_thresholds() {
    let mut report = sample();
    report.totals = Totals::default();
    assert!(!report.has_errors());
    assert!(!report.has_warnings());
    assert!(!report.has_unsupported());
    report.totals.fail = 1;
    assert!(report.has_errors());
    report.totals.warn = 1;
    assert!(report.has_warnings());
    report.totals.unsupported = 1;
    assert!(report.has_unsupported());
}

#[test]
fn judged_nothing_is_only_a_run_with_clauses_it_never_reached() {
    let totals = |pass, fail, warn, unsupported, not_observed| Totals {
        pass,
        fail,
        warn,
        excluded: 5,
        unsupported,
        not_applicable: 3,
        not_observed,
    };
    assert!(totals(0, 0, 0, 0, 1).judged_nothing());
    // Each escape on its own, so no arm can be dropped or inverted unnoticed.
    assert!(!totals(1, 0, 0, 0, 1).judged_nothing(), "a pass");
    assert!(!totals(0, 1, 0, 0, 1).judged_nothing(), "a failure");
    assert!(!totals(0, 0, 1, 0, 1).judged_nothing(), "a warning");
    assert!(
        !totals(0, 0, 0, 1, 1).judged_nothing(),
        "the registry's problem"
    );
    assert!(!totals(0, 0, 0, 0, 0).judged_nothing(), "nothing to reach");
}
