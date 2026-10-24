// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Validation reports: per-requirement outcomes with actionable findings.
//!
//! Reports are artifacts: they get committed as golden files, diffed in CI, and cited
//! in published results. Two consequences shape this module: serialization order is
//! fixed (registry order; struct fields in declaration order), and nothing
//! environment-dependent (paths, timestamps, hostnames) is ever included.

use core::fmt;

use mcp_conformance_core::requirement::Actor;
use serde::{Deserialize, Serialize};

mod clause;
mod human;
mod party;
mod serialize;

pub use party::{Party, PartyTally};

/// The JSON Schema (draft 2020-12) of `validate --format json` output.
///
/// It describes a [`Report`], or a [`MultiReport`](crate::multi::MultiReport)
/// when several revisions were judged. Closed, so the test that validates every
/// golden report and both CLI shapes against it proves each emitted member is
/// documented.
pub const JSON_SCHEMA: &str = include_str!("../schema/report.schema.json");

pub use clause::ClauseSource;
pub use human::{HumanOptions, printable};

/// One concrete violation, addressed to a requirement and (where possible) an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Finding {
    /// The validator check that produced this finding.
    pub check: String,
    /// The event `seq` the finding points at, when one event is identifiable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    /// The party the finding is held against: the clause's actor when it binds
    /// one party; for a clause binding both, the sender of the message at
    /// [`Self::seq`]. Absent when neither decides it — no `seq`, or an event
    /// that is not a message — rather than guessed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party: Option<Party>,
    /// What was observed and what was expected, in one actionable sentence.
    pub detail: String,
}

/// The outcome of evaluating one requirement against one trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Outcome {
    /// All covering checks ran and produced no findings.
    Pass,
    /// A MUST / MUST NOT requirement has findings.
    Fail,
    /// A SHOULD / SHOULD NOT requirement has findings.
    Warn,
    /// The registry documents that this requirement is not judged from traces.
    Excluded,
    /// The registry references a check this validator build does not implement.
    Unsupported,
    /// The requirement is gated on a capability this session never declared
    /// (ADR-0006); its checks were not run.
    NotApplicable,
    /// Every covering check ran and found nothing to judge: this session
    /// carried none of the traffic the clause binds to.
    ///
    /// Distinct from [`Self::Pass`], and the distinction is the whole point. A
    /// clause about `subscriptions/listen` cannot be *complied with* by a
    /// session that never opened a stream — there was no opportunity to break
    /// it — so reporting `pass` states evidence the trace does not carry.
    /// Distinct from [`Self::NotApplicable`] too: that one is the registry
    /// saying the clause is gated on a capability nobody declared, this one is
    /// the trace saying it had nothing to show.
    NotObserved,
}

impl Outcome {
    /// Whether a reader must act on this outcome: a failure, a warning, or a clause
    /// this build could not judge.
    #[must_use]
    pub const fn needs_attention(self) -> bool {
        matches!(self, Self::Fail | Self::Warn | Self::Unsupported)
    }
}

/// Aggregate counts, in report order. `excluded` and `unsupported` are first-class:
/// inflating pass rates by hiding them is how conformance tools lose trust.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Totals {
    /// Requirements with outcome [`Outcome::Pass`].
    pub pass: u32,
    /// Requirements with outcome [`Outcome::Fail`].
    pub fail: u32,
    /// Requirements with outcome [`Outcome::Warn`].
    pub warn: u32,
    /// Requirements with outcome [`Outcome::Excluded`].
    pub excluded: u32,
    /// Requirements with outcome [`Outcome::Unsupported`].
    pub unsupported: u32,
    /// Requirements with outcome [`Outcome::NotApplicable`].
    pub not_applicable: u32,
    /// Requirements with outcome [`Outcome::NotObserved`].
    pub not_observed: u32,
}

impl Totals {
    /// Whether the run judged nothing although there were clauses to judge:
    /// no pass, fail or warning, no unsupported check, and at least one clause
    /// not observed. That is the shape of an empty or contentless recording —
    /// almost always a capture that failed — which the CLI refuses (exit 2)
    /// rather than reporting a vacuous `pass`. A registry that could judge
    /// nothing (all exclusions, or checks this build lacks) is not this.
    #[must_use]
    pub const fn judged_nothing(&self) -> bool {
        self.pass + self.fail + self.warn == 0 && self.unsupported == 0 && self.not_observed > 0
    }

