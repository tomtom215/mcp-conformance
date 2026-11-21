// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A report with a baseline applied ([`crate::baseline`], ADR-0025): which
//! findings it accepts, how they are counted, and the verdict that follows.
//!
//! The rows and [`Totals`](super::Totals) stay the trace's facts, as under a
//! party view; a finding a baseline accepts keeps its place in the report and
//! carries its [`Acceptance`], and only the verdict stops counting it.

use core::fmt::Write as _;

use serde::{Deserialize, Serialize};

use super::{Finding, Outcome, Report, Verdict};
use crate::baseline::GENERATED_REASON;

/// Why a finding does not count: a baseline entry accepts it.
///
/// Present on a finding only when a baseline was applied and an entry matched
/// it; a finding without one is *not accepted*, whatever else is true of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Acceptance {
    /// The entry's reason, as the baseline file states it.
    pub reason: String,
    /// The last day (`YYYY-MM-DD`, UTC) the entry accepts the finding, when it
    /// has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
}

impl Acceptance {
    /// An acceptance for `reason`, with no expiry.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            expires: None,
        }
    }

    /// Whether the reason is the sentence `--write-baseline` writes
    /// ([`GENERATED_REASON`]): nobody has reviewed this acceptance.
    #[must_use]
    pub fn unreviewed(&self) -> bool {
        self.reason == GENERATED_REASON
    }
}

impl Finding {
    /// Whether a baseline accepts this finding.
    #[must_use]
    pub const fn is_baselined(&self) -> bool {
        self.baselined.is_some()
    }
}

/// How the findings that count toward a verdict fall under a baseline.
///
/// Counts findings, not clauses: a row with three findings, two accepted, is
/// two accepted and one not. An extension's findings are not counted here, as
/// they are not counted toward any verdict (ADR-0023).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct BaselineTally {
    /// Findings a baseline entry accepts.
    pub accepted: u32,
    /// Findings no entry accepts: the ones a verdict counts (less any a party
    /// view sets aside).
    pub unaccepted: u32,
    /// Of the accepted, those whose reason is the one `--write-baseline`
    /// writes, which nobody has replaced with a judgment.
    pub unreviewed: u32,
}

impl BaselineTally {
    /// The tally of `findings`.
    pub(crate) fn of<'f>(findings: impl Iterator<Item = &'f Finding>) -> Self {
        let mut tally = Self::default();
        for finding in findings {
            match &finding.baselined {
                Some(acceptance) => {
                    tally.accepted += 1;
                    tally.unreviewed += u32::from(acceptance.unreviewed());
                }
                None => tally.unaccepted += 1,
            }
        }
        tally
    }

    /// The sum of two tallies, for a run of several reports.
    #[must_use]
    pub const fn plus(self, other: Self) -> Self {
        Self {
            accepted: self.accepted + other.accepted,
            unaccepted: self.unaccepted + other.unaccepted,
            unreviewed: self.unreviewed + other.unreviewed,
        }
    }

    /// The tally as one phrase for a report's `baseline:` line:
    /// `8 findings accepted, 0 not accepted`, then what is unreviewed.
    #[must_use]
    pub fn describe(&self) -> String {
        let plural = |count: u32| if count == 1 { "finding" } else { "findings" };
        let mut text = format!(
            "{} {} accepted, {} not accepted",
            self.accepted,
            plural(self.accepted),
            self.unaccepted
        );
        if self.unreviewed > 0 {
            let _ = write!(
                text,
                "; {} of the accepted carry the reason --write-baseline wrote, which nobody has reviewed",
                self.unreviewed
            );
        }
        text
    }
}

impl Report {
    /// Whether `finding` counts toward [`Self::effective_verdict`]: a baseline
    /// does not accept it, and the party view, if any, does not set it aside.
    #[must_use]
    pub fn counts(&self, finding: &Finding) -> bool {
        !finding.is_baselined()
            && self
                .party_view
                .is_none_or(|party| finding.counts_for(party))
    }

    /// The verdict counting only the findings `counts` approves: unsupported
    /// over fail over warnings over pass, where a `fail` or `warn` row — and a
    /// failed wire schema — counts only through a finding that counts.
    pub(super) fn verdict_counting(&self, counts: impl Fn(&Finding) -> bool) -> Verdict {
        if self.totals.unsupported > 0 {
            return Verdict::Unsupported;
        }
        let any = |findings: &[Finding]| findings.iter().any(&counts);
        let mut failed = self
            .wire_schema
            .as_ref()
            .is_some_and(|schema| schema.outcome == Outcome::Fail && any(&schema.findings));
        let mut warned = false;
        for row in &self.requirements {
            match row.outcome {
                Outcome::Fail if any(&row.findings) => failed = true,
                Outcome::Warn if any(&row.findings) => warned = true,
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

    /// How this report's findings — the rows' and the wire schema's — fall
    /// under the baseline applied to it, whatever the party view.
    #[must_use]
    pub fn baseline_tally(&self) -> BaselineTally {
        let rows = self
            .requirements
            .iter()
            .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
            .flat_map(|row| &row.findings);
        let schema = self.wire_schema.iter().flat_map(|schema| &schema.findings);
        BaselineTally::of(rows.chain(schema))
    }
}
