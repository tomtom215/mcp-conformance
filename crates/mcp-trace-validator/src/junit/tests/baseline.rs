// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A baseline in `JUnit`: accepted findings are skipped cases, never passes and
//! never failures, and the entries that did not do their job are a suite of
//! their own that fails the document as they fail the exit status.

use crate::baseline::{Entry, Notes};
use crate::fixtures::{baselined_sdk_session, sdk_session};
use crate::junit::{Options, render_run_with_baseline, render_with};

fn options() -> Options {
    Options::for_trace("t.jsonl")
}

/// The `name="value"` attributes of the first `<testsuites>` or `<testsuite>`
/// element named `tag`.
fn attributes(xml: &str, tag: &str) -> Vec<(String, u32)> {
    let line = xml
        .lines()
        .find(|line| line.trim_start().starts_with(&format!("<{tag} ")))
        .unwrap();
    ["tests", "failures", "skipped"]
        .iter()
        .map(|name| {
            let at = line.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
            (
                (*name).to_owned(),
                line[at..].split('"').next().unwrap().parse().unwrap(),
            )
        })
        .collect()
}

fn count(xml: &str, tag: &str, name: &str) -> u32 {
    attributes(xml, tag)
        .into_iter()
        .find(|(key, _)| key == name)
        .unwrap()
        .1
}

/// The `BASE-086` case, whole.
fn case(xml: &str) -> &str {
    let start = xml.find("<testcase classname=\"t.jsonl.mcp.2025-11-25\" file=\"t.jsonl\" name=\"BASE-086 (SHOULD)\"").unwrap();
    let end = xml[start..]
        .find("</testcase>")
        .map_or(xml.len(), |at| start + at);
    &xml[start..end]
}

#[test]
fn an_accepted_row_is_skipped_with_its_findings_and_reasons_and_is_not_a_pass() {
    let known = baselined_sdk_session("the SDK's generator emits draft-07");
    let plain = sdk_session();
    let accepted_xml = render_with(&known.reports, &options());
    let plain_xml = render_with(&plain.reports, &options());

    let case = case(&accepted_xml);
    assert!(
        case.contains("<skipped message=\"accepted by baseline: 2 of 2 findings are known"),
        "{case}"
    );
    assert!(case.contains("<system-out>"), "{case}");
    assert!(
        case.contains(
            "[base.schemas-use-2020-12] at seq 4 (server): the server sent tool &quot;a&quot;"
        ),
        "{case}"
    );
    assert!(
        case.contains("[accepted by baseline: the SDK&apos;s generator")
            || case.contains("[accepted by baseline: the SDK's generator emits draft-07]"),
        "{case}"
    );
    assert!(!case.contains("<failure"), "{case}");
    assert!(
        !case.contains("/>\n") || case.contains("<skipped"),
        "never a self-closing pass: {case}"
    );

    // Un-baselined, the same row is a passing warning; baselined, it is skipped.
    assert!(!case_of_plain(&plain_xml).contains("<skipped"));
    assert_eq!(
        count(&accepted_xml, "testsuites", "tests"),
        count(&plain_xml, "testsuites", "tests")
    );
    assert_eq!(
        count(&accepted_xml, "testsuites", "skipped"),
        count(&plain_xml, "testsuites", "skipped") + 1
    );
    assert_eq!(count(&accepted_xml, "testsuites", "failures"), 0);
}

fn case_of_plain(xml: &str) -> &str {
    case(xml)
}

#[test]
fn strict_does_not_turn_an_accepted_warning_into_a_failure_but_still_fails_a_new_one() {
    let known = baselined_sdk_session("known");
    let strict = options().strict(true);
    let xml = render_with(&known.reports, &strict);
    assert_eq!(count(&xml, "testsuites", "failures"), 0, "{xml}");
    assert!(case(&xml).contains("<skipped"));
    // Without the baseline, strict makes the same row a failure.
    let plain = render_with(&sdk_session().reports, &strict);
    assert_eq!(count(&plain, "testsuites", "failures"), 1);
}

#[test]
fn a_row_with_a_new_finding_fails_for_it_alone_and_lists_the_accepted_ones() {
    let mut known = baselined_sdk_session("known");
    let row = known.reports[0]
        .requirements
        .iter_mut()
        .find(|row| row.id == "BASE-086")
        .unwrap();
    let mut new = row.findings[0].clone();
    new.baselined = None;
    "the server sent tool \"d\"'s inputSchema (new)".clone_into(&mut new.detail);
    row.findings.push(new);
    let xml = render_with(&known.reports, &options().strict(true));
    let case = case(&xml);
    assert_eq!(case.matches("<failure ").count(), 1, "{case}");
    assert!(case.contains("tool &quot;d&quot;"), "{case}");
    assert!(!case.contains("<skipped"), "{case}");
    // The two accepted findings are in the case's output, not failures.
    assert_eq!(
        case.matches("[accepted by baseline: known").count(),
        2,
        "{case}"
    );
    assert_eq!(count(&xml, "testsuites", "failures"), 1);
}

