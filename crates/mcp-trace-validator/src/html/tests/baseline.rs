// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A baseline on the page: an accepted finding keeps its place, marked and
//! with its reason; the verdict says a baseline was applied; the entries that
//! did not do their job have a note; and the stylesheet — hence the policy that
//! names its digest, and every page without a baseline — is untouched.

use crate::baseline::{Entry, Notes};
use crate::fixtures::{baselined_sdk_session, sdk_session};
use crate::html::{Trace, render, render_run, render_run_with_baseline};
use crate::judge::Judgment;

fn page_of(judgment: &Judgment, notes: &Notes) -> String {
    render_run_with_baseline(
        &[(
            Trace {
                name: Some("t.jsonl"),
                events: &judgment.events,
            },
            &judgment.reports[..],
        )],
        &[],
        notes,
    )
}

#[test]
fn an_accepted_finding_is_marked_with_its_reason_beside_the_finding_and_at_its_event() {
    let known = baselined_sdk_session("the SDK's generator emits draft-07");
    let page = page_of(&known, &Notes::default());
    // On the clause's list…
    assert!(
        page.contains(
            "<span class=\"pill party none\" title=\"accepted by baseline: the SDK&#39;s generator emits draft-07\">accepted</span>"
        ) || page.contains("title=\"accepted by baseline: the SDK&#x27;s generator emits draft-07\">accepted</span>")
            || page.contains("title=\"accepted by baseline: the SDK's generator emits draft-07\">accepted</span>"),
        "{}",
        page.lines().filter(|l| l.contains("accepted")).collect::<Vec<_>>().join("\n")
    );
    assert!(
        page.contains("accepted by baseline: the SDK"),
        "the reason is printed beside the finding"
    );
    // …and in the timeline at the event that holds it: one mark per finding, and
    // two list items.
    assert_eq!(
        page.matches(">accepted</span>").count(),
        4,
        "list and timeline, for two findings"
    );
    // The verdict says a baseline was applied and what it accepted; the totals
    // still count the warning.
    assert!(
        page.contains("baseline applied: 2 findings accepted, 0 not accepted"),
        "{}",
        page.lines()
            .filter(|l| l.contains("verdict-why"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(page.contains("1 clause warned"));
    assert!(page.contains("<span class=\"badge v-pass\">Pass</span>"));
}

#[test]
fn the_stylesheet_and_policy_are_the_pages_without_a_baseline() {
    let known = baselined_sdk_session("known");
    let plain = sdk_session();
    let policy = |page: &str| {
        page.lines()
            .find(|line| line.contains("Content-Security-Policy"))
            .unwrap()
            .to_owned()
    };
    assert_eq!(
        policy(&page_of(&known, &Notes::default())),
        policy(&page_of(&plain, &Notes::default()))
    );
    assert!(!page_of(&known, &Notes::default()).contains("<script"));
}

#[test]
fn a_page_without_a_baseline_says_nothing_of_one_and_is_render_runs() {
    let plain = sdk_session();
    let page = page_of(&plain, &Notes::default());
    // (The stylesheet has `vertical-align: baseline`, so the bare word is not a test.)
    for phrase in [
        "baseline applied",
        "accepted by baseline",
        "did not do their job",
    ] {
        assert!(!page.contains(phrase), "{phrase:?} without a baseline");
    }
    assert!(!page.contains(">accepted<"));
    let trace = Trace {
        name: Some("t.jsonl"),
        events: &plain.events,
    };
    assert_eq!(page, render(&plain.reports, trace));
    assert_eq!(page, render_run(&[(trace, &plain.reports[..])], &[]));
}

#[test]
fn a_new_finding_beside_accepted_ones_has_no_mark() {
    let mut judgment = baselined_sdk_session("known");
    let row = judgment.reports[0]
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-086")
        .unwrap();
    let mut new = row.findings[0].clone();
    new.baselined = None;
    "the server sent tool \"d\"'s inputSchema (new)".clone_into(&mut new.detail);
    row.findings.push(new);
    let page = page_of(&judgment, &Notes::default());
    assert!(page.contains("baseline applied: 2 findings accepted, 1 not accepted"));
    assert!(page.contains("<span class=\"badge v-warn\">Pass with warnings</span>"));
    assert_eq!(
        page.matches(">accepted</span>").count(),
        4,
        "the new finding adds none"
    );
}

#[test]
fn the_notes_are_listed_escaped_and_say_whether_the_run_fails_for_them() {
    let known = baselined_sdk_session("known");
    let mut notes = Notes::default();
    notes
        .stale
        .push(Entry::new("BASE-086:check:<gone>:1", "was <b>fixed</b>"));
    notes.lapsed.push(
        Entry::new("BASE-086:check:old:1", "until July").expiring("2020-07-01".parse().unwrap()),
    );
    let page = page_of(&known, &notes);
    assert!(
        page.contains("Baseline entries that did not do their job."),
        "{page}"
    );
    assert!(page.contains("1 matches no finding of this run"), "{page}");
    assert!(
        page.contains("the run exits 1 until they are removed"),
        "{page}"
    );
    assert!(
        page.contains("<code>BASE-086:check:&lt;gone&gt;:1</code> — was &lt;b&gt;fixed&lt;/b&gt;"),
        "escaped"
    );
    assert!(!page.contains("<b>fixed</b>"));
    assert!(page.contains("1 entry has expired"), "{page}");
    assert!(page.contains("(expired 2020-07-01)"), "{page}");
    let allowed = page_of(&known, &notes.clone().allowing_stale(true));
    assert!(allowed.contains("stale entries are allowed for this run"));
    assert!(!allowed.contains("exits 1 until"));
    // The note sits under the verdict, before the clauses.
    assert!(page.find("did not do their job").unwrap() < page.find("Clauses to act on").unwrap());
}

#[test]
fn a_run_page_carries_the_note_once_and_not_inside_each_trace() {
    let known = baselined_sdk_session("known");
    let mut notes = Notes::default();
    notes
        .stale
        .push(Entry::new("BASE-086:check:gone:1", "was fixed"));
    let trace = |name| Trace {
        name: Some(name),
        events: &known.events,
    };
    let page = render_run_with_baseline(
        &[
            (trace("a.jsonl"), &known.reports[..]),
            (trace("b.jsonl"), &known.reports[..]),
        ],
        &[],
        &notes,
    );
    assert_eq!(page.matches("did not do their job").count(), 1);
}
