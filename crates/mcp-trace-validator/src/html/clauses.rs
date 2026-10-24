// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The clauses: those a reader acts on, in full, and the accounting of the
//! rest, collapsed.

use core::fmt::Write as _;

use mcp_conformance_core::requirement::Actor;

use super::Scope;
use super::escape::{Text, href};
use crate::report::{
    ClauseSource, Finding, Outcome, Party, Report, RequirementReport, WireSchemaReport,
};

/// The actor pill's text: who the clause binds.
pub(super) const fn binds(actor: Actor) -> &'static str {
    match actor {
        Actor::Client => "binds client",
        Actor::Server => "binds server",
        Actor::Both => "binds both",
        // Foreign #[non_exhaustive] enum: a future party surfaces visibly.
        _ => "binds an unknown party",
    }
}

/// The party pill beside a finding: the party it is held against, or, under a
/// party `view`, `unattributed` for one held against nobody (the view counts
/// it, and the reader should see that it was counted rather than attributed).
pub(super) fn party_pill(out: &mut String, finding: &Finding, view: Option<Party>) {
    match (finding.party, view) {
        (Some(party), _) => {
            let _ = write!(out, "<span class=\"pill party\">{party}</span>");
        }
        (None, Some(_)) => out.push_str("<span class=\"pill party none\">unattributed</span>"),
        (None, None) => {}
    }
}

/// The anchor name of the wire schema's row, beside the requirement IDs.
pub(super) const WIRE_SCHEMA: &str = "wire-schema";

/// An outcome's report token, which is also its style class.
pub(super) const fn token(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Pass => "pass",
        Outcome::Fail => "fail",
        Outcome::Warn => "warn",
        Outcome::Excluded => "excluded",
        Outcome::Unsupported => "unsupported",
        Outcome::NotApplicable => "not-applicable",
        Outcome::NotObserved => "not-observed",
    }
}

/// Opens a section of the trace: its anchor and heading.
pub(super) fn open_section(out: &mut String, scope: &Scope<'_>, name: &str, title: &str) {
    let id = scope.id(name);
    let _ = writeln!(
        out,
        "<section class=\"section\" id=\"{id}\" aria-labelledby=\"{id}-h\">\n\
         <h{level} id=\"{id}-h\">{title}</h{level}>",
        level = scope.level
    );
}

/// Under a several-revision page, the heading of one revision's part.
pub(super) fn revision_heading(out: &mut String, scope: &Scope<'_>, report: &Report) -> u8 {
    if scope.multi() {
        let _ = writeln!(
            out,
            "<h{level} class=\"revision-heading\">At revision {}</h{level}>",
            Text(&report.revision),
            level = scope.level + 1
        );
        scope.level + 2
    } else {
        scope.level + 1
    }
}

/// Every failing, warning and unsupported clause, and a failed wire schema.
pub(super) fn attention(out: &mut String, scope: &Scope<'_>) {
    open_section(out, scope, "findings", "Clauses to act on");
    for report in scope.reports {
        let level = revision_heading(out, scope, report);
        let rows: Vec<&RequirementReport> = report
            .requirements
            .iter()
            .filter(|row| row.outcome.needs_attention())
            .collect();
        let schema = report
            .wire_schema
            .as_ref()
            .filter(|s| s.outcome == Outcome::Fail);
        if rows.is_empty() && schema.is_none() {
            let _ = writeln!(
                out,
                "<p class=\"empty\">No clause failed, warned or went unjudged at {}.</p>",
                Text(&report.revision)
            );
        }
        for row in rows {
            clause(out, scope, report, row, level);
        }
        if let Some(schema) = schema {
            wire_schema(out, scope, report, schema, level);
        }
    }
    out.push_str("</section>\n");
}

