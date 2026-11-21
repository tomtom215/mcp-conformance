// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A report with a baseline applied, in the terminal: the accepted findings
//! stay in it, tagged; the verdict says it stopped counting them; and nothing
//! reads as a pass for a finding that is still there.

use super::super::*;
use crate::baseline::GENERATED_REASON;
use crate::fixtures::{DRAFT_07, baselined_sdk_session, judged};

fn text(reports: &[Report]) -> String {
    reports[0].render_with(&HumanOptions::default().all(false))
}

#[test]
fn an_accepted_finding_is_listed_tagged_with_its_reason_and_the_verdict_names_the_baseline() {
    let judgment = baselined_sdk_session("the SDK's generator emits draft-07");
    let text = text(&judgment.reports);
    // The finding is still on its row, which keeps its WARN marker…
    assert!(text.contains("  WARN  BASE-086 (SHOULD)"), "{text}");
    assert!(
        text.contains(r#"[server] [accepted] seq 4: the server sent tool "a"'s inputSchema"#),
        "{text}"
    );
    assert!(text.contains(r#"tool "b"'s"#), "{text}");
    // …its reason is said once for the two it covers…
    assert!(
        text.contains("accepted by baseline (2): the SDK's generator emits draft-07\n"),
        "{text}"
    );
    // …and the verdict is of what the baseline left.
    assert!(
        text.contains("baseline: 2 findings accepted, 0 not accepted\n"),
        "{text}"
    );
    assert!(
        text.ends_with("verdict (baseline applied): pass\n"),
        "{text}"
    );
    // The totals are the trace's facts.
    assert!(text.contains("1 warn"), "{text}");
    assert!(!text.contains("pass-with-warnings"), "{text}");
}

#[test]
fn a_report_without_a_baseline_says_nothing_of_one() {
    let judgment = judged(&[("a", DRAFT_07)]);
    let text = text(&judgment.reports);
    assert!(
        !text.contains("accepted") && !text.contains("baseline"),
        "{text}"
    );
    assert!(text.ends_with("verdict: pass-with-warnings\n"), "{text}");
}

#[test]
fn a_new_finding_beside_accepted_ones_is_untagged_and_decides_the_verdict() {
    let mut judgment = baselined_sdk_session("known");
    // A finding no entry accepts, on the same row.
    let row = judgment.reports[0]
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-086")
        .unwrap();
    let mut new = row.findings[0].clone();
    new.baselined = None;
    "the server sent tool \"d\"'s inputSchema (new)".clone_into(&mut new.detail);
    row.findings.push(new);
    let text = text(&judgment.reports);
    assert!(
        text.contains(r#"[server] seq 4: the server sent tool "d"'s inputSchema (new)"#),
        "{text}"
    );
    assert!(
        text.contains("baseline: 2 findings accepted, 1 not accepted\n"),
        "{text}"
    );
    assert!(
        text.ends_with("verdict (baseline applied): pass-with-warnings\n"),
        "{text}"
    );
}

#[test]
fn the_cap_hides_lines_but_not_the_count_of_what_is_accepted() {
    let judgment = baselined_sdk_session("known");
    let options = HumanOptions::default().all(false).max_findings(Some(1));
    let text = judgment.reports[0].render_with(&options);
    assert!(text.contains("… and 1 more"), "{text}");
    assert!(
        text.contains("accepted by baseline (2): known"),
        "the unlisted one is accepted too: {text}"
    );
}

#[test]
fn the_generated_reason_is_shown_for_what_it_is_and_counted_as_unreviewed() {
    let judgment = baselined_sdk_session(GENERATED_REASON);
    let text = text(&judgment.reports);
    assert!(
        text.contains(&format!("accepted by baseline (2): {GENERATED_REASON}\n")),
        "{text}"
    );
    assert!(
        text.contains(
            "baseline: 2 findings accepted, 0 not accepted; 2 of the accepted carry the reason \
             --write-baseline wrote, which nobody has reviewed\n"
        ),
        "{text}"
    );
}

#[test]
fn an_expiry_is_shown_with_the_reason_and_a_control_character_in_it_is_not_sent_raw() {
    let mut judgment = baselined_sdk_session("known \u{1b}[31mred");
    for row in &mut judgment.reports[0].requirements {
        for finding in &mut row.findings {
            if let Some(acceptance) = &mut finding.baselined {
                acceptance.expires = Some("2027-03-31".to_owned());
            }
        }
    }
    let text = text(&judgment.reports);
    assert!(
        text.contains(r"known \u{1b}[31mred (accepted through 2027-03-31)"),
        "{text}"
    );
    assert!(!text.contains('\u{1b}'), "{text:?}");
}

/// Changes the first accepted finding's acceptance with `change`, and
/// returns the report's text.
fn with_the_first_acceptance(change: impl FnOnce(&mut Acceptance)) -> String {
    let mut judgment = baselined_sdk_session("known");
    let first = judgment.reports[0]
        .requirements
        .iter_mut()
        .flat_map(|row| &mut row.findings)
        .find_map(|finding| finding.baselined.as_mut());
    // Were there none, the assertions on the text would say so.
    if let Some(first) = first {
        change(first);
    }
    text(&judgment.reports)
}

#[test]
fn acceptances_are_grouped_only_when_both_the_reason_and_the_expiry_match() {
    // One reason, two expiries: two lines.
    let text = with_the_first_acceptance(|first| first.expires = Some("2027-03-31".to_owned()));
    assert!(
        text.contains(
            "accepted by baseline (1): known (accepted through 2027-03-31)
"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "accepted by baseline (1): known
"
        ),
        "{text}"
    );
    // Two reasons, one expiry (none): two lines.
    let text = with_the_first_acceptance(|first| "other".clone_into(&mut first.reason));
    assert!(
        text.contains(
            "accepted by baseline (1): other
"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "accepted by baseline (1): known
"
        ),
        "{text}"
    );
}

#[test]
fn a_party_view_and_a_baseline_label_the_verdict_with_both() {
    let mut judgment = baselined_sdk_session("known");
    judgment.reports[0].party_view = Some(Party::Server);
    let text = text(&judgment.reports);
    assert!(
        text.ends_with("verdict (server findings only, baseline applied): pass\n"),
        "{text}"
    );
    judgment.reports[0].party_view = Some(Party::Client);
    let text = judgment.reports[0].render_with(&HumanOptions::default().all(false));
    assert!(
        text.ends_with(
            "verdict (client findings only, baseline applied): pass — 2 server findings set aside\n"
        ),
        "{text}"
    );
}

#[test]
fn the_wording_for_the_verdict_label_has_one_form_per_view() {
    assert_eq!(
        HumanOptions::verdict_label("verdict", None, false),
        "verdict"
    );
    assert_eq!(
        HumanOptions::verdict_label("verdict", Some(Party::Client), false),
        "verdict (client findings only)"
    );
    assert_eq!(
        HumanOptions::verdict_label("overall verdict", None, true),
        "overall verdict (baseline applied)"
    );
}
