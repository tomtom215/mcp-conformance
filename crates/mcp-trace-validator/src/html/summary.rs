// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The top of a page: what was judged, the verdict, and every outcome's count.

use core::fmt::Write as _;

use super::escape::Text;
use super::{Scope, Trace, trace_verdict};
use crate::report::{Outcome, Report, Totals, Unjudged, Verdict};

/// The verdict as a heading reads it.
pub(super) const fn verdict_word(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "Pass",
        Verdict::PassWithWarnings => "Pass with warnings",
        Verdict::Fail => "Fail",
        Verdict::Unsupported => "Unsupported",
        Verdict::Incomplete => "Incomplete",
    }
}

/// The verdict's style class, and the token the other formats print.
pub(super) const fn verdict_class(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "pass",
        Verdict::PassWithWarnings => "warn",
        Verdict::Fail => "fail",
        Verdict::Unsupported => "unsupported",
        Verdict::Incomplete => "incomplete",
    }
}

/// A verdict badge: the word, styled by the verdict's token.
fn badge(out: &mut String, verdict: Verdict) {
    let _ = write!(
        out,
        "<span class=\"badge v-{}\">{}</span>",
        verdict_class(verdict),
        verdict_word(verdict)
    );
}

/// The page header of a one-trace page.
pub(super) fn masthead(out: &mut String, scope: &Scope<'_>, eyebrow: &str) {
    let _ = writeln!(
        out,
        "<header class=\"masthead\"><div class=\"wrap\">\n<p class=\"eyebrow\">{eyebrow}</p>\n\
         <h1 class=\"trace-name\">{}</h1>",
        Text(&scope.name())
    );
    meta(out, scope);
    out.push_str("</div></header>\n");
}

/// A trace's heading inside a run page.
pub(super) fn trace_heading(out: &mut String, scope: &Scope<'_>, index: usize) {
    let _ = writeln!(
        out,
        "<h2 class=\"trace-name\" id=\"t{index}-title\">{}</h2>",
        Text(&scope.name())
    );
    meta(out, scope);
}

/// One line under a trace's name: revisions, how they were chosen, the size
/// and transport of the recording.
fn meta(out: &mut String, scope: &Scope<'_>) {
    let revisions: Vec<&str> = scope.reports.iter().map(|r| r.revision.as_str()).collect();
    let noun = if revisions.len() == 1 {
        "revision"
    } else {
        "revisions"
    };
    let _ = write!(
        out,
        "<p class=\"meta\"><span>{noun} <strong>{}</strong>",
        Text(&revisions.join(", "))
    );
    if let Some(source) = scope.reports.first().and_then(|r| r.revision_source) {
        let _ = write!(out, " ({source})");
    }
    let events = scope.trace.events.len();
    let _ = writeln!(
        out,
        "</span><span>{events} {}</span><span>{}</span></p>",
        if events == 1 { "event" } else { "events" },
        scope.transport()
    );
}

/// The page header of a run over several traces.
pub(super) fn run_masthead(out: &mut String, total: usize) {
    let _ = writeln!(
        out,
        "<header class=\"masthead\"><div class=\"wrap\">\n<p class=\"eyebrow\">MCP trace validation run</p>\n\
         <h1>{total} {}</h1>\n</div></header>",
        if total == 1 { "trace" } else { "traces" }
    );
}

