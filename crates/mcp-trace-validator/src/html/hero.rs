// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The first thing a reader sees: the verdict, what it means in plain words,
//! and what to do next. The clause-level wording stays one line below it.

use core::fmt::Write as _;

use super::clauses::WIRE_SCHEMA;
use super::escape::Text;
use super::summary::{plural, verdict_class, verdict_word};
use super::{Scope, Trace, trace_verdict};
use crate::report::{Report, Unjudged, Verdict};

/// The verdict's glyph: a different shape per verdict, so it is never told by
/// colour alone. Drawn inline; the page loads no image.
const fn glyph(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Pass => "M5.5 12.5l4.5 4.5 8.5-9.5",
        Verdict::PassWithWarnings => "M12 6.5v7M12 17.4v.2",
        Verdict::Fail => "M7 7l10 10M17 7L7 17",
        Verdict::Unsupported | Verdict::Incomplete => {
            "M9 9.4a3 3 0 1 1 4.7 2.5c-1 .7-1.7 1.3-1.7 2.4M12 17.4v.2"
        }
    }
}

/// Opens the hero: the glyph, the verdict as the largest thing on the page,
/// and `headline` under it. [`close`] ends it.
pub(super) fn open(out: &mut String, verdict: Verdict, headline: &str) {
    let _ = writeln!(
        out,
        "<div class=\"hero\"><span class=\"hero-icon\" aria-hidden=\"true\">\
         <svg class=\"icon\" viewBox=\"0 0 24 24\" focusable=\"false\"><path d=\"{}\"/></svg></span>\n\
         <div class=\"hero-body\">\n<p class=\"verdict-line\"><span class=\"badge v-{}\">{}</span></p>\n\
         <p class=\"headline\">{}</p>",
        glyph(verdict),
        verdict_class(verdict),
        verdict_word(verdict),
        Text(headline)
    );
}

/// The validator's own wording of the verdict, one step down, and the end of
/// the hero.
pub(super) fn close(out: &mut String, why: &str) {
    let _ = writeln!(
        out,
        "<p class=\"verdict-detail\">In the validator's terms: \
         <span class=\"verdict-why\">{why}</span></p>\n</div></div>"
    );
}

/// What one report's verdict means, in a sentence a newcomer can act on.
pub(super) fn headline(report: &Report) -> String {
    let totals = report.totals;
    let mut sentence = match report.effective_verdict() {
        Verdict::Fail => broken(report),
        Verdict::PassWithWarnings => format!(
            "No required rule was broken, but {} not followed.",
            plural(totals.warn, "recommendation was", "recommendations were")
        ),
        // A party view or a baseline set every finding aside.
        Verdict::Pass if report.verdict() != Verdict::Pass => {
            "No finding counts against this session here: the ones it has are set aside, as the \
             line below says."
                .to_owned()
        }
        Verdict::Pass if totals.pass == 0 => {
            "No rule was broken, but none could be checked against this session.".to_owned()
        }
        Verdict::Pass if totals.pass == 1 => {
            "No rule was broken: the one rule this session could be checked against held."
                .to_owned()
        }
        Verdict::Pass => format!(
            "No rule was broken: all {} rules this session could be checked against held.",
            totals.pass
        ),
        Verdict::Unsupported => format!(
            "{} could not be checked by this version of the tool, so the result is not a pass.",
            plural(totals.unsupported, "rule", "rules")
        ),
        Verdict::Incomplete => "This session could not be judged.".to_owned(),
    };
    if let Some(party) = report.party_view {
        let _ = write!(sentence, " Only what is held against the {party} counts.");
    }
    sentence
}

/// The failing sentence: MUST-level rules broken, messages the schema refuses,
/// and any recommendations beside them.
fn broken(report: &Report) -> String {
    let totals = report.totals;
    let mut parts = Vec::new();
    if totals.fail > 0 {
        parts.push(format!(
            "broke {}",
            plural(totals.fail, "MUST-level rule", "MUST-level rules")
        ));
    }
    if let Some(schema) = report
        .wire_schema
        .as_ref()
        .filter(|_| report.wire_schema_failed())
    {
        let invalid = u32::try_from(schema.findings.len()).unwrap_or(u32::MAX);
        parts.push(format!(
            "sent {} that {} not match the protocol's schema",
            plural(invalid, "message", "messages"),
            if invalid == 1 { "does" } else { "do" }
        ));
    }
    let mut sentence = if parts.is_empty() {
        "This session has findings held against it.".to_owned()
    } else {
        format!("This session {}.", parts.join(" and "))
    };
    if totals.warn > 0 {
        let _ = write!(
            sentence,
            " It also did not follow {}.",
            plural(totals.warn, "recommendation", "recommendations")
        );
    }
    sentence
}

