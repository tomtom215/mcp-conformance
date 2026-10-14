// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! An extension's clauses as a `JUnit` suite of their own (ADR-0023): judged,
//! shown, and never a failure.

use core::fmt::Write as _;

use super::{Options, clause_lines, escape, location, skip_reason};
use crate::report::{ExtensionReport, Outcome, Report, RequirementReport};

/// `(tests, failures, skipped)` for one extension's suite: never a failure.
pub(super) const fn extension_counts(extension: &ExtensionReport) -> (u32, u32, u32) {
    let totals = extension.totals;
    let skipped =
        totals.excluded + totals.unsupported + totals.not_applicable + totals.not_observed;
    (
        totals.pass + totals.fail + totals.warn + skipped,
        0,
        skipped,
    )
}

/// One extension's suite: its clauses as test cases, a finding in a passing
/// case's `<system-out>`, so the CI that ingests the file shows it and fails
/// nothing over it.
pub(super) fn render_extension(
    out: &mut String,
    suite: &str,
    report: &Report,
    extension: &ExtensionReport,
    options: &Options,
) {
    let (tests, _, skipped) = extension_counts(extension);
    let _ = writeln!(
        out,
        r#"  <testsuite name="{} ({}, extension {}, not scored)" tests="{tests}" failures="0" skipped="{skipped}">"#,
        escape(suite),
        escape(&report.revision),
        escape(&extension.extension)
    );
    let class = format!("mcp.{}.{}", report.revision, extension.extension);
    let attrs = options.trace.as_deref().map_or_else(
        || format!(r#"classname="{}""#, escape(&class)),
        |trace| {
            format!(
                r#"classname="{}" file="{}""#,
                escape(&format!("{trace}.{class}")),
                escape(trace)
            )
        },
    );
    for row in &extension.requirements {
        render_extension_row(out, &attrs, row);
    }
    out.push_str("  </testsuite>\n");
}

/// One extension clause as a test case: passing, skipped with its reason, or
/// passing with its findings in `<system-out>`.
fn render_extension_row(out: &mut String, attrs: &str, row: &RequirementReport) {
    let name = escape(&format!("{} ({})", row.id, row.level));
    match row.outcome {
        Outcome::Pass => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}"/>"#);
        }
        Outcome::Fail | Outcome::Warn => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
            let _ = writeln!(
                out,
                "      <system-out>not scored ({}, extension): the clause {}s",
                escape(&row.level),
                if row.outcome == Outcome::Fail {
                    "fail"
                } else {
                    "warn"
                }
            );
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