#[test]
fn an_accepted_wire_schema_failure_is_skipped_and_a_counted_one_fails() {
    use crate::baseline::{Baseline, TraceReports, record};
    use crate::fixtures::session_answering;
    use serde_json::json;
    let mut judgment =
        crate::judge::judge(&session_answering(&json!({"tools": "not a list"}))).unwrap();
    let failing = render_with(&judgment.reports, &options());
    assert!(failing.contains("name=\"wire schema\">"), "{failing}");
    assert!(count(&failing, "testsuites", "failures") >= 1);
    let recorded = record(&[(Some("t.jsonl"), &judgment.reports)], None, None);
    let baseline = Baseline::new(
        recorded
            .baseline
            .entries()
            .iter()
            .filter(|entry| entry.fingerprint.starts_with("wire-schema:"))
            .cloned()
            .collect(),
    );
    let mut traces = [TraceReports {
        name: Some("t.jsonl"),
        reports: &mut judgment.reports,
    }];
    baseline.apply(&mut traces, None).unwrap();
    let xml = render_with(&judgment.reports, &options());
    let start = xml.find("name=\"wire schema\">").unwrap();
    let schema_case = &xml[start
        ..xml[start..]
            .find("</testcase>")
            .map(|at| start + at)
            .unwrap()];
    assert!(
        schema_case.contains("<skipped message=\"accepted by baseline"),
        "{schema_case}"
    );
    assert!(!schema_case.contains("<failure"), "{schema_case}");
    assert!(count(&xml, "testsuites", "failures") < count(&failing, "testsuites", "failures"));
    // It moves from the failures to the skipped cases, one for one, in the
    // suite's attributes and the document's.
    for tag in ["testsuites", "testsuite"] {
        assert_eq!(
            count(&xml, tag, "skipped"),
            count(&failing, tag, "skipped") + 1,
            "{tag}"
        );
        assert_eq!(
            count(&xml, tag, "failures") + 1,
            count(&failing, tag, "failures"),
            "{tag}"
        );
        assert_eq!(count(&xml, tag, "tests"), count(&failing, tag, "tests"));
    }
}

#[test]
fn the_notes_are_a_suite_a_stale_entry_fails_unless_allowed_and_a_lapsed_one_is_skipped() {
    let known = baselined_sdk_session("known");
    let traces = [(options(), &known.reports[..])];
    let mut notes = Notes::default();
    notes
        .stale
        .push(Entry::new("BASE-086:check:gone:1", "was fixed <soon>"));
    notes.lapsed.push(
        Entry::new("BASE-086:check:old:1", "until July").expiring("2020-07-01".parse().unwrap()),
    );
    let xml = render_run_with_baseline(&traces, &[], &notes);
    let suite_start = xml.find("<testsuite name=\"baseline\"").unwrap();
    let suite = &xml[suite_start..];
    assert!(
        suite.starts_with("<testsuite name=\"baseline\" tests=\"2\" failures=\"1\" skipped=\"1\">"),
        "{suite}"
    );
    assert!(suite.contains("name=\"stale baseline entry: BASE-086:check:gone:1\"><failure message=\"no finding of this run matches this entry"), "{suite}");
    assert!(suite.contains("was fixed &lt;soon&gt;"), "escaped: {suite}");
    assert!(suite.contains("name=\"lapsed baseline entry: BASE-086:check:old:1\"><skipped message=\"this entry expired on 2020-07-01"), "{suite}");
    // The root counts the suite: the traces' own plus its two tests and one failure.
    let without = render_run_with_baseline(&traces, &[], &Notes::default());
    assert_eq!(
        count(&xml, "testsuites", "tests"),
        count(&without, "testsuites", "tests") + 2
    );
    assert_eq!(
        count(&xml, "testsuites", "failures"),
        count(&without, "testsuites", "failures") + 1
    );

    let allowed = render_run_with_baseline(&traces, &[], &notes.clone().allowing_stale(true));
    assert!(
        allowed.contains("<testsuite name=\"baseline\" tests=\"2\" failures=\"0\" skipped=\"2\">"),
        "{allowed}"
    );
    assert!(
        allowed.contains("stale baseline entry: BASE-086:check:gone:1\"><skipped"),
        "{allowed}"
    );
    assert_eq!(
        count(&allowed, "testsuites", "failures"),
        count(&without, "testsuites", "failures")
    );
}

#[test]
fn no_notes_means_no_baseline_suite_and_the_document_render_run_gives() {
    let plain = sdk_session();
    let traces = [(options(), &plain.reports[..])];
    assert_eq!(
        render_run_with_baseline(&traces, &[], &Notes::default()),
        crate::junit::render_run(&traces, &[])
    );
    assert!(!crate::junit::render_run(&traces, &[]).contains("baseline"));
}
