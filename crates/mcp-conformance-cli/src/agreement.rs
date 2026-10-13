// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The agreement check: the validator's verdicts reconciled with the official
//! suite's, over the very sessions the suite drove.
//!
//! The policy is docs/plan/03-conformance-strategy.md §Calibration: **zero
//! unexplained divergence**. A divergence is a MUST-level validator failure
//! ([`Outcome::Fail`]) on a recorded session. A [`Baseline`] explains known
//! ones: every entry names the requirement, a triage class (`our-bug`,
//! `suite-bug` or `spec-ambiguity`) and an upstream link, and may narrow itself
//! to traces whose name contains a substring. The baseline must also stay live:
//! an entry that explains nothing in the current run is *stale* — the divergence
//! it described no longer occurs — and breaks agreement until it is removed, so
//! an explanation cannot lie in wait for the next failure that matches it.
//!
//! `cargo xtask conformance` gates this repository on it, and
//! `mcp-conformance test` reports it for any subject, with the same code.
//!
//! ```
//! use mcp_conformance_cli::agreement::{Baseline, ValidatorFailure, reconcile};
//!
//! let baseline = Baseline::parse(
//!     r#"{"policy": "…", "divergences": [{"requirement": "LIFE-003",
//!         "class": "suite-bug", "upstream": "https://example.com/issues/1"}]}"#,
//! )?;
//! let failures = [ValidatorFailure::new("002.jsonl", "LIFE-003", "no initialized")];
//! let reconciliation = reconcile(&baseline, &failures);
//! assert_eq!(reconciliation.explained.len(), 1);
//! assert!(reconciliation.holds());
//!
//! // Without the divergence, the entry is stale and agreement no longer holds.
//! assert!(!reconcile(&baseline, &[]).holds());
//! # Ok::<(), String>(())
//! ```

use mcp_trace_validator::report::{Outcome, Report};
use serde::{Deserialize, Serialize};

/// The triage classes the policy admits — nothing else parses.
pub const TRIAGE_CLASSES: [&str; 3] = ["our-bug", "suite-bug", "spec-ambiguity"];

/// One explained divergence. Unknown fields are rejected so a typo (say,
/// `trace_containz`) cannot silently broaden an entry's reach.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ExplainedDivergence {
    /// The registry requirement ID the validator flags.
    pub requirement: String,
    /// Triage class: one of [`TRIAGE_CLASSES`].
    pub class: String,
    /// Upstream issue or pull request explaining the divergence.
    pub upstream: String,
    /// Optional substring filter on the trace's name.
    #[serde(default)]
    pub trace_contains: Option<String>,
    /// Free-text context for reviewers; never consulted.
    #[serde(default, rename = "_note")]
    pub note: Option<String>,
}

impl ExplainedDivergence {
    /// How the entry is named in output: requirement plus filter.
    #[must_use]
    pub fn describe(&self) -> String {
        self.trace_contains.as_ref().map_or_else(
            || self.requirement.clone(),
            |needle| format!("{} (trace_contains {needle:?})", self.requirement),
        )
    }

    /// Whether this entry explains `failure`.
    #[must_use]
    pub fn explains(&self, failure: &ValidatorFailure) -> bool {
        self.requirement == failure.requirement
            && self
                .trace_contains
                .as_ref()
                .is_none_or(|needle| failure.trace.contains(needle))
    }
}

/// A divergence baseline: the policy text and the explained divergences
/// (`conformance/agreement-divergences.json` is this repository's).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Baseline {
    /// The policy text, kept in the file so the contract travels with it.
    pub policy: String,
    /// The explained divergences.
    pub divergences: Vec<ExplainedDivergence>,
}

impl Baseline {
    /// Parses a baseline document and checks every entry: a known triage class
    /// and an `http(s)://` upstream link.
    ///
    /// # Errors
    ///
    /// The document is not a baseline, or an entry breaks the policy; the
    /// message names the entry and what is wrong.
    pub fn parse(text: &str) -> Result<Self, String> {
        let baseline: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        for entry in &baseline.divergences {
            if !TRIAGE_CLASSES.contains(&entry.class.as_str()) {
                return Err(format!(
                    "entry for {} has class {:?}; the policy admits {TRIAGE_CLASSES:?}",
                    entry.requirement, entry.class
                ));
            }
            if !entry.upstream.starts_with("https://") && !entry.upstream.starts_with("http://") {
                return Err(format!(
                    "entry for {} needs an upstream issue/PR URL, got {:?}",
                    entry.requirement, entry.upstream
                ));
            }
        }
        Ok(baseline)
    }
}

/// One MUST-level validator failure on a recorded session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct ValidatorFailure {
    /// The trace's name.
    pub trace: String,
    /// The failing requirement ID.
    pub requirement: String,
    /// The first finding's detail line.
    pub detail: String,
}

