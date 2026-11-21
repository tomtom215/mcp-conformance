// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The terminal rendering of a [`MultiReport`]: one row per clause with its
//! outcome under each revision, then each revision's findings, totals and
//! verdict.

use core::fmt::Write as _;

use super::{MultiReport, MultiRow};
use crate::report::{Filter, HumanOptions, Outcome};

impl MultiReport {
    /// Renders the human-readable form: every clause's row.
    #[must_use]
    pub fn render_human(&self) -> String {
        self.render_with(&HumanOptions::default())
    }

    /// Renders only the clauses that need attention under some revision, with the
    /// per-revision totals and the verdict.
    #[must_use]
    pub fn render_findings(&self) -> String {
        self.render_with(&HumanOptions::default().all(false))
    }

    /// Renders the human-readable form as `options` lay it out.
    #[must_use]
    pub fn render_with(&self, options: &HumanOptions) -> String {
        self.render_with_filter(options, &Filter::default())
    }

    /// Renders the human-readable form as `options` lay it out, listing only
    /// the clauses `filter` selects; the totals, the verdicts and the other
    /// blocks are the report's ([`Report::render_with_filter`](crate::report::Report::render_with_filter)).
    #[must_use]
    pub fn render_with_filter(&self, options: &HumanOptions, filter: &Filter) -> String {
        let hidden: usize = self
            .requirements
            .iter()
            .filter(|row| !filter.matches(&row.id, &row.level))
            .map(|row| row.findings.iter().map(Vec::len).sum::<usize>())
            .sum();
        let mut out = String::new();
        self.write_head(&mut out);
        self.write_revision_mismatch(&mut out);
        if !filter.is_empty() {
            out.push_str(&filter.note(hidden));
        }
        for row in &self.requirements {
            if !filter.matches(&row.id, &row.level) {
                continue;
            }
            if !options.all
                && !row
                    .outcomes
                    .iter()
                    .flatten()
                    .any(|outcome| outcome.needs_attention())
            {
                continue;
            }
            self.write_row(&mut out, row, options);
        }
        for summary in &self.summaries {
            if let Some(wire_schema) = &summary.wire_schema {
                wire_schema.write_human(
                    &mut out,
                    options,
                    Some(&summary.revision),
                    self.party_view,
                );
            }
            for extension in &summary.extensions {
                extension.write_human(&mut out, options, Some(&summary.revision), self.party_view);
            }
        }
        if let Some(unanswered) = &self.unanswered_requests {
            unanswered.write_human(&mut out);
        }
        if !filter.is_empty() {
            out.push_str(&filter.note(hidden));
        }
        self.write_summaries(&mut out);
        if self.baseline_applied {
            let _ = writeln!(out, "baseline: {}", self.baseline_tally().describe());
        }
        let label =
            HumanOptions::verdict_label("overall verdict", self.party_view, self.baseline_applied);
        let _ = writeln!(out, "{label}: {}", self.verdict());
        self.write_revision_mismatch(&mut out);
        out
    }

    /// The title line: the revisions judged and how they were chosen.
    fn write_head(&self, out: &mut String) {
        let _ = write!(
            out,
            "MCP multi-revision validation — revisions {}",
            self.revisions.join(", ")
        );
        match self.revision_source {
            Some(source) => {
                let _ = writeln!(out, " ({source})");
            }
            None => {
                let _ = writeln!(out);
            }
        }
    }

    /// The `per revision:` block: each revision's counts, verdict (and, under
    /// a party view, what the view set aside) and wire-schema line.
    fn write_summaries(&self, out: &mut String) {
        let _ = writeln!(out, "per revision:");
        for (index, summary) in self.summaries.iter().enumerate() {
            // The same phrase the single-revision report prints, from the same
            // source: this line used to name six of the seven outcomes, so a
            // reader adding it up found fewer clauses than the revision has.
            let _ = write!(
                out,
                "  {}: {} — verdict {}",
                summary.revision, summary.totals, summary.verdict
            );
            let mut views = Vec::new();
            if let Some(party) = self.party_view {
                let what = self.party_tally(index, party).describe(party);
                views.push(if what.is_empty() {
                    format!("{party} findings only")
                } else {
                    format!("{party} findings only: {what}")
                });
            }
            if self.baseline_applied {
                views.push("baseline applied".to_owned());
            }
            if views.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, " ({})", views.join(", "));
            }
            if let Some(wire_schema) = &summary.wire_schema {
                let _ = writeln!(
                    out,
                    "  {}: wire schema {}",
                    summary.revision,
                    wire_schema.summary()
                );
            }
        }
    }

    /// One clause's row: its cell per revision, then each revision's findings and
    /// the clause they break.
    fn write_row(&self, out: &mut String, row: &MultiRow, options: &HumanOptions) {
        let _ = write!(out, "  {:<10} ({})", row.id, row.level);
        for (revision, outcome) in self.revisions.iter().zip(&row.outcomes) {
            let _ = write!(out, "  {revision}={}", cell_token(*outcome));
        }
        if row.differs() {
            let _ = write!(out, "  *differs");
        }
        let _ = writeln!(out);
        for (index, (revision, findings)) in self.revisions.iter().zip(&row.findings).enumerate() {
            options.write_findings(out, findings, Some(revision), self.party_view);
            if let Some(Some(source)) = row.sources.get(index) {
                source.write_human(out);
            }
        }
    }

    /// The revision-disagreement note, worded for a run that names its own
    /// revisions: the fix is a different `--revision`, not the absence of one.
    fn write_revision_mismatch(&self, out: &mut String) {
        let Some(declared) = &self.revision_mismatch else {
            return;
        };
        let (subject, tail) = if declared.len() == 1 {
            ("revision", "which was not judged")
        } else {
            ("revisions", "none of which was judged")
        };
        let _ = writeln!(
            out,
            "  NOTE  this session declares protocol {subject} {}, {tail}.",
            declared.join(", ")
        );
        let _ = writeln!(
            out,
            "        Every outcome here judges it against rules it was not playing by."
        );
    }
}

/// The per-cell token for a clause's outcome under one revision. Exhaustive on purpose
/// (same-crate enum): a new [`Outcome`] variant must force a deliberate token here.
const fn cell_token(outcome: Option<Outcome>) -> &'static str {
    match outcome {
        None => "absent",
        Some(Outcome::Pass) => "pass",
        Some(Outcome::Fail) => "fail",
        Some(Outcome::Warn) => "warn",
        Some(Outcome::Excluded) => "excluded",
        Some(Outcome::Unsupported) => "unsupported",
        Some(Outcome::NotApplicable) => "not-applicable",
        Some(Outcome::NotObserved) => "not-observed",
    }
}
