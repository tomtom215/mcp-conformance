// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Multi-revision judgment: one trace against several protocol revisions in a single
//! pass, with per-clause applicability differences made visible.
//!
//! [`validate_revisions`] projects a [`RegistrySet`] to each requested revision, runs the
//! ordinary [`engine::validate`] against each projection, and
//! aligns the results into a [`MultiReport`]: one row per clause in the union, carrying
//! its outcome under every judged revision. A clause that does not exist at a revision
//! (its `applies` range excludes it) reports `None` there — *absent*, which the report
//! keeps distinct from [`Outcome::NotApplicable`] (the clause exists at that revision but
//! a capability gating it was never negotiated, ADR-0006). Seeing both side by side is
//! what makes a migration's gains and losses legible: a clause removed in the newer
//! revision reads `pass` then `absent`; one introduced there reads `absent` then `pass`.
//!
//! Against the two registries this build ships, those are the *only* patterns — see
//! [`MultiRow::differs`] for why, and for what that costs the `*differs` marker.

use core::fmt;
use core::fmt::Write as _;

use mcp_conformance_core::requirement::RegistrySet;
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;
use serde::{Deserialize, Serialize};

use crate::engine;
use crate::report::{Outcome, Report, Totals, Verdict};

/// Error produced by a multi-revision run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum MultiError {
    /// No revisions were requested; there is nothing to judge against.
    NoRevisions,
    /// A requested revision is not one the registry set describes.
    UnknownRevision(ProtocolRevision),
}

impl fmt::Display for MultiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRevisions => f.write_str("no revisions requested for multi-revision judgment"),
            Self::UnknownRevision(revision) => {
                write!(f, "registry set does not describe revision {revision}")
            }
        }
    }
}

impl core::error::Error for MultiError {}

/// One revision's aggregate result within a [`MultiReport`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RevisionSummary {
    /// The protocol revision (`YYYY-MM-DD`).
    pub revision: String,
    /// Aggregate counts for this revision's projected registry.
    pub totals: Totals,
    /// This revision's standalone verdict.
    pub verdict: Verdict,
}

/// One clause's row across every judged revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MultiRow {
    /// The requirement ID (`AREA-NNN`).
    pub id: String,
    /// The requirement's RFC 2119 level, as registry text (`"MUST"`, …).
    pub level: String,
    /// Outcome under each judged revision, aligned by index with
    /// [`MultiReport::revisions`]. `None` means the clause does not exist at that
    /// revision — *absent*, not [`Outcome::NotApplicable`].
    pub outcomes: Vec<Option<Outcome>>,
}

impl MultiRow {
    /// Whether this clause's presence-or-outcome is not uniform across the judged
    /// revisions.
    ///
    /// How much this discriminates depends on the registries judged, and against
    /// the two this build ships it discriminates nothing: the registries are
    /// extracted per revision rather than sharing entries, so a clause restated
    /// with narrower text at the later revision gets its own ID — the reason
    /// `2025-11-25`'s BASE-003 (no reuse within a session) and `2026-07-28`'s
    /// BASE-045 (no reuse *while in flight*) are two clauses and not one. The ID
    /// spaces are therefore disjoint, every row is `absent` on one side, and
    /// `differs` is true for all of them. Read the *pattern* instead: `pass` then
    /// `absent` is a clause the migration removes, `absent` then `pass` one it
    /// adds. A row that differs in outcome while present at both revisions —
    /// the one a review would want first — cannot occur here, and would only
    /// arise for a revision pair that does share clauses.
    #[must_use]
    pub fn differs(&self) -> bool {
        self.outcomes.windows(2).any(|pair| pair[0] != pair[1])
    }
}

/// A multi-revision report: the same trace judged against several revisions, aligned per
/// clause.
///
/// Like [`Report`], it is an artifact — serialization order is fixed (revisions in the
/// order requested; clauses in registry-union order) and nothing environment-dependent
/// appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MultiReport {
    /// The revisions judged, in the order requested — the column order for every row.
    pub revisions: Vec<String>,
    /// Per-revision aggregate results, aligned by index with `revisions`.
    pub summaries: Vec<RevisionSummary>,
    /// Union of clauses across the judged revisions, in registry-union order. A clause is
    /// included when it exists at one or more of the judged revisions.
    pub requirements: Vec<MultiRow>,
}

impl MultiReport {
    /// The overall verdict: the worst across revisions, by the same severity priority a
    /// single [`Report`] uses (unsupported ≻ fail ≻ pass-with-warnings ≻ pass). A
    /// multi-revision run is only as good as its weakest revision.
    #[must_use]
    pub fn verdict(&self) -> Verdict {
        let any = |verdict: Verdict| self.summaries.iter().any(|s| s.verdict == verdict);
        if any(Verdict::Unsupported) {
            Verdict::Unsupported
        } else if any(Verdict::Fail) {
            Verdict::Fail
        } else if any(Verdict::PassWithWarnings) {
            Verdict::PassWithWarnings
        } else {
            Verdict::Pass
        }
    }

