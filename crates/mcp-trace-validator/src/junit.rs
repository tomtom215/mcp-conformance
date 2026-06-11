// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `JUnit` XML rendering of validation reports, for CI systems that ingest test
//! result files.
//!
//! Mapping (documented because `JUnit` has no native concept of a warning):
//!
//! | Outcome | `JUnit` representation |
//! |---------|----------------------|
//! | `pass` | passing `<testcase>` |
//! | `fail` | `<failure>` per requirement, findings in the body |
//! | `warn` | passing `<testcase>` with findings in `<system-out>` — SHOULD-level findings do not fail CI unless promoted by `--strict`, and that promotion is an exit-code concern, not a report concern |
//! | `excluded` / `unsupported` / `not-applicable` | `<skipped>` with the reason as its message |
//!
//! The output is deterministic (registry order, no timestamps, no hostnames) for the
//! same reason every other report format is: reports are artifacts.

use core::fmt::Write as _;

use crate::report::{Outcome, Report};

/// Renders the report as a single-suite `JUnit` XML document.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, junit};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let xml = junit::render(&engine::validate(&registry, &[]));
/// assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render(report: &Report) -> String {
    let totals = report.totals;
    let failures = totals.fail;
    let skipped = totals.excluded + totals.unsupported + totals.not_applicable;
    let tests = totals.pass + totals.fail + totals.warn + skipped;

    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    let _ = writeln!(
        out,
        r#"<testsuites tests="{tests}" failures="{failures}" skipped="{skipped}">"#
    );
    let _ = writeln!(
        out,
        r#"  <testsuite name="mcp-trace-validator ({})" tests="{tests}" failures="{failures}" skipped="{skipped}">"#,
        escape(&report.revision)
    );

    for row in &report.requirements {
        render_row(&mut out, report, row);
    }

    out.push_str("  </testsuite>\n</testsuites>\n");
    out
}

fn render_row(out: &mut String, report: &Report, row: &crate::report::RequirementReport) {
    let name = escape(&format!("{} ({})", row.id, row.level));
    let classname = escape(&format!("mcp.{}", report.revision));
    match row.outcome {
        Outcome::Pass => {
            let _ = writeln!(
                out,
                r#"    <testcase classname="{classname}" name="{name}"/>"#
            );
        }
        Outcome::Fail => {
            let _ = writeln!(
                out,
                r#"    <testcase classname="{classname}" name="{name}">"#
            );
            for finding in &row.findings {
                let _ = writeln!(
                    out,
                    r#"      <failure message="{}">{}</failure>"#,
                    escape(&finding.detail),
                    escape(&location(finding.seq, &finding.check)),
                );
            }
            out.push_str("    </testcase>\n");
        }
        Outcome::Warn => {
            let _ = writeln!(
                out,
                r#"    <testcase classname="{classname}" name="{name}">"#
            );
            out.push_str("      <system-out>");
            for finding in &row.findings {
                let _ = writeln!(
                    out,
                    "{}: {}",
                    escape(&location(finding.seq, &finding.check)),
                    escape(&finding.detail)
                );
            }
            out.push_str("</system-out>\n    </testcase>\n");
        }
        Outcome::Excluded | Outcome::Unsupported | Outcome::NotApplicable => {
            let _ = writeln!(
                out,
                r#"    <testcase classname="{classname}" name="{name}"><skipped message="{}"/></testcase>"#,
                escape(&skip_reason(row))
            );
        }
    }
}

/// The `<skipped>` message for the three non-judged outcomes.
fn skip_reason(row: &crate::report::RequirementReport) -> String {
    match row.outcome {
        Outcome::Excluded => row
            .exclusion
            .clone()
            .unwrap_or_else(|| "excluded".to_owned()),
        Outcome::NotApplicable => format!(
            "not applicable: capability {} was not declared in this session",
            row.capability.as_deref().unwrap_or("(unknown)")
        ),
        _ => format!(
            "registry references checks this build does not implement: {}",
            row.missing_checks.join(", ")
        ),
    }
}

fn location(seq: Option<u64>, check: &str) -> String {
    seq.map_or_else(
        || format!("[{check}]"),
        |seq| format!("[{check}] at seq {seq}"),
    )
}

/// XML escaping for text and attribute content (we always double-quote
/// attributes, so escaping `"` but not `'` is sufficient).
///
/// Findings quote trace strings — method names, ids — that come from untrusted
/// input and may carry characters XML 1.0 forbids entirely. C0 control
/// characters other than tab/LF/CR cannot appear in an XML 1.0 document even as
/// numeric references (XML 1.0 §2.2), so a trace whose method name contains,
/// say, U+0001 would otherwise produce a document a strict CI parser rejects.
/// Those characters are replaced with U+FFFD (the Unicode replacement
/// character) so the output is always well-formed.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(ch),
            // C0 controls (except the three above) are not representable in
            // XML 1.0 at all; substitute rather than emit an invalid document.
            c if (c as u32) < 0x20 => out.push('\u{FFFD}'),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
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
        }
    }

    #[test]
    fn skip_accounting_and_location_text_are_exact() {
        use crate::report::{Finding, Totals};
        // Hand-built report with excluded, unsupported, AND not-applicable rows:
        // pins the skipped sum (excluded + unsupported + not_applicable), the
        // per-variant skip messages, and the failure-body location text.
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
        let report = Report {
            revision: "2025-11-25".to_owned(),
            totals: Totals {
                pass: 0,
                fail: 1,
                warn: 0,
                excluded: 2,
                unsupported: 1,
                not_applicable: 1,
            },
            requirements: vec![failed, excluded_a, excluded_b, unsupported, not_applicable],
        };
        let xml = render(&report);
        // skipped = excluded + unsupported + not_applicable, no other arithmetic.
        assert!(
            xml.contains(r#"<testsuites tests="5" failures="1" skipped="4">"#),
            "{xml}"
        );
        assert!(
            xml.contains(
                r#"<skipped message="not applicable: capability server.tools was not declared in this session"/>"#
            ),
            "{xml}"
        );
        // The two skip variants carry their own distinct messages.
        assert!(
            xml.contains(r#"<skipped message="not judgeable from traces"/>"#),
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
}