    /// Every outcome's report label and count, in report order.
    ///
    /// Destructured exhaustively on purpose, and that is the whole point of the
    /// method existing: a field added to [`Totals`] fails to compile here until
    /// it is given a label, and every summary line in the crate is formatted
    /// from this one list. Hand-written `write!` arms could not offer that —
    /// the single-revision line named all seven outcomes while the
    /// multi-revision line named six, so the same run reported 140 clauses as
    /// human text and 140 as JSON but only accounted for 125 of them in the
    /// former. Counts that do not add up are how a conformance tool overstates
    /// what it judged.
    #[must_use]
    pub const fn labelled(&self) -> [(&'static str, u32); 7] {
        let Self {
            pass,
            fail,
            warn,
            excluded,
            unsupported,
            not_applicable,
            not_observed,
        } = *self;
        [
            ("pass", pass),
            ("fail", fail),
            ("warn", warn),
            ("excluded", excluded),
            ("unsupported", unsupported),
            ("not applicable", not_applicable),
            ("not observed", not_observed),
        ]
    }
}

/// The counts as one phrase — `23 pass, 0 fail, …` — naming every outcome.
///
/// The summary lines differ in what surrounds them (`totals: ` on a
/// single-revision report, the revision and its verdict on a multi-revision
/// one) and agree on what is inside, so what is inside is written once.
impl fmt::Display for Totals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, (label, count)) in self.labelled().into_iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{count} {label}")?;
        }
        Ok(())
    }
}

/// One requirement's row in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RequirementReport {
    /// The requirement ID (`AREA-NNN`).
    pub id: String,
    /// The requirement's RFC 2119 level, as registry text (`"MUST"`, …).
    pub level: String,
    /// The party the clause binds, as the registry states it. Every row the
    /// engine builds carries it; `None` only in a row built by hand or read
    /// from a report written before the member existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<Actor>,
    /// The evaluation outcome.
    pub outcome: Outcome,
    /// Findings, in event order. Empty unless `outcome` is `fail` or `warn`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    /// The documented exclusion reason, when `outcome` is `excluded`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusion: Option<String>,
    /// Check IDs the build lacks, when `outcome` is `unsupported`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_checks: Vec<String>,
    /// The undeclared capability gate, when `outcome` is `not-applicable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    /// The violated clause, when `outcome` is `fail` or `warn` — the rows a reader
    /// acts on. Other rows omit it; `mcp-trace-validator requirements` lists every
    /// clause's source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ClauseSource>,
}

/// Whether a session's messages are valid against the judged revision's own
/// JSON Schema ([`crate::wire_schema`], ADR-0020).
///
/// Reported beside the requirement rows, not as one of them: the specification
/// makes the schema normative ("the source of truth for all protocol messages
/// and structures") without an RFC 2119 sentence a row could quote, so it has
/// no requirement ID and does not count in [`Totals`]. A failure here fails the
/// [`Verdict`] all the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WireSchemaReport {
    /// `pass`, `fail` (some message is invalid), or `not-observed` (the trace
    /// held no message).
    pub outcome: Outcome,
    /// How many messages were validated.
    pub messages: u32,
    /// One finding per invalid message, in trace order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
    /// The vendored schema, at the upstream commit it was copied from.
    pub schema: String,
    /// The specification's statement that the schema is normative.
    pub source: ClauseSource,
}

/// A complete validation report for one trace against one registry.
///
/// Serialized with its [`verdict`](Self::verdict) between the revision fields
/// and the totals — derived on output, never stored, so it cannot disagree with
/// the counts; a `verdict` member is ignored on input.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Report {
    /// The registry's protocol revision (`YYYY-MM-DD`).
    pub revision: String,
    /// The revisions the *session* declared, when it declared some and
    /// [`Self::revision`] is not among them — so the reader is told that these
    /// findings judge the trace against rules it was not playing by.
    ///
    /// Absent whenever there is nothing to say, which is the common case; see
    /// [`crate::declared`] for what counts as a declaration and why the rule is
    /// deliberately quiet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_mismatch: Option<Vec<String>>,
    /// How [`Self::revision`] was chosen, when the caller recorded it (the CLI
    /// always does). Absent from reports built by [`crate::engine::validate`]
    /// directly, which is handed a registry and chooses nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_source: Option<crate::declared::RevisionSource>,
    /// The party this report is viewed for (`validate --party`), when it is
    /// one: only findings held against that party, and findings held against
    /// nobody, count toward [`Self::effective_verdict`], which the JSON's
    /// `verdict` member and the exit status then follow. The rows and
    /// [`Self::totals`] are the trace's facts and do not change with the view.
    /// Absent by default, when every finding counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub party_view: Option<Party>,
    /// Aggregate counts.
    pub totals: Totals,
    /// Per-requirement outcomes, in registry order.
    pub requirements: Vec<RequirementReport>,
    /// The messages against the revision's JSON Schema; absent when this build
    /// vendors no schema for [`Self::revision`] (an external registry's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_schema: Option<WireSchemaReport>,
    /// The clauses of each extension the session exercised, judged and never
    /// scored: nothing here moves [`Self::totals`] or [`Self::verdict`]
    /// (ADR-0023). An extension appears once one of its clauses judged
    /// something — passed, failed, warned — or names a check this build lacks;
    /// one the session never touched has nothing to report and is absent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<ExtensionReport>,
}

