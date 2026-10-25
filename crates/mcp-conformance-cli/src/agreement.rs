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
//! **Whose failure it is.** The suite drives its subject with a client of its
//! own (or, for a client under test, a server of its own), and that end breaks
//! clauses too. Each failure therefore carries the party its findings are held
//! against ([`failures_by_party`]), and [`reconcile_for`] holds a run to the
//! *subject's* party: what is held against the other end is set aside — kept
//! and shown, never counted — and what is held against nobody counts, as the
//! validator's own party view counts it ([`Report::verdict_for`]). A baseline
//! entry still explains a failure of either party, and is live while it
//! explains any: the one baseline file serves the repository's gate, which
//! sees every failure, and `test`, which sees the subject's.
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
//!
//! The same failures, for a server under test whose suite's client broke the
//! clause:
//!
//! ```
//! use mcp_conformance_cli::agreement::{Baseline, ValidatorFailure, reconcile_for};
//! use mcp_trace_validator::report::Party;
//!
//! let failure = ValidatorFailure::new("002.jsonl", "LIFE-009", "logging not negotiated")
//!     .held_against(Party::Client);
//! let reconciliation = reconcile_for(&Baseline::default(), &[failure], Party::Server);
//! assert!(reconciliation.holds(), "the suite's client's failure is not the server's");
//! assert_eq!(reconciliation.set_aside.len(), 1);
//! ```

use mcp_trace_validator::report::{Outcome, Party, Report};
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
    /// The party that first finding is held against; `None` when the failure
    /// was not attributed ([`failures`]) or the trace could not say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub party: Option<Party>,
}

impl ValidatorFailure {
    /// A failure of `requirement` on `trace`, described by `detail`, held
    /// against nobody in particular.
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
            party: None,
        }
    }

    /// This failure, held against `party`.
    #[must_use]
    pub const fn held_against(mut self, party: Party) -> Self {
        self.party = Some(party);
        self
    }

    /// ` [client]` for a failure held against a party; ` [unattributed]` for
    /// one held against nobody when the run is `scoped` to a party (where it
    /// counts, and a reader should see that it was counted rather than
    /// attributed); empty otherwise.
    fn tag(&self, scoped: bool) -> String {
        match (self.party, scoped) {
            (Some(party), _) => format!(" [{party}]"),
            (None, true) => " [unattributed]".to_owned(),
            (None, false) => String::new(),
        }
    }
}

/// Every failing clause in `report`, as failures of the trace named `trace`,
/// whoever each is held against (so none carries a party).
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
            party: None,
        })
        .collect()
}

/// Every failing clause in `report` once for each party its findings are held
/// against — and once more for the findings held against nobody — each with
/// the first of that party's findings as its detail.
///
/// A clause can fail on both ends of a session (the client reuses a request id
/// and the server answers a request with two results), and the two failures are
/// not each other's: a run holds the subject to its own.
#[must_use]
pub fn failures_by_party(trace: &str, report: &Report) -> Vec<ValidatorFailure> {
    let mut found = Vec::new();
    for row in report
        .requirements
        .iter()
        .filter(|row| row.outcome == Outcome::Fail)
    {
        let mut parties: Vec<Option<Party>> = Vec::new();
        for finding in &row.findings {
            if !parties.contains(&finding.party) {
                parties.push(finding.party);
            }
        }
        if parties.is_empty() {
            // A failing clause with no finding to attribute: still a failure.
            parties.push(None);
        }
        for party in parties {
            let detail = row
                .findings
                .iter()
                .find(|finding| finding.party == party)
                .map_or_else(String::new, |finding| finding.detail.clone());
            found.push(ValidatorFailure {
                trace: trace.to_owned(),
                requirement: row.id.clone(),
                detail,
                party,
            });
        }
    }
    found
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

/// The failures of a run, split against a baseline (and, for a run scoped to a
/// party, against that party).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Reconciliation {
    /// Failures no baseline entry explains, held against the subject or
    /// against nobody: the ones that break agreement.
    pub unexplained: Vec<ValidatorFailure>,
    /// Failures a baseline entry explains, whichever party they are held
    /// against.
    pub explained: Vec<ValidatorFailure>,
    /// Failures held against the other end of the session — the suite's own
    /// client for a server under test, the suite's own server for a client —
    /// that no baseline entry explains: shown, and never counted. Empty unless
    /// the run is scoped to a party ([`reconcile_for`]), and then left out of
    /// the serialized form when empty, so a run without a party serializes as
    /// it always has.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub set_aside: Vec<ValidatorFailure>,
    /// Baseline entries (as [`ExplainedDivergence::describe`] names them) that
    /// explained nothing in this run.
    #[serde(rename = "stale_baseline_entries")]
    pub stale: Vec<String>,
    /// The party the run is scoped to, when it is.
    #[serde(skip)]
    subject: Option<Party>,
}

