// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `spec-drift --draft` report, as Markdown: a summary table a reader can
//! take in at a glance, then each finding with what to do about it.

use std::fmt::Write as _;

use super::compare::Status;
use super::{BASE, Report};

/// How many findings each kind of quote has.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Totals {
    pub present: usize,
    pub changed: usize,
    pub recorded: usize,
    pub removed: usize,
    pub page_removed: usize,
}

/// Counts the report's quote findings by status.
pub(super) fn totals(report: &Report<'_>) -> Totals {
    let mut totals = Totals::default();
    for finding in &report.findings {
        match finding.status {
            Status::Present => totals.present += 1,
            Status::Changed { .. } => totals.changed += 1,
            Status::Removed { .. } => totals.removed += 1,
            Status::PageRemoved => totals.page_removed += 1,
        }
        if finding.recorded && !matches!(finding.status, Status::Present) {
            totals.recorded += 1;
        }
    }
    totals
}

/// Renders the whole report. `writeln!` into a `String` cannot fail, so its
/// results are discarded.
pub(super) fn markdown(report: &Report<'_>) -> String {
    let mut out = String::new();
    let totals = totals(report);
    let live = report.live_census.values();
    let (must, should, may) = live.fold((0, 0, 0), |(must, should, may), tally| {
        (must + tally.must, should + tally.should, may + tally.may)
    });
    let _ = writeln!(out, "## Draft tracking: `draft` against `{BASE}`\n");
    let _ = writeln!(
        out,
        "Informational (`cargo xtask spec-drift --draft`): what the next revision has \
         changed so far, read from the published `draft/` text. Nothing here affects a \
         verdict.\n"
    );
    let _ = writeln!(out, "| Measure | Value |\n|---|---|");
    let _ = writeln!(
        out,
        "| Pages the draft publishes | {} ({} new, {} gone, against the skeleton) |",
        report.published,
        report.pages.added.len(),
        report.pages.removed.len()
    );
    let _ = writeln!(
        out,
        "| Keywords on in-scope draft pages | {must} MUST-family, {should} SHOULD-family, \
         {may} MAY-family across {} page(s) |",
        report.live_census.len()
    );
    let _ = writeln!(
        out,
        "| Census moves against the skeleton | {} |",
        report.census_moves.len()
    );
    let _ = writeln!(
        out,
        "| MUST/SHOULD-family keywords no `{BASE}` quote covers | {} |",
        report.unentered.len()
    );
    let _ = writeln!(
        out,
        "| `{BASE}` quotes | {} checked: {} present, {} changed ({} recorded), {} removed, \
         {} on a page the draft dropped |",
        report.findings.len(),
        totals.present,
        totals.changed,
        totals.recorded,
        totals.removed,
        totals.page_removed
    );
    let _ = writeln!(
        out,
        "| Skeleton or record problems | {} |\n",
        report.skeleton_problems.len() + report.stale_records.len()
    );
    pages(&mut out, report);
    census(&mut out, report);
    quotes(&mut out, report);
    problems(&mut out, report);
    out
}

fn pages(out: &mut String, report: &Report<'_>) {
    if report.pages.added.is_empty() && report.pages.removed.is_empty() {
        return;
    }
    let _ = writeln!(out, "### Page set\n");
    for page in &report.pages.added {
        let _ = writeln!(
            out,
            "- `{page}`: new in the draft. Read it, then add it to the skeleton's \
             `in_scope` (with its census) or `out_of_scope` (with the reason)."
        );
    }
    for page in &report.pages.removed {
        let _ = writeln!(
            out,
            "- `{page}`: in the skeleton, no longer published by the draft. Retire the row; \
             entries citing it will need a new home when the revision ships."
        );
    }
    out.push('\n');
}

fn census(out: &mut String, report: &Report<'_>) {
    if !report.census_moves.is_empty() {
        let _ = writeln!(
            out,
            "### Census moves\n\nThe skeleton's number, the live draft's, and `{BASE}`'s \
             committed one. Update the skeleton once the move is read.\n"
        );
        let _ = writeln!(
            out,
            "| Page | Family | Skeleton | Draft | `{BASE}` |\n|---|---|---|---|---|"
        );
        for moved in &report.census_moves {
            let base = report.base_census.get(&moved.page).map_or_else(
                || "—".to_owned(),
                |tally| tally.get(moved.family).to_string(),
            );
            let committed = moved
                .committed
                .map_or_else(|| "—".to_owned(), |n| n.to_string());
            let _ = writeln!(
                out,
                "| `{}` | {} | {committed} | {} | {base} |",
                moved.page,
                moved.family.label(),
                moved.live
            );
        }
        out.push('\n');
    }
    if !report.unentered.is_empty() {
        let _ = writeln!(
            out,
            "### Keywords no `{BASE}` quote covers\n\nA clause the next revision adds or \
             rewords. Each will need an entry when the revision ships.\n"
        );
        for line in &report.unentered {
            let _ = writeln!(out, "- {line}");
        }
        out.push('\n');
    }
}

fn quotes(out: &mut String, report: &Report<'_>) {
    let moved: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| !matches!(finding.status, Status::Present))
        .collect();
    if moved.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "### `{BASE}` quotes the draft no longer carries verbatim\n"
    );
    for finding in moved {
        let id = &finding.requirement.id;
        let page = &finding.page;
        let note = if finding.recorded {
            " — recorded in `post-release.json`"
        } else {
            " — **not yet recorded** in `post-release.json`"
        };
        match &finding.status {
            Status::Changed { closest, shared } => {
                let _ = writeln!(
                    out,
                    "- **{id}** changed on `{page}` ({shared}% of its words survive){note}\n  \
                     - released: {}\n  - draft: {closest}",
                    finding.requirement.source.quote
                );
            }
            Status::Removed { shared } => {
                let _ = writeln!(
                    out,
                    "- **{id}** removed from `{page}` (no passage shares more than \
                     {shared}% of its words){note}\n  - released: {}",
                    finding.requirement.source.quote
                );
            }
            Status::PageRemoved => {
                let _ = writeln!(
                    out,
                    "- **{id}**: the draft no longer publishes `{page}`{note}"
                );
            }
            Status::Present => {}
        }
    }
    out.push('\n');
}

fn problems(out: &mut String, report: &Report<'_>) {
    if !report.skeleton_problems.is_empty() || !report.stale_records.is_empty() {
        let _ = writeln!(out, "### Skeleton and record problems\n");
        for problem in &report.skeleton_problems {
            let _ = writeln!(out, "- {problem}");
        }
        for id in &report.stale_records {
            let _ = writeln!(
                out,
                "- {id}: recorded in `post-release.json`, but the draft no longer bears the \
                 row out (its `draft` wording is gone, or the released wording is back). \
                 Re-read the page and update or retire the row."
            );
        }
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "<details><summary>Draft pages read ({}), by content fingerprint</summary>\n",
        report.fingerprints.len()
    );
    for (page, fingerprint) in &report.fingerprints {
        let _ = writeln!(out, "- `{page}` {fingerprint}");
    }
    let _ = writeln!(out, "\n</details>");
}