/// What several revisions' verdicts mean together.
pub(super) fn revisions_headline(reports: &[Report]) -> String {
    let failing = reports
        .iter()
        .filter(|report| report.effective_verdict() != Verdict::Pass)
        .count();
    let failing = u32::try_from(failing).unwrap_or(u32::MAX);
    if failing == 0 {
        format!(
            "No rule was broken at any of the {} protocol revisions this session was judged at.",
            reports.len()
        )
    } else {
        format!(
            "This session was judged at {} protocol revisions, and {} something to look at. \
             The verdict here is the most severe of them.",
            reports.len(),
            plural(failing, "revision has", "revisions have")
        )
    }
}

/// The first clause a reader should open: its anchor and its name.
fn first_to_act_on(scope: &Scope<'_>) -> Option<(String, String)> {
    scope.reports.iter().find_map(|report| {
        report
            .requirements
            .iter()
            .find(|row| row.outcome.needs_attention())
            .map(|row| (scope.clause(&report.revision, &row.id), row.id.clone()))
            .or_else(|| {
                report.wire_schema_failed().then(|| {
                    (
                        scope.clause(&report.revision, WIRE_SCHEMA),
                        "Wire schema".to_owned(),
                    )
                })
            })
    })
}

/// The "what to do next" line under the headline.
pub(super) fn next_step(out: &mut String, scope: &Scope<'_>, verdict: Verdict) {
    out.push_str("<p class=\"next\"><strong>What to do next</strong> ");
    let first = first_to_act_on(scope);
    let link = |out: &mut String| {
        if let Some((anchor, name)) = &first {
            let _ = write!(out, "<a href=\"#{anchor}\">{}</a>", Text(name));
        } else {
            let _ = write!(
                out,
                "<a href=\"#{}\">the cards below</a>",
                scope.id("findings")
            );
        }
    };
    match verdict {
        Verdict::Fail => {
            out.push_str("Start with ");
            link(out);
            out.push_str(
                ": its card says what was seen, quotes the rule, and links to the exact message. \
                 Fix what it names, record the session again, and run this check again.",
            );
        }
        Verdict::PassWithWarnings => {
            out.push_str("Nothing has to change. When you can, read ");
            link(out);
            out.push_str(": a recommendation is there because ignoring it tends to cause trouble.");
        }
        Verdict::Unsupported => {
            out.push_str("Update the validator and run it again. ");
            link(out);
            out.push_str(" names the check this build does not have.");
        }
        Verdict::Pass | Verdict::Incomplete => {
            let unseen: u32 = scope.reports.iter().map(|r| r.totals.not_observed).sum();
            out.push_str("Nothing to fix.");
            if unseen > 0 {
                let _ = write!(
                    out,
                    " A pass covers only what this session did: {} never came up in it. To \
                     check those, record a session that uses more of the protocol \
                     (<a href=\"#{}\">Accounting</a> lists them).",
                    plural(unseen, "rule", "rules"),
                    scope.id("accounting")
                );
            }
        }
    }
    out.push_str("</p>\n");
}

/// A run's hero: how many of its traces need a look, and where to start.
pub(super) fn run(
    out: &mut String,
    traces: &[(Trace<'_>, &[Report])],
    unjudged: &[Unjudged],
    overall: Verdict,
) {
    let total = traces.len() + unjudged.len();
    let clean = traces
        .iter()
        .filter(|(_, reports)| trace_verdict(reports) == Verdict::Pass)
        .count();
    let headline = if total == 0 {
        "No recording was given, so nothing was judged.".to_owned()
    } else if clean == total {
        format!("No rule was broken in any of the {total} recordings.")
    } else {
        format!(
            "{} of {total} recordings {} something to look at.",
            total - clean,
            if total - clean == 1 { "has" } else { "have" }
        )
    };
    open(out, overall, &headline);
    out.push_str("<p class=\"next\"><strong>What to do next</strong> ");
    out.push_str(if unjudged.is_empty() {
        "Open each recording below that is not marked Pass; each has its own summary and its own \
         next step."
    } else {
        "First fix the recording that could not be read (its reason is in the list), then open \
         each one below that is not marked Pass."
    });
    out.push_str("</p>\n");
    let mut why = "the most severe verdict of the run".to_owned();
    if !unjudged.is_empty() {
        why.push_str(", which left a trace unjudged");
    }
    close(out, &why);
}
