// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The terminal-oriented rendering of a [`Report`].
//!
//! Split from the data model so the report's shape and the way it is printed
//! can be read separately: everything here is presentation, and nothing here
//! decides an outcome.

use core::fmt::Write as _;

use super::{Outcome, Report};

impl Report {
    /// Renders the human-readable form: every requirement's row.
    #[must_use]
    pub fn render_human(&self) -> String {
        self.render(false)
    }

    /// Renders only what needs attention — failing, warning and unsupported rows —
    /// with the header, totals and verdict. The totals still account for every
    /// clause, so nothing is hidden from the arithmetic.
    #[must_use]
    pub fn render_findings(&self) -> String {
        self.render(true)
    }

    fn render(&self, findings_only: bool) -> String {
        let mut out = String::new();
        match self.revision_source {
            Some(source) => {
                let _ = writeln!(
                    out,
                    "MCP trace validation — revision {} ({source})",
                    self.revision
                );
            }
            None => {
                let _ = writeln!(out, "MCP trace validation — revision {}", self.revision);
            }
        }
        self.write_revision_mismatch(&mut out);
        for row in &self.requirements {
            if findings_only && !row.outcome.needs_attention() {
                continue;
            }
            write_row(&mut out, row);
        }
        // Every outcome is named, so the counts sum to the registry's size — a
        // reader can check the arithmetic, and `Totals`' own exhaustive
        // destructuring is what keeps that true as outcomes are added.
        let _ = writeln!(out, "totals: {}", self.totals);
        let _ = writeln!(out, "verdict: {}", self.verdict());
        self.write_revision_mismatch(&mut out);
        out
    }

    /// Writes the revision-disagreement note, if there is one.
    ///
    /// Rendered twice — under the header and under the verdict — because both
    /// are where a reader looks, and a note that scrolls past a hundred rows of
    /// findings is a note nobody reads. It is short enough that repeating it
    /// costs less than missing it.
    fn write_revision_mismatch(&self, out: &mut String) {
        let Some(declared) = &self.revision_mismatch else {
            return;
        };
        let subject = if declared.len() == 1 {
            "revision"
        } else {
            "revisions"
        };
        let suggestion = declared.last().map_or("<revision>", String::as_str);
        let _ = writeln!(
            out,
            "  NOTE  this session declares protocol {subject} {}, not {}.",
            declared.join(", "),
            self.revision
        );
        let _ = writeln!(
            out,
            "        Every outcome here judges it against rules it was not playing by;"
        );
        let _ = writeln!(
            out,
            "        re-run with `--revision {suggestion}` to judge it against its own."
        );
    }
}

/// One requirement's row and its detail lines.
fn write_row(out: &mut String, row: &super::RequirementReport) {
    let marker = match row.outcome {
        Outcome::Pass => "PASS",
        Outcome::Fail => "FAIL",
        Outcome::Warn => "WARN",
        Outcome::Excluded => "EXCL",
        Outcome::Unsupported => "UNSUP",
        Outcome::NotApplicable => "N/A",
        Outcome::NotObserved => "NOBS",
    };
    let _ = writeln!(out, "  {marker:<5} {} ({})", row.id, row.level);
    for finding in &row.findings {
        match finding.seq {
            Some(seq) => {
                let _ = writeln!(out, "        seq {seq}: {}", finding.detail);
            }
            None => {
                let _ = writeln!(out, "        {}", finding.detail);
            }
        }
    }
    if let Some(exclusion) = &row.exclusion {
        let _ = writeln!(out, "        excluded: {exclusion}");
    }
    for check in &row.missing_checks {
        let _ = writeln!(out, "        unsupported check: {check}");
    }
    if let Some(capability) = &row.capability {
        let _ = writeln!(
            out,
            "        not applicable: capability {capability} was not declared in this session"
        );
    }
    if row.outcome == Outcome::NotObserved {
        let _ = writeln!(
            out,
            "        not observed: the session carried none of the traffic this clause binds to"
        );
    }
}
