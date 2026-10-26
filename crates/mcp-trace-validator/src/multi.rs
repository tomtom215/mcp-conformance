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

use mcp_conformance_core::requirement::{Actor, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;
use serde::{Deserialize, Serialize};

use crate::engine;
use crate::report::{ClauseSource, Finding, HumanOptions, Outcome, Party, Totals, Verdict};

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
        let mut out = String::new();
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
        self.write_revision_mismatch(&mut out);
        for row in &self.requirements {
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
        self.write_summaries(&mut out);
        match self.party_view {
            Some(party) => {
                let _ = writeln!(
                    out,
                    "overall verdict ({party} findings only): {}",
                    self.verdict()
                );
            }
            None => {
                let _ = writeln!(out, "overall verdict: {}", self.verdict());
            }
        }
        self.write_revision_mismatch(&mut out);
        out
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
            match self.party_view {
                Some(party) => {
                    let _ = write!(out, " ({party} findings only");
                    let what = self.party_tally(index, party).describe(party);
                    if !what.is_empty() {
                        let _ = write!(out, ": {what}");
                    }
                    out.push_str(")\n");
                }
                None => out.push('\n'),
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

impl Serialize for MultiReport {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut report = serializer.serialize_struct("MultiReport", 8)?;
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
