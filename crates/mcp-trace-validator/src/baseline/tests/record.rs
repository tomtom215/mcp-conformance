// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Recording a baseline from a run, and refreshing one.

use super::*;

fn recorded(judgment: &Judgment, view: Option<Party>, previous: Option<&Baseline>) -> Recorded {
    record(&[(Some("t.jsonl"), &judgment.reports)], view, previous)
}

#[test]
fn a_recorded_baseline_accepts_exactly_what_was_recorded_and_says_it_is_unreviewed() {
    let mut judgment = sdk_session();
    let written = recorded(&judgment, None, None);
    assert_eq!((written.carried, written.added, written.dropped), (0, 2, 0));
    assert_eq!(written.baseline.entries().len(), 2);
    assert!(
        written
            .baseline
            .entries()
            .iter()
            .all(|e| e.reason == GENERATED_REASON && e.expires.is_none())
    );
    // Applying it to the run it was written from leaves nothing new and nothing stale.
    let applied = apply_to(&written.baseline, &mut judgment, "t.jsonl", None).unwrap();
    assert!(applied.notes.is_empty());
    assert_eq!(applied.tally.unaccepted, 0);
    assert_eq!(applied.tally.unreviewed, 2);
    assert_eq!(judgment.reports[0].effective_verdict(), Verdict::Pass);
    // The file it would write reads back as itself.
    let text = written.baseline.to_json();
    assert_eq!(Baseline::parse(&text).unwrap().to_json(), text);
}

#[test]
fn a_clean_run_records_an_empty_baseline() {
    let judgment = judged(&[("c", DRAFT_2020)]);
    let written = recorded(&judgment, None, None);
    assert_eq!(written.baseline.entries().len(), 0);
    assert_eq!((written.carried, written.added, written.dropped), (0, 0, 0));
    assert!(written.baseline.to_json().contains("\"findings\": []"));
}

#[test]
fn a_finding_present_in_several_traces_is_one_entry() {
    let one = sdk_session();
    let two = sdk_session();
    let written = record(
        &[
            (Some("a.jsonl"), &one.reports),
            (Some("b.jsonl"), &two.reports),
        ],
        None,
        None,
    );
    assert_eq!(written.baseline.entries().len(), 2);
    assert_eq!(written.added, 2);
}

#[test]
fn a_refresh_keeps_reasons_and_expiry_adds_new_findings_and_drops_stale_entries() {
    let judgment = sdk_session();
    let by_hand = recorded(&judgment, None, None);
    let [a, b] = [
        &by_hand.baseline.entries()[0],
        &by_hand.baseline.entries()[1],
    ];
    let stale = Entry::new("BASE-086:gone:gone:1", "was fixed");
    let previous = Baseline::new(vec![
        Entry::new(a.fingerprint.clone(), "reviewed: the SDK's generator")
            .expiring(day("2999-12-31")),
        stale,
    ]);
    // Tool b's finding has no entry yet; the stale entry matches nothing.
    let written = recorded(&judgment, None, Some(&previous));
    assert_eq!((written.carried, written.added, written.dropped), (1, 1, 1));
    let entries = written.baseline.entries();
    let kept = entries
        .iter()
        .find(|e| e.fingerprint == a.fingerprint)
        .unwrap();
    assert_eq!(kept.reason, "reviewed: the SDK's generator");
    assert_eq!(
        kept.expires,
        Some(day("2999-12-31")),
        "a refresh never renews an expiry"
    );
    let added = entries
        .iter()
        .find(|e| e.fingerprint == b.fingerprint)
        .unwrap();
    assert_eq!(added.reason, GENERATED_REASON);
    assert!(
        entries
            .iter()
            .all(|e| e.fingerprint != "BASE-086:gone:gone:1")
    );
}

#[test]
fn a_refresh_carries_a_lapsed_entry_as_it_is() {
    let judgment = sdk_session();
    let fingerprint = recorded(&judgment, None, None).baseline.entries()[0]
        .fingerprint
        .clone();
    let lapsed = Entry::new(fingerprint, "until July").expiring(day("2020-07-01"));
    let previous = Baseline::new(vec![lapsed.clone()]);
    let written = recorded(&judgment, None, Some(&previous));
    assert!(
        written.baseline.entries().contains(&lapsed),
        "still lapsed after the refresh"
    );
}

#[test]
fn a_party_view_records_only_the_findings_that_count_under_it() {
    // Both findings are the server's.
    let judgment = sdk_session();
    let client = recorded(&judgment, Some(Party::Client), None);
    assert_eq!(
        client.baseline.entries().len(),
        0,
        "the client's view sets them aside"
    );
    let server = recorded(&judgment, Some(Party::Server), None);
    assert_eq!(server.baseline.entries().len(), 2);
}

#[test]
fn a_refresh_under_a_view_does_not_lose_an_entry_for_a_finding_the_view_sets_aside() {
    let judgment = sdk_session();
    let all = recorded(&judgment, None, None);
    let kept = Entry::new(all.baseline.entries()[0].fingerprint.clone(), "reviewed");
    let previous = Baseline::new(vec![kept.clone()]);
    let written = recorded(&judgment, Some(Party::Client), Some(&previous));
    assert_eq!(
        written.baseline.entries(),
        [kept],
        "carried, and nothing new is added"
    );
    assert_eq!((written.carried, written.added, written.dropped), (1, 0, 0));
}

#[test]
fn an_extensions_findings_are_never_recorded() {
    let (reports, _) = crate::fixtures::undeclared_task();
    assert!(reports[0].extensions.iter().any(|e| e.totals.fail > 0));
    let written = record(&[(Some("t.jsonl"), &reports)], None, None);
    assert!(
        written
            .baseline
            .entries()
            .iter()
            .all(|e| !e.fingerprint.starts_with("TASK-")),
        "{:#?}",
        written.baseline.entries()
    );
}

#[test]
fn what_strict_would_do_to_an_exit_status_does_not_decide_what_is_recorded() {
    // `record` has no notion of --strict: warnings are recorded as findings,
    // which is what lets a later `--strict` run gate on the same file.
    let judgment = sdk_session();
    assert_eq!(judgment.reports[0].totals.fail, 0);
    assert_eq!(recorded(&judgment, None, None).baseline.entries().len(), 2);
}