/// One extension's clauses as a trace was judged against them: reported beside
/// the revision's own, never counted with them (ADR-0023).
///
/// An extension is optional by definition — the specification's extensions are
/// "disabled by default and require explicit opt-in" — so conforming to the
/// protocol cannot depend on one. The official suite draws the same line: its
/// requirement sets run an extension's scenarios under `not_scored` with the
/// reason `extension`, reported and never moving the pass rate. Here the
/// extension keeps its own [`Totals`] and its own [`verdict`](Self::verdict),
/// for the reader who implements it; the report's verdict ignores both.
///
/// Serialized with its verdict after the extension's identifier, derived from
/// the totals as [`Report`]'s is; a `verdict` member is ignored on input.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct ExtensionReport {
    /// The extension's identifier (`io.modelcontextprotocol/tasks`).
    pub extension: String,
    /// The extension's own counts, kept out of the report's.
    pub totals: Totals,
    /// Its clauses' outcomes, in registry order.
    pub requirements: Vec<RequirementReport>,
}

impl ExtensionReport {
    /// What the extension's clauses add up to, by the rule a [`Report`] derives
    /// its own: unsupported over fail over warnings over pass. Informational —
    /// no exit status, and no report verdict, depends on it.
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        if self.totals.unsupported > 0 {
            Verdict::Unsupported
        } else if self.totals.fail > 0 {
            Verdict::Fail
        } else if self.totals.warn > 0 {
            Verdict::PassWithWarnings
        } else {
            Verdict::Pass
        }
    }
}

impl Report {
    /// `true` when any requirement failed (errors, not warnings), or a message
    /// is invalid against the revision's JSON Schema.
    #[must_use]
    pub const fn has_errors(&self) -> bool {
        self.totals.fail > 0 || self.wire_schema_failed()
    }

    /// `true` when [`Self::wire_schema`] found an invalid message.
    #[must_use]
    pub const fn wire_schema_failed(&self) -> bool {
        matches!(
            &self.wire_schema,
            Some(WireSchemaReport {
                outcome: Outcome::Fail,
                ..
            })
        )
    }

    /// `true` when any SHOULD-level requirement produced findings.
    #[must_use]
    pub const fn has_warnings(&self) -> bool {
        self.totals.warn > 0
    }

    /// `true` when the registry referenced checks this build does not implement.
    #[must_use]
    pub const fn has_unsupported(&self) -> bool {
        self.totals.unsupported > 0
    }

    /// One-word verdict for the trailing summary line: every finding counts.
    #[must_use]
    pub const fn verdict(&self) -> Verdict {
        if self.totals.unsupported > 0 {
            Verdict::Unsupported
        } else if self.has_errors() {
            Verdict::Fail
        } else if self.totals.warn > 0 {
            Verdict::PassWithWarnings
        } else {
            Verdict::Pass
        }
    }

    /// The verdict as the report is viewed: [`Self::verdict`], or
    /// [`Self::verdict_for`] its [`Self::party_view`] when one is set. What the
    /// JSON's `verdict` member carries and the exit status follows.
    #[must_use]
    pub fn effective_verdict(&self) -> Verdict {
        self.party_view
            .map_or_else(|| self.verdict(), |party| self.verdict_for(party))
    }
}

/// Overall verdict of a validation run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Verdict {
    /// No findings at all.
    Pass,
    /// Only SHOULD-level findings.
    PassWithWarnings,
    /// At least one MUST-level violation, or a message invalid against the
    /// revision's JSON Schema.
    Fail,
    /// The registry and this build disagree about available checks.
    Unsupported,
    /// A run over several traces could not judge at least one of them — it was
    /// unreadable, malformed, of an unsupported revision, or judged nothing —
    /// so no verdict covers the whole run. Never the verdict of one report: a
    /// trace that cannot be judged has no report.
    ///
    /// Ranked above every other verdict, as its exit status (`2` or `3`) ranks
    /// above theirs: whatever the judged traces say, a run that left one out
    /// must not read as passing.
    Incomplete,
}

impl Verdict {
    /// The most severe of `verdicts` — incomplete over unsupported over fail
    /// over warnings over pass, the order a report derives its own — or `None`
    /// when there are none.
    pub fn most_severe(verdicts: impl IntoIterator<Item = Self>) -> Option<Self> {
        verdicts
            .into_iter()
            .max_by_key(|verdict| verdict.severity())
    }

    const fn severity(self) -> u8 {
        match self {
            Self::Pass => 0,
            Self::PassWithWarnings => 1,
            Self::Fail => 2,
            Self::Unsupported => 3,
            Self::Incomplete => 4,
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Pass => "pass",
            Self::PassWithWarnings => "pass-with-warnings",
            Self::Fail => "fail",
            Self::Unsupported => "unsupported",
            Self::Incomplete => "incomplete",
        };
        f.write_str(text)
    }
}

/// A trace a run was asked to judge and could not, and why: unreadable,
/// malformed, of an unsupported revision, or judging nothing.
///
/// The multi-trace renderers ([`junit::render_run`](crate::junit::render_run),
/// [`sarif::render_run`](crate::sarif::render_run)) list each one, so a report
/// over several traces never reads as complete when one was left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Unjudged {
    /// The trace as the caller named it.
    pub trace: String,
    /// Why it could not be judged, as one line.
    pub error: String,
}

impl Unjudged {
    /// `trace`, which could not be judged because of `error`.
    #[must_use]
    pub fn new(trace: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            trace: trace.into(),
            error: error.into(),
        }
    }
}

#[cfg(test)]
mod tests;