/// A run's verdict, and a line per trace with its own.
pub(super) fn run_summary(
    out: &mut String,
    traces: &[(Trace<'_>, &[Report])],
    unjudged: &[Unjudged],
    overall: Verdict,
) {
    let _ = write!(
        out,
        "<section class=\"panel verdict v-{}\" aria-label=\"Run verdict\">\n<p class=\"verdict-line\">",
        verdict_class(overall)
    );
    badge(out, overall);
    out.push_str("<span class=\"verdict-why\">the most severe verdict of the run");
    if !unjudged.is_empty() {
        out.push_str(", which left a trace unjudged");
    }
    out.push_str("</span></p>\n<ol class=\"runlist\">\n");
    for (index, (trace, reports)) in traces.iter().enumerate() {
        let _ = write!(
            out,
            "<li><a href=\"#t{index}\">{}</a>",
            Text(trace.name.unwrap_or("standard input"))
        );
        badge(out, trace_verdict(reports));
        out.push_str("</li>\n");
    }
    for trace in unjudged {
        let _ = write!(
            out,
            "<li><span class=\"unjudged-name\">{}</span>",
            Text(&trace.trace)
        );
        badge(out, Verdict::Incomplete);
        let _ = writeln!(
            out,
            "<p class=\"unjudged-why\">Not judged: {}</p></li>",
            Text(&trace.error)
        );
    }
    out.push_str("</ol>\n</section>\n");
}

/// The verdict panel: the verdict, why, and the counts — one card per revision
/// when there are several.
pub(super) fn verdict(out: &mut String, scope: &Scope<'_>) {
    let overall = trace_verdict(scope.reports);
    let _ = write!(
        out,
        "<section class=\"panel verdict v-{}\" aria-label=\"Verdict\">\n<p class=\"verdict-line\">",
        verdict_class(overall)
    );
    badge(out, overall);
    if let [report] = scope.reports {
        let _ = writeln!(
            out,
            "<span class=\"verdict-why\">{}</span></p>",
            why(report)
        );
        tally(out, &report.totals);
        schema_line(out, report);
    } else {
        let _ = writeln!(
            out,
            "<span class=\"verdict-why\">the most severe of {} revisions</span></p>",
            scope.reports.len()
        );
        out.push_str("<div class=\"revisions\">\n");
        for report in scope.reports {
            let _ = write!(
                out,
                "<section class=\"revision v-{}\" aria-label=\"Revision {}\">\n<p class=\"revision-line\">\
                 <span class=\"revision-name\">{}</span>",
                verdict_class(report.verdict()),
                Text(&report.revision),
                Text(&report.revision)
            );
            badge(out, report.verdict());
            let _ = writeln!(out, "</p>\n<p class=\"verdict-why\">{}</p>", why(report));
            tally(out, &report.totals);
            schema_line(out, report);
            out.push_str("</section>\n");
        }
        out.push_str("</div>\n");
    }
    out.push_str("</section>\n");
}

/// Why the report reached its verdict, in a sentence.
fn why(report: &Report) -> String {
    let totals = report.totals;
    let mut parts = Vec::new();
    if totals.fail > 0 {
        parts.push(plural(totals.fail, "clause failed", "clauses failed"));
    }
    if totals.warn > 0 {
        parts.push(plural(totals.warn, "clause warned", "clauses warned"));
    }
    if totals.unsupported > 0 {
        parts.push(plural(
            totals.unsupported,
            "clause needs a check this build lacks",
            "clauses need checks this build lacks",
        ));
    }
    if let Some(schema) = report
        .wire_schema
        .as_ref()
        .filter(|s| s.outcome == Outcome::Fail)
    {
        parts.push(format!(
            "{} of {} messages invalid against the schema",
            schema.findings.len(),
            schema.messages
        ));
    }
    match parts.as_slice() {
        [] => format!(
            "no clause failed or warned; {} passed",
            plural(totals.pass, "clause", "clauses")
        ),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// `count` and the noun that agrees with it.
fn plural(count: u32, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// Every outcome's count, in report order, so the sum is the registry's size.
fn tally(out: &mut String, totals: &Totals) {
    out.push_str("<dl class=\"tally\">\n");
    for (label, count) in totals.labelled() {
        let class = label.replace(' ', "-");
        let zero = if count == 0 { " zero" } else { "" };
        let _ = writeln!(
            out,
            "<div class=\"t-{class}{zero}\"><dt>{label}</dt><dd>{count}</dd></div>"
        );
    }
    out.push_str("</dl>\n");
}

/// The wire-schema outcome, under the counts it is not part of.
fn schema_line(out: &mut String, report: &Report) {
    if let Some(schema) = &report.wire_schema {
        let class = match schema.outcome {
            Outcome::Fail => "fail",
            Outcome::NotObserved => "not-observed",
            _ => "pass",
        };
        let _ = writeln!(
            out,
            "<p class=\"schema-line s-{class}\">Wire schema: {}</p>",
            Text(&schema.summary())
        );
    }
}

/// The note for each revision the session did not declare.
pub(super) fn mismatch_notes(out: &mut String, scope: &Scope<'_>) {
    for report in scope.reports {
        let Some(declared) = &report.revision_mismatch else {
            continue;
        };
        let suggestion = declared.last().map_or("", String::as_str);
        let _ = writeln!(
            out,
            "<aside class=\"note\" role=\"note\"><strong>Judged by rules the session was not \
             playing by.</strong> This session declares protocol {} {}, not {}; every outcome \
             at {} judges it against another revision's rules. Judge it at {} \
             (<code>--revision {}</code>) to hold it to its own.</aside>",
            if declared.len() == 1 {
                "revision"
            } else {
                "revisions"
            },
            Text(&declared.join(", ")),
            Text(&report.revision),
            Text(&report.revision),
            Text(suggestion),
            Text(suggestion)
        );
    }
}

/// The section links under the verdict.
pub(super) fn contents(out: &mut String, scope: &Scope<'_>) {
    let attention: usize = scope
        .reports
        .iter()
        .map(|report| {
            report
                .requirements
                .iter()
                .filter(|row| row.outcome.needs_attention())
                .count()
                + usize::from(report.wire_schema_failed())
        })
        .sum();
    let _ = writeln!(
        out,
        "<nav class=\"contents\" aria-label=\"Sections\"><a href=\"#{}\">Clauses to act on \
         <span class=\"count\">{attention}</span></a><a href=\"#{}\">Message timeline \
         <span class=\"count\">{}</span></a><a href=\"#{}\">Accounting</a></nav>",
        scope.id("findings"),
        scope.id("timeline"),
        scope.trace.events.len(),
        scope.id("accounting"),
    );
}
