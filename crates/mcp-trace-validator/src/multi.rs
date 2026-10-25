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

use mcp_conformance_core::requirement::{Actor, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;
use serde::{Deserialize, Serialize};

use crate::engine;
use crate::report::{ClauseSource, Finding, Outcome, Party, Totals, Verdict};

mod baseline;
mod human;
mod party;
mod rows;

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
    /// The messages against this revision's JSON Schema
    /// ([`Report::wire_schema`](crate::report::Report::wire_schema)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_schema: Option<crate::report::WireSchemaReport>,
    /// The extensions the session exercised at this revision, judged and never
    /// scored ([`Report::extensions`](crate::report::Report::extensions)):
    /// [`Self::verdict`] does not read them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<crate::report::ExtensionReport>,
}

/// One clause's row across every judged revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MultiRow {
    /// The requirement ID (`AREA-NNN`).
    pub id: String,
    /// The requirement's RFC 2119 level, as registry text (`"MUST"`, …).
    pub level: String,
    /// The party the clause binds, as the registry states it
    /// ([`RequirementReport::actor`](crate::report::RequirementReport::actor)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<Actor>,
    /// Outcome under each judged revision, aligned by index with
    /// [`MultiReport::revisions`]. `None` means the clause does not exist at that
    /// revision — *absent*, not [`Outcome::NotApplicable`].
    pub outcomes: Vec<Option<Outcome>>,
    /// The findings behind each outcome, aligned by index with
    /// [`MultiReport::revisions`]: what failed or warned, where (`seq`), and why.
    /// Omitted from JSON when no revision has any, which is the common case.
    #[serde(default, skip_serializing_if = "no_findings")]
    pub findings: Vec<Vec<Finding>>,
    /// The violated clause behind each `fail` or `warn` outcome, aligned by index
    /// with [`MultiReport::revisions`] — per revision because the published page
    /// is. Omitted from JSON when no revision has one.
    #[serde(default, skip_serializing_if = "no_sources")]
    pub sources: Vec<Option<ClauseSource>>,
}

/// Whether a row carries no clause source under any revision.
fn no_sources(sources: &[Option<ClauseSource>]) -> bool {
    sources.iter().all(Option::is_none)
}

/// Whether a row carries no finding under any revision.
fn no_findings(findings: &[Vec<Finding>]) -> bool {
    findings.iter().all(Vec::is_empty)
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
/// Like [`Report`](crate::report::Report), it is an artifact — serialization order is fixed (revisions in the
/// order requested; clauses in registry-union order) and nothing environment-dependent
/// appears. Its JSON carries the overall [`verdict`](Self::verdict) after the
/// revision fields, derived on output like the single report's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct MultiReport {
    /// The revisions judged, in the order requested — the column order for every row.
    pub revisions: Vec<String>,
    /// The revisions the *session* declared, when it declared some and none of
    /// [`Self::revisions`] is among them. Named explicitly on the command line
    /// or not, judging a recording against rules it was never playing by is the
    /// same mistake, and the reader is told so either way
    /// ([`crate::declared`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_mismatch: Option<Vec<String>>,
    /// How [`Self::revisions`] were chosen, when the caller recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_source: Option<crate::declared::RevisionSource>,
    /// The party the report is viewed for, when it is one
    /// ([`Report::party_view`](crate::report::Report::party_view)): set by
    /// [`Self::view_party`], which also recomputes each summary's verdict
    /// from the findings held against that party.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_view: Option<Party>,
    /// Whether a baseline was applied ([`Self::adopt_baseline`]): findings it
    /// accepts carry their [`Finding::baselined`], and no revision's verdict
    /// counts them. Serialized as the `baseline` member, whose counts are
    /// derived ([`Self::baseline_tally`]); absent by default.
    #[serde(
        default,
        rename = "baseline",
        deserialize_with = "crate::report::baseline_present"
    )]
    pub baseline_applied: bool,
    /// The requests the trace ended without answering
    /// ([`Report::unanswered_requests`](crate::report::Report::unanswered_requests)):
    /// a fact of the trace, the same under every revision, so carried once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unanswered_requests: Option<crate::report::UnansweredRequests>,
    /// Per-revision aggregate results, aligned by index with `revisions`.
    pub summaries: Vec<RevisionSummary>,
    /// Union of clauses across the judged revisions, in registry-union order. A clause is
    /// included when it exists at one or more of the judged revisions.
    pub requirements: Vec<MultiRow>,
}

impl MultiReport {
    /// The overall verdict: the worst across revisions, by the same severity priority a
    /// single [`Report`](crate::report::Report) uses (unsupported ≻ fail ≻ pass-with-warnings ≻ pass). A
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
}

impl Serialize for MultiReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut report = serializer.serialize_struct("MultiReport", 9)?;
        report.serialize_field("revisions", &self.revisions)?;
        if let Some(mismatch) = &self.revision_mismatch {
            report.serialize_field("revision_mismatch", mismatch)?;
        } else {
            report.skip_field("revision_mismatch")?;
        }
        if let Some(source) = &self.revision_source {
            report.serialize_field("revision_source", source)?;
        } else {
            report.skip_field("revision_source")?;
        }
        if let Some(party) = &self.party_view {
            report.serialize_field("party_view", party)?;
        } else {
            report.skip_field("party_view")?;
        }
        if self.baseline_applied {
            report.serialize_field("baseline", &self.baseline_tally())?;
        } else {
            report.skip_field("baseline")?;
        }
        report.serialize_field("verdict", &self.verdict())?;
        report.serialize_field("summaries", &self.summaries)?;
        report.serialize_field("requirements", &self.requirements)?;
        if let Some(unanswered) = &self.unanswered_requests {
            report.serialize_field("unanswered_requests", unanswered)?;
        } else {
            report.skip_field("unanswered_requests")?;
        }
        report.end()
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
            wire_schema: report.wire_schema.clone(),
            extensions: report.extensions.clone(),
        });
        reports.push(report);
    }

    let rows = rows::align(set, &reports);

    Ok(MultiReport {
        revisions: revisions.iter().map(ProtocolRevision::to_string).collect(),
        revision_mismatch: crate::declared::mismatch_any(revisions, events),
        revision_source: None,
        party_view: None,
        baseline_applied: false,
        // The same under every revision: it reads the events, not the registry.
        unanswered_requests: reports
            .first()
            .and_then(|report| report.unanswered_requests.clone()),
        summaries,
        requirements: rows,
    })
}

#[cfg(test)]
mod tests;
