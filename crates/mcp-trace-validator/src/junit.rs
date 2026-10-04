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
    render_all(core::slice::from_ref(report))
}

/// Renders several single-revision reports as one `JUnit` document.
///
/// One trace judged under each revision gives one `<testsuite>` per revision, in the
/// order given; the document-level counts are the sums of the suites'.
///
/// ```
/// use mcp_conformance_core::requirement::RegistrySet;
/// use mcp_trace_validator::{engine, junit};
///
/// let set = RegistrySet::builtin()?;
/// let reports: Vec<_> = set
///     .revisions()
///     .iter()
///     .filter_map(|revision| set.registry(*revision))
///     .map(|registry| engine::validate(&registry, &[]))
///     .collect();
/// let xml = junit::render_all(&reports);
/// assert_eq!(xml.matches("<testsuite ").count(), 2);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_all(reports: &[Report]) -> String {
    render_with(reports, &Options::default())
}

/// How [`render_with`] presents a run.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Options {
    /// The trace the reports judge, as the caller named it. Names each suite and
    /// becomes each test case's `file` and `classname` prefix, so a CI report
    /// aggregating several traces can tell their identically named clauses apart.
    pub trace: Option<String>,
    /// Render SHOULD-level findings (warnings) as `<failure>`s, matching a run
    /// whose exit status `--strict` makes them fail. Without it a warning is a
    /// passing test case with its findings in `<system-out>`.
    pub strict: bool,
}

impl Options {
    /// Options naming `trace`, otherwise the defaults.
    #[must_use]
    pub fn for_trace(trace: impl Into<String>) -> Self {
        Self {
            trace: Some(trace.into()),
            strict: false,
        }
    }

    /// These options, with warnings rendered as failures.
    #[must_use]
    pub const fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

/// Renders several single-revision reports as one `JUnit` document, as
/// [`render_all`] does, with the presentation `options` choose.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, junit};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let report = engine::validate(&registry, &[]);
/// let xml = junit::render_with(&[report], &junit::Options::for_trace("session.jsonl"));
/// assert!(xml.contains(r#"<testsuite name="session.jsonl (2025-11-25)""#));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_with(reports: &[Report], options: &Options) -> String {
    render_traces(&[(options.clone(), reports)])
}

/// Renders the reports of several traces as one `JUnit` document: a
/// `<testsuite>` per trace and revision, each named by its trace's
/// [`Options::trace`], with the document-level counts summed across all.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, junit};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let reports = [engine::validate(&registry, &[])];
/// let xml = junit::render_traces(&[
///     (junit::Options::for_trace("a.jsonl"), &reports[..]),
///     (junit::Options::for_trace("b.jsonl"), &reports[..]),
/// ]);
/// assert_eq!(xml.matches("<testsuite ").count(), 2);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_traces(traces: &[(Options, &[Report])]) -> String {
    let suites: Vec<(&Options, &Report, (u32, u32, u32))> = traces
        .iter()
        .flat_map(|(options, reports)| {
            reports
                .iter()
                .map(move |report| (options, report, counts(report, options.strict)))
        })
        .collect();
    let (tests, failures, skipped) = suites.iter().fold((0, 0, 0), |sum, (_, _, count)| {
        (sum.0 + count.0, sum.1 + count.1, sum.2 + count.2)
    });
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    let _ = writeln!(
        out,
        r#"<testsuites tests="{tests}" failures="{failures}" skipped="{skipped}">"#
    );
    for (options, report, (tests, failures, skipped)) in suites {
        let suite = options.trace.as_deref().unwrap_or("mcp-trace-validator");
        let _ = writeln!(
            out,
            r#"  <testsuite name="{} ({})" tests="{tests}" failures="{failures}" skipped="{skipped}">"#,
            escape(suite),
            escape(&report.revision)
        );
        for row in &report.requirements {
            render_row(&mut out, report, row, options);
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

/// `(tests, failures, skipped)` for one report; warnings count as failures when
/// `strict` renders them as such.
const fn counts(report: &Report, strict: bool) -> (u32, u32, u32) {
    let totals = report.totals;
    let skipped =
        totals.excluded + totals.unsupported + totals.not_applicable + totals.not_observed;
    let failures = if strict {
        totals.fail + totals.warn
    } else {
        totals.fail
    };
    (
        totals.pass + totals.fail + totals.warn + skipped,
        failures,
        skipped,
    )
}

/// A test case's identifying attributes: its class, and the trace as a `file`
/// attribute when one is named.
fn case_attributes(report: &Report, options: &Options) -> String {
    options.trace.as_deref().map_or_else(
        || {
            format!(
                r#"classname="{}""#,
                escape(&format!("mcp.{}", report.revision))
            )
        },
        |trace| {
            format!(
                r#"classname="{}" file="{}""#,
                escape(&format!("{trace}.mcp.{}", report.revision)),
                escape(trace)
            )
        },
    )
}

fn render_row(
    out: &mut String,
    report: &Report,
    row: &crate::report::RequirementReport,
    options: &Options,
) {
    let name = escape(&format!("{} ({})", row.id, row.level));
    let attrs = case_attributes(report, options);
    let outcome = if options.strict && row.outcome == Outcome::Warn {
        Outcome::Fail
    } else {
        row.outcome
    };
    match outcome {
        Outcome::Pass => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}"/>"#);
        }
        Outcome::Fail => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
            for finding in &row.findings {
                let _ = writeln!(
                    out,
                    r#"      <failure message="{}">{}{}</failure>"#,
                    escape(&finding.detail),
                    escape(&location(finding.seq, &finding.check)),
                    escape(&clause_lines(row)),
                );
            }
            out.push_str("    </testcase>\n");
        }
        Outcome::Warn => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
            out.push_str("      <system-out>");
            for finding in &row.findings {
                let _ = writeln!(
                    out,
                    "{}: {}",
                    escape(&location(finding.seq, &finding.check)),
                    escape(&finding.detail)
                );
            }
            out.push_str(escape(clause_lines(row).trim_start()).as_str());
            out.push_str("</system-out>\n    </testcase>\n");
        }
        Outcome::Excluded
        | Outcome::Unsupported
        | Outcome::NotApplicable
        | Outcome::NotObserved => {
            let _ = writeln!(
                out,
                r#"    <testcase {attrs} name="{name}"><skipped message="{}"/></testcase>"#,
                escape(&skip_reason(row))
            );
        }
    }
}

/// The `<skipped>` message for the four non-judged outcomes.
fn skip_reason(row: &crate::report::RequirementReport) -> String {
    match row.outcome {
        Outcome::Excluded => row
            .exclusion
            .clone()
            .unwrap_or_else(|| "excluded".to_owned()),
        Outcome::NotObserved => {
            "the session carried none of the traffic this clause binds to".to_owned()
        }
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

/// The violated clause as body text — `\nspec: "…"\nsee: <url>` — or nothing when
/// the row carries no source. CI systems show a failure's body beside its message,
/// so this is what puts the clause in front of the person reading the failure.
fn clause_lines(row: &crate::report::RequirementReport) -> String {
    row.source.as_ref().map_or_else(String::new, |source| {
        format!("\nspec: \"{}\"\nsee: {}", source.quote, source.url)
    })
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
mod tests;
