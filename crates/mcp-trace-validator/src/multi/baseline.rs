// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A multi-revision report with a baseline applied
//! ([`crate::baseline`], ADR-0025).
//!
//! The baseline is applied to the per-revision [`Report`]s, where each
//! finding has its fingerprint; the aligned rows here are copies, so they
//! adopt the result rather than recompute it.

use crate::report::{BaselineTally, Outcome, Report};

use super::MultiReport;

impl MultiReport {
    /// Takes the baseline marks of `reports` — the per-revision reports this
    /// report was aligned from, a baseline since applied to them — into its
    /// rows and summaries, and recomputes each revision's verdict as its
    /// report's [`effective_verdict`](Report::effective_verdict): the party
    /// view and the baseline together.
    pub fn adopt_baseline(&mut self, reports: &[Report]) {
        self.baseline_applied = true;
        for row in &mut self.requirements {
            for (index, report) in reports.iter().enumerate() {
                let found = report.requirements.iter().find(|other| other.id == row.id);
                if let (Some(found), Some(slot)) = (found, row.findings.get_mut(index)) {
                    slot.clone_from(&found.findings);
                }
            }
        }
        for (summary, report) in self.summaries.iter_mut().zip(reports) {
            summary.wire_schema.clone_from(&report.wire_schema);
            summary.verdict = report.effective_verdict();
        }
    }

    /// How the findings of every revision — the rows' and the wire schemas' —
    /// fall under the baseline applied to this report.
    #[must_use]
    pub fn baseline_tally(&self) -> BaselineTally {
        let mut tally = BaselineTally::default();
        for index in 0..self.summaries.len() {
            let rows = self
                .requirements
                .iter()
                .filter(|row| {
                    matches!(
                        row.outcomes.get(index).copied().flatten(),
                        Some(Outcome::Fail | Outcome::Warn)
                    )
                })
                .filter_map(|row| row.findings.get(index))
                .flatten();
            let schema = self.summaries[index]
                .wire_schema
                .iter()
                .flat_map(|schema| &schema.findings);
            tally = tally.plus(BaselineTally::of(rows.chain(schema)));
        }
        tally
    }
}