/// The header of a clause's card: its anchor, ID and pills.
fn card_head(out: &mut String, anchor: &str, title: &str, pills: &[(&str, &str)], level: u8) {
    let _ = write!(
        out,
        "<header class=\"clause-head\"><h{level} class=\"clause-id\"><a href=\"#{anchor}\">{}</a></h{level}>",
        Text(title)
    );
    for (class, text) in pills {
        let _ = write!(out, "<span class=\"pill {class}\">{}</span>", Text(text));
    }
    out.push_str("</header>\n");
}

pub(super) fn clause(
    out: &mut String,
    scope: &Scope<'_>,
    report: &Report,
    row: &RequirementReport,
    level: u8,
) {
    let anchor = scope.clause(&report.revision, &row.id);
    let outcome = token(row.outcome);
    let _ = writeln!(
        out,
        "<article class=\"clause o-{outcome}\" id=\"{anchor}\">"
    );
    let mut pills = vec![
        (format!("o-{outcome}"), outcome),
        ("level".to_owned(), &row.level),
    ];
    if let Some(actor) = row.actor {
        pills.push(("actor".to_owned(), binds(actor)));
    }
    let pills: Vec<(&str, &str)> = pills
        .iter()
        .map(|(class, text)| (class.as_str(), *text))
        .collect();
    card_head(out, &anchor, &row.id, &pills, level);
    if let Some(source) = &row.source {
        quote(out, source);
    }
    if !row.missing_checks.is_empty() {
        out.push_str(
            "<p class=\"missing\">This build does not implement the checks this clause names:",
        );
        for check in &row.missing_checks {
            let _ = write!(out, " <code>{}</code>", Text(check));
        }
        out.push_str(".</p>\n");
    }
    findings(out, scope, &anchor, &row.findings, report.party_view);
    out.push_str("</article>\n");
}

fn wire_schema(
    out: &mut String,
    scope: &Scope<'_>,
    report: &Report,
    schema: &WireSchemaReport,
    level: u8,
) {
    let anchor = scope.clause(&report.revision, WIRE_SCHEMA);
    let _ = writeln!(out, "<article class=\"clause o-fail\" id=\"{anchor}\">");
    card_head(
        out,
        &anchor,
        "Wire schema",
        &[("o-fail", "fail"), ("level", "normative")],
        level,
    );
    quote(out, &schema.source);
    let _ = write!(
        out,
        "<p class=\"schema\">{} of {} messages are invalid against the revision's vendored \
         schema: ",
        schema.findings.len(),
        schema.messages,
    );
    match href(&schema.schema) {
        Some(url) => {
            let _ = writeln!(
                out,
                "<a href=\"{}\" rel=\"noreferrer\"><code>{}</code></a></p>",
                Text(url),
                Text(url)
            );
        }
        None => {
            let _ = writeln!(out, "<code>{}</code></p>", Text(&schema.schema));
        }
    }
    findings(out, scope, &anchor, &schema.findings, report.party_view);
    out.push_str("</article>\n");
}

/// The clause, verbatim, and where it is published.
fn quote(out: &mut String, source: &ClauseSource) {
    let _ = write!(
        out,
        "<blockquote class=\"quote\"><p>{}</p></blockquote>\n<p class=\"source\">\
         <span class=\"section-path\">{}</span>",
        Text(&source.quote),
        Text(&source.section)
    );
    match href(&source.url) {
        Some(url) => {
            let _ = writeln!(
                out,
                "<a class=\"spec-link\" href=\"{}\" rel=\"noreferrer\">Read it in the specification</a></p>",
                Text(url)
            );
        }
        None => {
            let _ = writeln!(
                out,
                "<span class=\"spec-url\">{}</span></p>",
                Text(&source.url)
            );
        }
    }
}

