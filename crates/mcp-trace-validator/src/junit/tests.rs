// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `JUnit` rendering.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::reader::{Limits, parse_trace};
use mcp_conformance_core::requirement::Registry;

fn report_for(trace: &str) -> Report {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let events = parse_trace(trace, &Limits::default()).unwrap();
    crate::engine::validate(&registry, &events)
}

const VIOLATION: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;

#[test]
fn renders_well_formed_suite_with_failure_and_skips() {
    let total = Registry::builtin_2025_11_25().unwrap().requirements().len();
    let xml = render(&report_for(VIOLATION));
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    // One testcase per registry requirement; counts reconcile with the totals.
    assert!(
        xml.contains(&format!(r#"<testsuites tests="{total}""#)),
        "{xml}"
    );
    assert!(xml.contains(r#"name="LIFE-001 (MUST)""#), "{xml}");
    assert!(xml.contains("<failure message="), "{xml}");
    assert!(xml.contains("<skipped message="), "{xml}");
    // The LIFE-004 warning must NOT be a failure; its findings live in system-out.
    assert!(xml.contains("<system-out>"), "{xml}");
    // Each carries its clause: the failure body under its location, the
    // warning's after its findings, closing the element.
    assert!(
            xml.contains(
                "at seq 0\nspec: &quot;The initialization phase MUST be the first interaction \
                 between client and server.&quot;\nsee: \
                 https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle#initialization</failure>"
            ),
            "{xml}"
        );
    assert!(
            xml.contains(
                "before the server has responded to the `initialize` request.&quot;\nsee: \
                 https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle#initialization</system-out>"
            ),
            "{xml}"
        );
    // Balanced tags, exactly once each.
    assert_eq!(xml.matches("<testsuites").count(), 1);
    assert_eq!(xml.matches("</testsuites>").count(), 1);
    assert_eq!(xml.matches("<testsuite ").count(), 1);
    assert_eq!(xml.matches("</testsuite>").count(), 1);
    assert_eq!(xml.matches("<testcase").count(), total);
}

#[test]
fn escapes_xml_metacharacters_in_details() {
    // Finding details quote method names: "tools/list" arrives inside XML
    // attributes, and quotes/angles must be escaped, never raw.
    let xml = render(&report_for(VIOLATION));
    assert!(xml.contains("&quot;tools/list&quot;"), "{xml}");
    assert!(
        !xml.contains(r#"message="first message is a "tools"#),
        "{xml}"
    );
    assert_eq!(escape(r#"<a & "b">"#), "&lt;a &amp; &quot;b&quot;&gt;");
}

#[test]
fn escape_substitutes_xml_illegal_control_characters() {
    // C0 controls other than tab/LF/CR cannot appear in XML 1.0 even as
    // numeric references (XML 1.0 §2.2), so escape() substitutes them with
    // U+FFFD; tab/LF/CR pass through. This is defense in depth: today's
    // findings format trace strings with `{:?}`, which already renders a
    // control char as printable `\u{1}` before it reaches escape(), so the
    // hazard is not reachable through a current check — but escape()'s
    // contract is "always emit a well-formed document," independent of how
    // any caller built its string, and a future Display-formatted finding
    // must not be able to void that.
    assert_eq!(escape("a\u{0001}b\u{001F}c"), "a\u{FFFD}b\u{FFFD}c");
    assert_eq!(escape("a\tb\nc\rd"), "a\tb\nc\rd");
    // The boundary: U+001F substitutes, U+0020 (space) passes.
    assert_eq!(escape("\u{001F}\u{0020}"), "\u{FFFD} ");
}

#[test]
fn passing_reports_have_zero_failures_and_self_closing_cases() {
    let xml = render(&report_for(
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
    ));
    assert!(xml.contains(r#"failures="0""#), "{xml}");
    assert!(xml.contains(r#"name="BASE-001 (MUST)"/>"#), "{xml}");
}

fn bare_row(id: &str, outcome: Outcome) -> crate::report::RequirementReport {
    crate::report::RequirementReport {
        id: id.to_owned(),
        level: "MUST".to_owned(),
        outcome,
        findings: vec![],
        exclusion: None,
        missing_checks: vec![],
        capability: None,
        source: None,
    }
}

/// A report carrying every non-judged outcome — excluded, unsupported,
/// not-applicable AND not-observed — because the sum is where they are easy
/// to forget: `not_observed` was rendered as a `<skipped>` row while being
/// left out of the `skipped=` attribute, so the counts disagreed with the
/// body.
fn every_skip_variant() -> Report {
    use crate::report::{Finding, Totals};
    let mut failed = bare_row("AAAA-001", Outcome::Fail);
    failed.findings = vec![Finding {
        check: "area.some-check".to_owned(),
        seq: Some(7),
        detail: "it went wrong".to_owned(),
    }];
    let mut excluded_a = bare_row("AAAA-002", Outcome::Excluded);
    excluded_a.exclusion = Some("not judgeable from traces".to_owned());
    let mut excluded_b = bare_row("AAAA-003", Outcome::Excluded);
    excluded_b.exclusion = Some("also excluded".to_owned());
    let mut unsupported = bare_row("AAAA-004", Outcome::Unsupported);
    unsupported.missing_checks = vec!["future.check".to_owned()];
    let mut not_applicable = bare_row("AAAA-005", Outcome::NotApplicable);
    not_applicable.capability = Some("server.tools".to_owned());
    Report {
        revision_mismatch: None,
        revision_source: None,
        revision: "2025-11-25".to_owned(),
        totals: Totals {
            pass: 0,
            fail: 1,
            warn: 0,
            excluded: 2,
            unsupported: 1,
            not_applicable: 1,
            not_observed: 1,
        },
        requirements: vec![
            failed,
            excluded_a,
            excluded_b,
            unsupported,
            not_applicable,
            bare_row("AAAA-006", Outcome::NotObserved),
        ],
    }
}

#[test]
fn skip_accounting_and_location_text_are_exact() {
    // Pins the skipped sum, the per-variant messages, and the failure-body
    // location text.
    let xml = render(&every_skip_variant());
    // skipped = excluded + unsupported + not_applicable + not_observed,
    // and `tests` counts every row exactly once.
    assert!(
        xml.contains(r#"<testsuites tests="6" failures="1" skipped="5">"#),
        "{xml}"
    );
    assert_eq!(xml.matches("<testcase").count(), 6, "{xml}");
    assert!(
            xml.contains(
                r#"<skipped message="not applicable: capability server.tools was not declared in this session"/>"#
            ),
            "{xml}"
        );
    // Each skip variant carries its own distinct message.
    assert!(
        xml.contains(r#"<skipped message="not judgeable from traces"/>"#),
        "{xml}"
    );
    assert!(
        xml.contains(
            r#"<skipped message="the session carried none of the traffic this clause binds to"/>"#
        ),
        "{xml}"
    );
    assert!(
            xml.contains(r#"<skipped message="registry references checks this build does not implement: future.check"/>"#),
            "{xml}"
        );
    // Failure bodies carry the check-and-seq location, verbatim.
    assert!(
        xml.contains(">[area.some-check] at seq 7</failure>"),
        "{xml}"
    );
    assert_eq!(location(None, "x.y"), "[x.y]");
    assert_eq!(location(Some(3), "x.y"), "[x.y] at seq 3");
}
