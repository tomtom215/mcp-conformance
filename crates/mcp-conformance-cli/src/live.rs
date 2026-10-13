// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Judging a session while it runs: `mcp-conformance watch`'s judge.
//!
//! [`Live`] takes a trace one line at a time, in `seq` order, as the recorder
//! writes it. After each line that completes an exchange — a response paired
//! with its request, a notification, or anything else that is not a request
//! still waiting (a stray response, a line that is not JSON-RPC) — the caller
//! asks it to [`Live::judge`] the trace so far, and gets the findings that
//! pass has and no earlier pass reported. A finding therefore prints in the
//! pass of the exchange its violating message completes — the response, the
//! notification — or, for a request, the exchange its response completes;
//! a request never answered is judged at the end.
//!
//! **What counts as a finding.** A `fail` or `warn` row's finding, keyed by
//! revision, requirement, check, `seq` and detail, reported once. An outcome
//! that changes for another reason (a clause going from `not-observed` to
//! `pass` as traffic reaches it) is not a finding and is never reported.
//!
//! **Nothing provisional.** Judged on a prefix, a check that concludes from
//! what the rest of the trace does *not* hold sees a session that has simply
//! not got there yet: right after the `initialize` response, `LIFE-003` ("the
//! client MUST send an `initialized` notification") fails, and a later message
//! retracts it — after one exchange, or after several when the server asks
//! the client something first. A retracted finding is not a finding, so the
//! findings of such checks ([`EVENTUAL_CHECKS`]) are left to the final
//! judgment. That they are the only ones is tested, not assumed: every corpus
//! trace replayed line by line reports nothing live that the whole trace does
//! not (`tests/live_corpus.rs`). Should a finding still be retracted in some
//! session the corpus does not cover, [`Live::finish`] names it as
//! *withdrawn*, so the live output never stands uncorrected.
//!
//! **The final word is the validator's.** [`Live::finish`] judges the whole
//! trace with [`judge::judge`], exactly as `mcp-trace-validator validate`
//! would judge the file, and any finding only that judgment shows is reported
//! then.
//!
//! Each pass re-judges the whole prefix (the engine has no incremental mode).
//! Its cost and how `watch` keeps it off the session are measured in the
//! crate README.

use std::collections::HashSet;
use std::time::SystemTime;

use mcp_conformance_core::requirement::Registry;
use mcp_conformance_core::requirement::{RegistryError, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::judge::{self, JudgeError, Judgment};
use mcp_trace_validator::report::{Outcome, Report, Totals};
use mcp_trace_validator::{declared, engine};

pub use exchange::{Ending, Exchange};

mod exchange;

/// The checks whose findings say what the rest of the trace does not hold, and
/// so are only final when the trace is: judged on a prefix they report what
/// the session may still do. Left to [`Live::finish`].
pub const EVENTUAL_CHECKS: &[&str] = &["lifecycle.initialized-notification"];

/// A `fail` or `warn` finding, with the clause it breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LiveFinding {
    /// The revision it was judged at.
    pub revision: String,
    /// The requirement ID (`BASE-003`), or `wire-schema` for a message the
    /// revision's JSON Schema rejects.
    pub requirement: String,
    /// The requirement's level (`MUST`, `SHOULD`, …).
    pub level: String,
    /// [`Outcome::Fail`] or [`Outcome::Warn`].
    pub outcome: Outcome,
    /// The check that found it.
    pub check: String,
    /// The event it points at, when one is identifiable.
    pub seq: Option<u64>,
    /// What was observed and what was expected.
    pub detail: String,
    /// The clause, verbatim, when the report carries it.
    pub quote: Option<String>,
    /// Where the clause is published.
    pub url: Option<String>,
}

type Key = (String, String, String, Option<u64>, String);

impl LiveFinding {
    fn key(&self) -> Key {
        (
            self.revision.clone(),
            self.requirement.clone(),
            self.check.clone(),
            self.seq,
            self.detail.clone(),
        )
    }
}

/// What [`Live::push`] made of one line.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Pushed {
    /// Exchanges the line completed, in order.
    pub completed: Vec<Exchange>,
    /// Whether to judge again before the next line.
    pub due: bool,
}

/// What one pass found.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Pass {
    /// Findings no earlier pass showed, in report order.
    pub new: Vec<LiveFinding>,
    /// Each judged revision and its counts, for a tally.
    pub totals: Vec<(String, Totals)>,
}

/// The end of a session: the validator's judgment of the whole trace.
#[derive(Debug)]
#[non_exhaustive]
pub struct Final {
    /// Findings only the final judgment shows.
    pub new: Vec<LiveFinding>,
    /// Findings reported live that the final judgment does not show.
    pub withdrawn: Vec<LiveFinding>,
    /// Requests the recording ended before answering, ended at the time given.
    pub unanswered: Vec<Exchange>,
    /// The judgment, or why the trace cannot be judged.
    pub judgment: Result<Judgment, JudgeError>,
}

/// One session's live judge.
#[derive(Debug)]
pub struct Live {
    set: RegistrySet,
    registries: Vec<Registry>,
    document: String,
    events: Vec<TraceEvent>,
    pairing: exchange::Pairing,
    reported: Vec<LiveFinding>,
    seen: HashSet<Key>,
    exchanges: u64,
    unreadable: u64,
}

