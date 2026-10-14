// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The extensions a session exercised (ADR-0023): each judged against its own
//! clauses and shown in full — its verdict, the clauses to act on, the rest
//! accounted for — in a section of its own, after the verdict it never moves.

use core::fmt::Write as _;

use super::Scope;
use super::clauses::{self, open_section};
use super::escape::Text;
use super::summary::{verdict_class, verdict_word};

/// The extensions section; nothing when no report carries an extension.
pub(super) fn write(out: &mut String, scope: &Scope<'_>) {
    if scope
        .reports
        .iter()
        .all(|report| report.extensions.is_empty())
    {
        return;
    }
    open_section(out, scope, "extensions", "Extensions — not scored");
    out.push_str(
        "<p class=\"lede\">An extension is optional by definition, so conforming to the protocol \
         cannot depend on one. Each extension this session used is judged against its own \
         clauses here and never changes the verdict above — the line the official suite draws \
         when it runs an extension's scenarios under <code>not_scored: extension</code>.</p>\n",
    );
    for report in scope
        .reports
        .iter()
        .filter(|report| !report.extensions.is_empty())
    {
        let level = clauses::revision_heading(out, scope, report);
        for extension in &report.extensions {
            let verdict = extension.verdict();
            let _ = writeln!(
                out,
                "<h{level} class=\"extension-id\"><code>{}</code> <span class=\"badge v-{}\">{}</span></h{level}>\n\
                 <p class=\"meaning\">{}</p>",
                Text(&extension.extension),
                verdict_class(verdict),
                verdict_word(verdict),
                Text(&extension.totals.to_string())
            );
            let mut acted = false;
            for row in extension
                .requirements
                .iter()
                .filter(|row| row.outcome.needs_attention())
            {
                clauses::clause(out, scope, report, row, level + 1);
                acted = true;
            }
            if !acted {
                out.push_str(
                    "<p class=\"empty\">No clause of the extension failed, warned or went \
                     unjudged.</p>\n",
                );
            }
            clauses::groups(out, scope, report, &extension.requirements);
        }
    }
    out.push_str("</section>\n");
}
