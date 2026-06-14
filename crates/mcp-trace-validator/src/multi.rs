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
    /// revisions — the rows a migration review wants to look at first.
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
            let totals = summary.totals;
            let _ = writeln!(
                out,
                "  {}: {} pass, {} fail, {} warn, {} excluded, {} unsupported, {} not applicable — verdict {}",
                summary.revision,
                totals.pass,
                totals.fail,
                totals.warn,
                totals.excluded,
                totals.unsupported,
                totals.not_applicable,
                summary.verdict
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::reader::{Limits, parse_trace};

    /// A two-revision set: BASE-001 throughout, LIFE-009 removed at 2026-07-28, DISC-001
    /// introduced at 2026-07-28. All use a real check so outcomes are meaningful.
    const SET: &str = r#"{
        "revisions": ["2025-11-25", "2026-07-28"],
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "b#x", "quote": "MUST jsonrpc 2.0"},
             "checks": ["base.jsonrpc-version"]},
            {"id": "LIFE-009", "level": "MUST", "actor": "server",
             "applies": {"removed": "2026-07-28"},
             "source": {"section": "l#y", "quote": "MUST jsonrpc 2.0"},
             "checks": ["base.jsonrpc-version"]},
            {"id": "DISC-001", "level": "MUST", "actor": "server",
             "applies": {"introduced": "2026-07-28"},
             "source": {"section": "d#z", "quote": "MUST jsonrpc 2.0"},
             "checks": ["base.jsonrpc-version"]}
        ]
    }"#;

    fn set() -> RegistrySet {
        RegistrySet::from_json(SET).unwrap()
    }

    fn revs() -> [ProtocolRevision; 2] {
        ["2025-11-25".parse().unwrap(), "2026-07-28".parse().unwrap()]
    }

    #[test]
    fn no_revisions_is_an_error() {
        assert_eq!(
            validate_revisions(&set(), &[], &[]),
            Err(MultiError::NoRevisions)
        );
    }

    #[test]
    fn unknown_revision_names_itself() {
        let unknown: ProtocolRevision = "2024-01-01".parse().unwrap();
        assert_eq!(
            validate_revisions(&set(), &[unknown], &[]),
            Err(MultiError::UnknownRevision(unknown))
        );
        assert!(unknown.to_string().contains("2024-01-01"));
    }

    #[test]
    fn rows_align_outcomes_with_revisions_and_mark_absence() {
        let report = validate_revisions(&set(), &revs(), &[]).unwrap();
        assert_eq!(report.revisions, ["2025-11-25", "2026-07-28"]);
        assert_eq!(report.summaries.len(), 2);

        let find = |id: &str| {
            report
                .requirements
                .iter()
                .find(|row| row.id == id)
                .cloned()
                .unwrap()
        };

        // Present throughout: an outcome in both columns, identical, not flagged.
        let base = find("BASE-001");
        assert!(base.outcomes[0].is_some() && base.outcomes[1].is_some());
        assert!(!base.differs());

        // Removed at the boundary: present, then absent.
        let life = find("LIFE-009");
        assert!(life.outcomes[0].is_some());
        assert_eq!(life.outcomes[1], None);
        assert!(life.differs());

        // Introduced at the boundary: absent, then present.
        let disc = find("DISC-001");
        assert_eq!(disc.outcomes[0], None);
        assert!(disc.outcomes[1].is_some());
        assert!(disc.differs());
    }

    #[test]
    fn union_order_follows_the_set_and_drops_clauses_in_no_judged_revision() {
        // Judge only the older revision: DISC-001 (introduced later) appears in no judged
        // revision and must be dropped entirely, not shown as an all-absent row.
        let older: [ProtocolRevision; 1] = ["2025-11-25".parse().unwrap()];
        let report = validate_revisions(&set(), &older, &[]).unwrap();
        let ids: Vec<&str> = report.requirements.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["BASE-001", "LIFE-009"]);
        // A single judged revision can never "differ".
        assert!(report.requirements.iter().all(|row| !row.differs()));
    }

    #[test]
    fn differs_detects_a_non_adjacent_divergence() {
        // Three identical-then-different columns: pins `any` against `all` and the row
        // comparison against equality.
        let uniform = MultiRow {
            id: "X-001".to_owned(),
            level: "MUST".to_owned(),
            outcomes: vec![
                Some(Outcome::Pass),
                Some(Outcome::Pass),
                Some(Outcome::Pass),
            ],
        };
        assert!(!uniform.differs());
        let diverges = MultiRow {
            outcomes: vec![
                Some(Outcome::Pass),
                Some(Outcome::Pass),
                Some(Outcome::Fail),
            ],
            ..uniform
        };
        assert!(diverges.differs());
    }

    #[test]
    fn overall_verdict_is_the_worst_across_revisions() {
        let mut report = validate_revisions(&set(), &revs(), &[]).unwrap();
        // The synthetic trace is empty, so every real check passes vacuously.
        assert_eq!(report.verdict(), Verdict::Pass);
        // Worsen the second revision and confirm the fold tracks the priority order.
        report.summaries[1].verdict = Verdict::PassWithWarnings;
        assert_eq!(report.verdict(), Verdict::PassWithWarnings);
        report.summaries[1].verdict = Verdict::Fail;
        assert_eq!(report.verdict(), Verdict::Fail);
        report.summaries[0].verdict = Verdict::Unsupported;
        assert_eq!(report.verdict(), Verdict::Unsupported);
    }

    #[test]
    fn human_render_shows_each_revision_cell_and_marks_divergence() {
        let report = validate_revisions(&set(), &revs(), &[]).unwrap();
        let text = report.render_human();
        assert!(text.contains("revisions 2025-11-25, 2026-07-28"), "{text}");
        // The removed clause reads present then absent, and is flagged.
        assert!(text.contains("LIFE-009"), "{text}");
        assert!(text.contains("2026-07-28=absent"), "{text}");
        assert!(text.contains("*differs"), "{text}");
        assert!(text.contains("overall verdict: pass"), "{text}");
    }

    #[test]
    fn judges_a_real_trace_and_is_deterministic() {
        // A real handshake, judged against both revisions, serializes identically twice.
        let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let a = validate_revisions(&set(), &revs(), &events).unwrap();
        let b = validate_revisions(&set(), &revs(), &events).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