impl ValidatorFailure {
    /// A failure of `requirement` on `trace`, described by `detail`.
    #[must_use]
    pub fn new(
        trace: impl Into<String>,
        requirement: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            trace: trace.into(),
            requirement: requirement.into(),
            detail: detail.into(),
        }
    }
}

/// Every failing clause in `report`, as failures of the trace named `trace`.
#[must_use]
pub fn failures(trace: &str, report: &Report) -> Vec<ValidatorFailure> {
    report
        .requirements
        .iter()
        .filter(|row| row.outcome == Outcome::Fail)
        .map(|row| ValidatorFailure {
            trace: trace.to_owned(),
            requirement: row.id.clone(),
            detail: row
                .findings
                .first()
                .map_or_else(String::new, |finding| finding.detail.clone()),
        })
        .collect()
}

/// Outcome counts summed across every judged session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Totals {
    /// Clauses passed.
    pub pass: u32,
    /// Clauses failed.
    pub fail: u32,
    /// Clauses warned (SHOULD-level).
    pub warn: u32,
    /// Clauses the registry declines to judge.
    pub excluded: u32,
    /// Clauses whose check this build lacks.
    pub unsupported: u32,
    /// Clauses gated on a capability nobody declared.
    pub not_applicable: u32,
    /// Clauses the session never exercised.
    pub not_observed: u32,
}

impl Totals {
    /// Adds one report's totals.
    pub const fn add(&mut self, report: &Report) {
        self.pass += report.totals.pass;
        self.fail += report.totals.fail;
        self.warn += report.totals.warn;
        self.excluded += report.totals.excluded;
        self.unsupported += report.totals.unsupported;
        self.not_applicable += report.totals.not_applicable;
        self.not_observed += report.totals.not_observed;
    }
}

/// The failures of a run, split three ways against a baseline.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Reconciliation {
    /// Failures no baseline entry explains.
    pub unexplained: Vec<ValidatorFailure>,
    /// Failures a baseline entry explains.
    pub explained: Vec<ValidatorFailure>,
    /// Baseline entries (as [`ExplainedDivergence::describe`] names them) that
    /// explained nothing in this run.
    #[serde(rename = "stale_baseline_entries")]
    pub stale: Vec<String>,
}

impl Reconciliation {
    /// Whether the suite and the validator agree: no unexplained divergence and
    /// no stale baseline entry.
    #[must_use]
    pub const fn holds(&self) -> bool {
        self.unexplained.is_empty() && self.stale.is_empty()
    }

    /// Why agreement does not hold, with what to do about it, naming
    /// `baseline` (the file the run was reconciled against); `None` when it
    /// holds.
    #[must_use]
    pub fn gate_error(&self, baseline: &str) -> Option<String> {
        let mut sections = Vec::new();
        if !self.unexplained.is_empty() {
            let listing: Vec<String> = self
                .unexplained
                .iter()
                .map(|failure| {
                    format!(
                        "  {} on {}: {}",
                        failure.requirement, failure.trace, failure.detail
                    )
                })
                .collect();
            sections.push(format!(
                "{} unexplained validator-vs-runner divergence(s):\n{}\n\
                 Triage each (our-bug | suite-bug | spec-ambiguity), fix or file \
                 upstream, and record explained ones in {baseline}",
                self.unexplained.len(),
                listing.join("\n")
            ));
        }
        if !self.stale.is_empty() {
            let listing: Vec<String> = self
                .stale
                .iter()
                .map(|entry| format!("  {entry}"))
                .collect();
            sections.push(format!(
                "{} stale baseline entr(y/ies) in {baseline} — each explains a \
                 divergence that no longer occurs:\n{}\n\
                 Remove them in this same change (typically the suite pin bump or \
                 fix that resolved them)",
                self.stale.len(),
                listing.join("\n")
            ));
        }
        (!sections.is_empty()).then(|| sections.join("\n"))
    }
}

/// Splits `failures` into explained and unexplained against `baseline`, and
/// finds the baseline entries that explained nothing.
///
/// Both directions break
/// agreement: an unexplained failure is an undocumented divergence, and a stale
/// entry, left in place, would silently absorb the *next* failure matching its
/// pattern.
#[must_use]
pub fn reconcile(baseline: &Baseline, failures: &[ValidatorFailure]) -> Reconciliation {
    let (explained, unexplained): (Vec<_>, Vec<_>) =
        failures.iter().cloned().partition(|failure| {
            baseline
                .divergences
                .iter()
                .any(|entry| entry.explains(failure))
        });
    let stale = baseline
        .divergences
        .iter()
        .filter(|entry| !failures.iter().any(|failure| entry.explains(failure)))
        .map(ExplainedDivergence::describe)
        .collect();
    Reconciliation {
        unexplained,
        explained,
        stale,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