/// A clause's findings, each linking to its event (which links back), with the
/// party each is held against.
fn findings(
    out: &mut String,
    scope: &Scope<'_>,
    anchor: &str,
    findings: &[Finding],
    view: Option<Party>,
) {
    if findings.is_empty() {
        return;
    }
    out.push_str("<ol class=\"findings\">\n");
    for (index, finding) in findings.iter().enumerate() {
        let _ = write!(out, "<li id=\"{anchor}-{}\">", index + 1);
        match finding.seq {
            Some(seq) => {
                let _ = write!(
                    out,
                    "<a class=\"seq\" href=\"#{}\">seq {seq}</a>",
                    scope.event(seq)
                );
            }
            None => out.push_str("<span class=\"seq none\">no single event</span>"),
        }
        out.push_str("<span class=\"detail\">");
        party_pill(out, finding, view);
        let _ = writeln!(
            out,
            "{}</span><span class=\"check\">{}</span></li>",
            Text(&finding.detail),
            Text(&finding.check)
        );
    }
    out.push_str("</ol>\n");
}

/// A group of the accounting: its outcome, what that outcome means, and how
/// each row shows its reason.
struct Group {
    outcome: Outcome,
    title: &'static str,
    meaning: &'static str,
}

const GROUPS: [Group; 4] = [
    Group {
        outcome: Outcome::Pass,
        title: "Passed",
        meaning: "Every covering check ran over traffic the clause binds to and found nothing wrong.",
    },
    Group {
        outcome: Outcome::NotObserved,
        title: "Not observed",
        meaning: "Every covering check ran and found nothing to judge: the session carried none of \
                  the traffic these clauses bind to. A session that exercises them may pass or \
                  fail them.",
    },
    Group {
        outcome: Outcome::NotApplicable,
        title: "Not applicable",
        meaning: "Gated on a capability this session never declared, so their checks did not run.",
    },
    Group {
        outcome: Outcome::Excluded,
        title: "Excluded",
        meaning: "The registry does not judge these from a recording; each row says why.",
    },
];

/// The clauses with nothing to act on, grouped by outcome and collapsed, so
/// every clause of the revision is accounted for on the page.
pub(super) fn accounting(out: &mut String, scope: &Scope<'_>) {
    open_section(out, scope, "accounting", "Accounting");
    out.push_str(
        "<p class=\"lede\">Every clause of the revision is counted above. These are the ones \
         with nothing to act on, and why.</p>\n",
    );
    for report in scope.reports {
        revision_heading(out, scope, report);
        groups(out, scope, report, &report.requirements);
    }
    out.push_str("</section>\n");
}

/// `rows` with nothing to act on, one collapsed group per outcome, each row
/// with its reason, anchored under `report`'s revision.
pub(super) fn groups(
    out: &mut String,
    scope: &Scope<'_>,
    report: &Report,
    rows: &[RequirementReport],
) {
    for group in &GROUPS {
        let members: Vec<&RequirementReport> = rows
            .iter()
            .filter(|row| row.outcome == group.outcome)
            .collect();
        if members.is_empty() {
            continue;
        }
        let class = token(group.outcome);
        let _ = writeln!(
            out,
            "<details class=\"group g-{class}\"><summary><span class=\"group-name\">{}</span>\
             <span class=\"count\">{}</span></summary>\n<p class=\"meaning\">{}</p>\n<ul class=\"rows\">",
            group.title,
            members.len(),
            group.meaning
        );
        for row in members {
            let _ = write!(
                out,
                "<li id=\"{}\"><code class=\"rid\">{}</code><span class=\"pill level\">{}</span>",
                scope.clause(&report.revision, &row.id),
                Text(&row.id),
                Text(&row.level)
            );
            if let Some(reason) = &row.exclusion {
                let _ = write!(out, "<span class=\"reason\">{}</span>", Text(reason));
            }
            if let Some(capability) = &row.capability {
                let _ = write!(
                    out,
                    "<span class=\"reason\">capability <code>{}</code> was not declared</span>",
                    Text(capability)
                );
            }
            out.push_str("</li>\n");
        }
        out.push_str("</ul>\n</details>\n");
    }
}