    /// Renders the human-readable form.
    #[must_use]
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "MCP multi-revision validation — revisions {}",
            self.revisions.join(", ")
        );
        for row in &self.requirements {
            let _ = write!(out, "  {:<10} ({})", row.id, row.level);
            for (revision, outcome) in self.revisions.iter().zip(&row.outcomes) {
                let _ = write!(out, "  {revision}={}", cell_token(*outcome));
            }
            if row.differs() {
                let _ = write!(out, "  *differs");
            }
            let _ = writeln!(out);
        }
        let _ = writeln!(out, "per revision:");
        for summary in &self.summaries {
            // The same phrase the single-revision report prints, from the same
            // source: this line used to name six of the seven outcomes, so a
            // reader adding it up found fewer clauses than the revision has.
            let _ = writeln!(
                out,
                "  {}: {} — verdict {}",
                summary.revision, summary.totals, summary.verdict
            );
        }
        let _ = writeln!(out, "overall verdict: {}", self.verdict());
        out
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

/// Validates one trace against several protocol revisions in a single pass.
///
/// ```
/// use mcp_conformance_core::requirement::RegistrySet;
/// use mcp_trace_validator::multi;
///
/// // BASE-001 is present throughout; LIFE-009 is removed at 2026-07-28.
/// let set = RegistrySet::from_json(r#"{
///     "revisions": ["2025-11-25", "2026-07-28"],
///     "requirements": [
///         {"id": "BASE-001", "level": "MUST", "actor": "both",
///          "source": {"section": "basic#x", "quote": "MUST jsonrpc 2.0"},
///          "checks": ["base.jsonrpc-version"]},
///         {"id": "LIFE-009", "level": "MUST", "actor": "server",
///          "applies": {"removed": "2026-07-28"},
///          "source": {"section": "life#y", "quote": "MUST jsonrpc 2.0"},
///          "checks": ["base.jsonrpc-version"]}
///     ]
/// }"#)?;
///
/// let revisions = ["2025-11-25".parse()?, "2026-07-28".parse()?];
/// let report = multi::validate_revisions(&set, &revisions, &[])?;
///
/// assert_eq!(report.revisions, ["2025-11-25", "2026-07-28"]);
/// let life = report.requirements.iter().find(|r| r.id == "LIFE-009").unwrap();
/// assert!(life.outcomes[0].is_some()); // present at 2025-11-25
/// assert!(life.outcomes[1].is_none()); // absent at 2026-07-28
/// assert!(life.differs());
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
///
/// # Errors
///
/// [`MultiError::NoRevisions`] when `revisions` is empty, and
/// [`MultiError::UnknownRevision`] when a requested revision is not one `set` describes.
pub fn validate_revisions(
    set: &RegistrySet,
    revisions: &[ProtocolRevision],
    events: &[TraceEvent],
) -> Result<MultiReport, MultiError> {
    if revisions.is_empty() {
        return Err(MultiError::NoRevisions);
    }
    let mut summaries = Vec::with_capacity(revisions.len());
    let mut reports = Vec::with_capacity(revisions.len());
    for &revision in revisions {
        let registry = set
            .registry(revision)
            .ok_or(MultiError::UnknownRevision(revision))?;
        let report = engine::validate(&registry, events);
        summaries.push(RevisionSummary {
            revision: revision.to_string(),
            totals: report.totals,
            verdict: report.verdict(),
        });
        reports.push(report);
    }

    // The union, in registry-union order: walk the set's requirements once and keep each
    // clause that exists at one or more judged revisions. A projected report contains
    // exactly the clauses in force at its revision, so a clause's outcome there is "found
    // in that report" and its absence is "not found" — applicability needs no second
    // source of truth.
    let mut rows = Vec::new();
    for requirement in set.requirements() {
        let id = requirement.id.as_str();
        let outcomes: Vec<Option<Outcome>> = reports
            .iter()
            .map(|report| outcome_in(report, id))
            .collect();
        if outcomes.iter().all(Option::is_none) {
            continue;
        }
        rows.push(MultiRow {
            id: id.to_owned(),
            level: requirement.level.keyword().to_owned(),
            outcomes,
        });
    }

    Ok(MultiReport {
        revisions: revisions.iter().map(ProtocolRevision::to_string).collect(),
        summaries,
        requirements: rows,
    })
}

/// One clause's outcome within a single-revision report, by ID; `None` when the clause is
/// not in that report (it does not exist at that revision).
fn outcome_in(report: &Report, id: &str) -> Option<Outcome> {
    report
        .requirements
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.outcome)
}

#[cfg(test)]
mod tests;