impl Live {
    /// A judge for a new session, with the built-in registries.
    ///
    /// # Errors
    ///
    /// A built-in registry failed to load (a build defect).
    pub fn new() -> Result<Self, RegistryError> {
        Ok(Self {
            set: RegistrySet::builtin()?,
            registries: Vec::new(),
            document: String::new(),
            events: Vec::new(),
            pairing: exchange::Pairing::default(),
            reported: Vec::new(),
            seen: HashSet::new(),
            exchanges: 0,
            unreadable: 0,
        })
    }

    /// Takes in the next line of the trace, recorded at `at`.
    pub fn push(&mut self, line: &[u8], at: SystemTime) -> Pushed {
        let Ok(text) = std::str::from_utf8(line) else {
            self.unreadable += 1;
            return Pushed::default();
        };
        self.document.push_str(text);
        self.document.push('\n');
        let Ok(event) = serde_json::from_str::<TraceEvent>(text) else {
            // Kept in the document, for the final judgment to refuse as the
            // validator would; not judged live.
            self.unreadable += 1;
            return Pushed::default();
        };
        let step = self.pairing.push(&event, at);
        self.events.push(event);
        self.exchanges += u64::try_from(step.completed.len()).unwrap_or(0);
        Pushed {
            completed: step.completed,
            due: step.due,
        }
    }

    /// Exchanges completed so far.
    #[must_use]
    pub const fn exchanges(&self) -> u64 {
        self.exchanges
    }

    /// Events taken in so far.
    #[must_use]
    pub const fn events(&self) -> usize {
        self.events.len()
    }

    /// Judges the trace so far, at the revision(s) it declares (else the
    /// newest), as the validator would; returns what is new.
    ///
    /// A prefix the validator could not judge yet — one declaring only
    /// revisions this build lacks — finds nothing.
    pub fn judge(&mut self) -> Pass {
        let Ok(selection) = declared::select(self.set.revisions(), &self.events) else {
            return Pass::default();
        };
        let mut reports = Vec::new();
        for revision in selection.revisions {
            if let Some(index) = self.registry(revision)
                && let Ok(report) = engine::try_validate(&self.registries[index], &self.events)
            {
                reports.push(report);
            }
        }
        let totals = reports
            .iter()
            .map(|report| (report.revision.clone(), report.totals))
            .collect();
        Pass {
            new: self.unseen(
                findings(&reports)
                    .into_iter()
                    .filter(|finding| !EVENTUAL_CHECKS.contains(&finding.check.as_str()))
                    .collect(),
            ),
            totals,
        }
    }

    /// The index of `revision`'s registry, loaded once.
    fn registry(&mut self, revision: ProtocolRevision) -> Option<usize> {
        let known = |registries: &[Registry]| {
            registries
                .iter()
                .position(|known| known.revision() == revision)
        };
        if known(&self.registries).is_none() {
            self.registries.push(self.set.registry(revision)?);
        }
        known(&self.registries)
    }

    fn unseen(&mut self, found: Vec<LiveFinding>) -> Vec<LiveFinding> {
        let mut new = Vec::new();
        for finding in found {
            if self.seen.insert(finding.key()) {
                self.reported.push(finding.clone());
                new.push(finding);
            }
        }
        new
    }

    /// The session is over: judges the whole trace as `mcp-trace-validator
    /// validate` would, and ends every unanswered request at `at`.
    pub fn finish(&mut self, at: SystemTime) -> Final {
        let judgment = judge::judge(&self.document);
        let found = judgment
            .as_ref()
            .map(|judgment| findings(&judgment.reports))
            .unwrap_or_default();
        // A trace the validator refuses withdraws nothing: it has no findings
        // to compare with, and the refusal is reported in their place.
        let withdrawn = if judgment.is_ok() {
            withdrawn(&self.reported, &found)
        } else {
            Vec::new()
        };
        Final {
            new: self.unseen(found),
            withdrawn,
            unanswered: self.pairing.unanswered(at),
            judgment,
        }
    }
}

/// The `reported` findings `finally` does not hold.
fn withdrawn(reported: &[LiveFinding], finally: &[LiveFinding]) -> Vec<LiveFinding> {
    let finally: HashSet<Key> = finally.iter().map(LiveFinding::key).collect();
    reported
        .iter()
        .filter(|finding| !finally.contains(&finding.key()))
        .cloned()
        .collect()
}

/// Every `fail` and `warn` finding of `reports`, wire-schema ones included.
fn findings(reports: &[Report]) -> Vec<LiveFinding> {
    let mut found = Vec::new();
    for report in reports {
        for row in &report.requirements {
            if !matches!(row.outcome, Outcome::Fail | Outcome::Warn) {
                continue;
            }
            for finding in &row.findings {
                found.push(LiveFinding {
                    revision: report.revision.clone(),
                    requirement: row.id.clone(),
                    level: row.level.clone(),
                    outcome: row.outcome,
                    check: finding.check.clone(),
                    seq: finding.seq,
                    detail: finding.detail.clone(),
                    quote: row.source.as_ref().map(|source| source.quote.clone()),
                    url: row.source.as_ref().map(|source| source.url.clone()),
                });
            }
        }
        if let Some(schema) = report
            .wire_schema
            .as_ref()
            .filter(|schema| schema.outcome == Outcome::Fail)
        {
            for finding in &schema.findings {
                found.push(LiveFinding {
                    revision: report.revision.clone(),
                    requirement: "wire-schema".to_owned(),
                    level: "schema".to_owned(),
                    outcome: Outcome::Fail,
                    check: finding.check.clone(),
                    seq: finding.seq,
                    detail: finding.detail.clone(),
                    quote: Some(schema.source.quote.clone()),
                    url: Some(schema.source.url.clone()),
                });
            }
        }
    }
    found
}

#[cfg(test)]
mod tests;