impl Reconciliation {
    /// Whether the suite and the validator agree: no unexplained divergence and
    /// no stale baseline entry.
    #[must_use]
    pub const fn holds(&self) -> bool {
        self.unexplained.is_empty() && self.stale.is_empty()
    }

    /// The party the run was held to ([`reconcile_for`]); `None` for a run
    /// that counted every failure ([`reconcile`]).
    #[must_use]
    pub const fn subject(&self) -> Option<Party> {
        self.subject
    }

    /// Why agreement does not hold, with what to do about it, naming
    /// `baseline` (the file the run was reconciled against); `None` when it
    /// holds.
    #[must_use]
    pub fn gate_error(&self, baseline: &str) -> Option<String> {
        let mut sections = Vec::new();
        if !self.unexplained.is_empty() {
            let scoped = self.subject.is_some();
            let listing: Vec<String> = self
                .unexplained
                .iter()
                .map(|failure| {
                    format!(
                        "  {} on {}{}: {}",
                        failure.requirement,
                        failure.trace,
                        failure.tag(scoped),
                        failure.detail
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
///
/// Every failure counts, whoever it is held against: this is the repository's
/// gate, which taps one end of a session and judges both. A run with a subject
/// is [`reconcile_for`].
#[must_use]
pub fn reconcile(baseline: &Baseline, failures: &[ValidatorFailure]) -> Reconciliation {
    partition(baseline, failures, None)
}

/// [`reconcile`] for a run whose subject is `subject`.
///
/// A failure held against the other party and not explained by the baseline is
/// set aside ([`Reconciliation::set_aside`]) instead of breaking agreement. One
/// held against `subject`, or against nobody (the trace could not say), counts.
///
/// Staleness is judged against every failure, set aside or not: an entry that
/// explains a failure of the other end is documenting a real suite bug, and the
/// baseline file is shared with runs that count both ends.
#[must_use]
pub fn reconcile_for(
    baseline: &Baseline,
    failures: &[ValidatorFailure],
    subject: Party,
) -> Reconciliation {
    partition(baseline, failures, Some(subject))
}

fn partition(
    baseline: &Baseline,
    failures: &[ValidatorFailure],
    subject: Option<Party>,
) -> Reconciliation {
    let explains = |failure: &ValidatorFailure| {
        baseline
            .divergences
            .iter()
            .any(|entry| entry.explains(failure))
    };
    let mut reconciliation = Reconciliation {
        subject,
        ..Reconciliation::default()
    };
    let other = subject.map(Party::other);
    for failure in failures {
        if explains(failure) {
            reconciliation.explained.push(failure.clone());
        } else if other.is_some_and(|other| failure.party == Some(other)) {
            reconciliation.set_aside.push(failure.clone());
        } else {
            reconciliation.unexplained.push(failure.clone());
        }
    }
    reconciliation.stale = baseline
        .divergences
        .iter()
        .filter(|entry| !failures.iter().any(|failure| entry.explains(failure)))
        .map(ExplainedDivergence::describe)
        .collect();
    reconciliation
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
