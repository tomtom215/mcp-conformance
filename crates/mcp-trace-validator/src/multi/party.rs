// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A multi-revision report viewed for one party: the rule
//! [`Report::verdict_for`](crate::report::Report::verdict_for) applies, read
//! off the aligned rows rather than off one revision's report.

use crate::report::{Finding, Outcome, Party, PartyTally, Verdict};

use super::MultiReport;

impl MultiReport {
    /// Views the report for `party`: only findings held against it, and those
    /// held against nobody, count toward each revision's verdict and so
    /// toward [`Self::verdict`]. The rows keep every finding with its party.
    pub fn view_party(&mut self, party: Party) {
        self.party_view = Some(party);
        for index in 0..self.summaries.len() {
            self.summaries[index].verdict = self.verdict_for(index, party);
        }
    }

    /// The verdict of the revision at `index`, counting only the findings held
    /// against `party` and those held against nobody.
    fn verdict_for(&self, index: usize, party: Party) -> Verdict {
        let summary = &self.summaries[index];
        if summary.totals.unsupported > 0 {
            return Verdict::Unsupported;
        }
        let counts =
            |findings: &[Finding]| findings.iter().any(|finding| finding.counts_for(party));
        let mut failed = summary
            .wire_schema
            .as_ref()
            .is_some_and(|schema| schema.outcome == Outcome::Fail && counts(&schema.findings));
        let mut warned = false;
        for row in &self.requirements {
            let findings = row.findings.get(index).map_or(&[][..], Vec::as_slice);
            match row.outcomes.get(index).copied().flatten() {
                Some(Outcome::Fail) if counts(findings) => failed = true,
                Some(Outcome::Warn) if counts(findings) => warned = true,
                _ => {}
            }
        }
        if failed {
            Verdict::Fail
        } else if warned {
            Verdict::PassWithWarnings
        } else {
            Verdict::Pass
        }
    }

    /// How the findings at the revision at `index` fall under `party`'s view.
    pub(super) fn party_tally(&self, index: usize, party: Party) -> PartyTally {
        let rows = self
            .requirements
            .iter()
            .filter_map(|row| row.findings.get(index))
            .flatten();
        let schema = self
            .summaries
            .get(index)
            .and_then(|summary| summary.wire_schema.as_ref())
            .into_iter()
            .flat_map(|schema| &schema.findings);
        PartyTally::of(rows.chain(schema), party)
    }
}
