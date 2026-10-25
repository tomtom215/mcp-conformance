// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The test cases of a `JUnit` suite: one per requirement row, and the wire
//! schema's, and the counts the suite's attributes carry.
//!
//! A row whose findings the report's view does not count — the other party's,
//! or accepted by a baseline — never fails the document, because the exit
//! status does not fail for it either. The two are told apart: a set-aside row
//! is a passing case that shows its findings, and an accepted one is
//! `<skipped>`, because a finding that is still there is not a pass.

use core::fmt::Write as _;

use super::Options;
use super::text::{
    Standing, acceptance_note, accepted_line, actor_token, clause_lines, escape, location,
    properties, set_aside_line, skip_reason, standing,
};
use crate::report::{Finding, Outcome, Report, RequirementReport, WireSchemaReport};

/// `(tests, failures, skipped)` for one report; warnings count as failures when
/// `strict` renders them as such.
///
/// The wire-schema case, when the report has one, counts like a row: a test,
/// failed when a message was invalid and skipped when there were none. Under a
/// party view or a baseline a row (or the schema) fails only through a finding
/// that counts, which is exactly when [`render_row`] writes a `<failure>`; one
/// whose findings a baseline accepts is skipped.
pub(super) fn counts(report: &Report, strict: bool) -> (u32, u32, u32) {
    let totals = report.totals;
    let mut skipped =
        totals.excluded + totals.unsupported + totals.not_applicable + totals.not_observed;
    let stands = |row: &RequirementReport| standing(&row.findings, report.party_view);
    // The rows that would fail — failing (or, under strict, warning) — and do
    // not count. None without a view or a baseline, so the count is then the
    // totals' own.
    let not_counted = report
        .requirements
        .iter()
        .filter(|row| {
            matches!(row.outcome, Outcome::Fail) || (strict && matches!(row.outcome, Outcome::Warn))
        })
        .filter(|row| stands(row) != Standing::Counts)
        .count();
    let accepted = report
        .requirements
        .iter()
        .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
        .filter(|row| stands(row) == Standing::Accepted)
        .count();
    let mut failures = if strict {
        totals.fail + totals.warn
    } else {
        totals.fail
    }
    .saturating_sub(u32::try_from(not_counted).unwrap_or(u32::MAX));
    skipped += u32::try_from(accepted).unwrap_or(u32::MAX);
    let mut tests = totals.pass
        + totals.fail
        + totals.warn
        + totals.excluded
        + totals.unsupported
        + totals.not_applicable
        + totals.not_observed;
    if let Some(wire_schema) = &report.wire_schema {
        tests += 1;
        match wire_schema.outcome {
            Outcome::Fail => match standing(&wire_schema.findings, report.party_view) {
                Standing::Counts => failures += 1,
                Standing::Accepted => skipped += 1,
                Standing::SetAside => {}
            },
            Outcome::NotObserved => skipped += 1,
            _ => {}
        }
    }
    (tests, failures, skipped)
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

/// Each finding as a `system-out` line — its location and detail, and why a
/// baseline accepts it — for the findings `keep` approves.
fn finding_lines(out: &mut String, findings: &[Finding], keep: impl Fn(&Finding) -> bool) {
    for finding in findings.iter().filter(|finding| keep(finding)) {
        let _ = writeln!(
            out,
            "{}: {}{}",
            escape(&location(finding)),
            escape(&finding.detail),
            escape(&acceptance_note(finding))
        );
    }
}

/// An accepted case: skipped, with every finding still listed beside why it is
/// accepted. The document counts it in `skipped`, not `failures` and not as a
/// pass.
fn render_accepted(
    out: &mut String,
    attrs: &str,
    name: &str,
    row_actor: Option<&str>,
    findings: &[Finding],
    clause: &str,
) {
    let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
    properties(out, row_actor, findings);
    let _ = writeln!(
        out,
        r#"      <skipped message="{}"/>"#,
        escape(accepted_line(findings).trim_end())
    );
    out.push_str("      <system-out>");
    finding_lines(out, findings, |_| true);
    out.push_str(escape(clause.trim_start()).as_str());
    out.push_str("</system-out>\n    </testcase>\n");
}

/// A failing case: a `<failure>` for each finding no baseline accepts, then the
/// accepted ones in `<system-out>` — they are not failures, and they are shown.
fn render_failing(out: &mut String, attrs: &str, name: &str, row: &RequirementReport) {
    let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
    properties(out, actor_token(row), &row.findings);
    for finding in row
        .findings
        .iter()
        .filter(|finding| !finding.is_baselined())
    {
        let _ = writeln!(
            out,
            r#"      <failure message="{}">{}{}</failure>"#,
            escape(&finding.detail),
            escape(&location(finding)),
            escape(&clause_lines(row)),
        );
    }
    if row.findings.iter().any(Finding::is_baselined) {
        out.push_str("      <system-out>");
        finding_lines(out, &row.findings, Finding::is_baselined);
        out.push_str("</system-out>\n");
    }
    out.push_str("    </testcase>\n");
}

/// A passing case that shows its findings in `<system-out>`: a warning, or a
/// row the party view `set_aside` (headed so).
fn render_warning(
    out: &mut String,
    attrs: &str,
    name: &str,
    row: &RequirementReport,
    set_aside: Option<crate::report::Party>,
) {
    let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
    properties(out, actor_token(row), &row.findings);
    out.push_str("      <system-out>");
    if let Some(party) = set_aside {
        out.push_str(&escape(&set_aside_line(party)));
    }
    finding_lines(out, &row.findings, |_| true);
    out.push_str(escape(clause_lines(row).trim_start()).as_str());
    out.push_str("</system-out>\n    </testcase>\n");
}

pub(super) fn render_row(
    out: &mut String,
    report: &Report,
    row: &RequirementReport,
    options: &Options,
) {
    let name = escape(&format!("{} ({})", row.id, row.level));
    let attrs = case_attributes(report, options);
    let stands = matches!(row.outcome, Outcome::Fail | Outcome::Warn)
        .then(|| standing(&row.findings, report.party_view));
    if stands == Some(Standing::Accepted) {
        render_accepted(
            out,
            &attrs,
            &name,
            actor_token(row),
            &row.findings,
            &clause_lines(row),
        );
        return;
    }
    // A `fail` or `warn` row none of whose findings the view counts is a
    // passing case that still shows them: the exit status set them aside, and
    // so does the document.
    let set_aside = stands == Some(Standing::SetAside);
    let outcome = match row.outcome {
        Outcome::Warn if options.strict && !set_aside => Outcome::Fail,
        Outcome::Fail if set_aside => Outcome::Warn,
        other => other,
    };
    match outcome {
        Outcome::Pass => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}"/>"#);
        }
        Outcome::Fail => render_failing(out, &attrs, &name, row),
        Outcome::Warn => render_warning(
            out,
            &attrs,
            &name,
            row,
            report.party_view.filter(|_| set_aside),
        ),
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

/// The wire-schema test case: one `<failure>` per invalid message that counts
/// — or, when a party view counts none of them, a passing case showing them,
/// like a row, and, when a baseline accepts them, a skipped one.
pub(super) fn render_wire_schema(
    out: &mut String,
    report: &Report,
    wire_schema: &WireSchemaReport,
    options: &Options,
) {
    let attrs = case_attributes(report, options);
    let name = "wire schema";
    let stands = standing(&wire_schema.findings, report.party_view);
    let clause = format!(
        "\nspec: \"{}\"\nsee: {}\nschema: {}",
        wire_schema.source.quote, wire_schema.source.url, wire_schema.schema
    );
    match (wire_schema.outcome, stands) {
        (Outcome::Fail, Standing::Counts) => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
            properties(out, None, &wire_schema.findings);
            for finding in wire_schema
                .findings
                .iter()
                .filter(|finding| !finding.is_baselined())
            {
                let _ = writeln!(
                    out,
                    r#"      <failure message="{}">{}{}</failure>"#,
                    escape(&finding.detail),
                    escape(&location(finding)),
                    escape(&clause),
                );
            }
            if wire_schema.findings.iter().any(Finding::is_baselined) {
                out.push_str("      <system-out>");
                finding_lines(out, &wire_schema.findings, Finding::is_baselined);
                out.push_str("</system-out>\n");
            }
            out.push_str("    </testcase>\n");
        }
        (Outcome::Fail, Standing::Accepted) => {
            render_accepted(out, &attrs, name, None, &wire_schema.findings, "");
        }
        (Outcome::Fail, Standing::SetAside) => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}">"#);
            properties(out, None, &wire_schema.findings);
            out.push_str("      <system-out>");
            if let Some(party) = report.party_view {
                out.push_str(&escape(&set_aside_line(party)));
            }
            finding_lines(out, &wire_schema.findings, |_| true);
            out.push_str("</system-out>\n    </testcase>\n");
        }
        (Outcome::NotObserved, _) => {
            let _ = writeln!(
                out,
                r#"    <testcase {attrs} name="{name}"><skipped message="the trace holds no message"/></testcase>"#
            );
        }
        _ => {
            let _ = writeln!(out, r#"    <testcase {attrs} name="{name}"/>"#);
        }
    }
}
