// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The identity of a finding: the one definition SARIF's `partialFingerprints`
//! and the baseline ([`crate::baseline`]) share.
//!
//! A fingerprint is `{rule}:{check}:{detail}:{n}`:
//!
//! - `rule` is the requirement ID, `wire-schema` for a message invalid against
//!   the revision's JSON Schema, and either suffixed `@revision` where the same
//!   ID has findings under more than one judged revision;
//! - `check` is the validator check that produced the finding;
//! - `detail` is the finding's text with every run of ASCII digits folded to
//!   `#`, so a re-recorded trace (every `seq`, every id moves) keeps its
//!   findings' identities;
//! - `n` counts, from 1, the findings of one trace sharing everything before
//!   it, in report, row and finding order, so repeated identical findings stay
//!   distinct.
//!
//! The trace's path is deliberately not part of it: code scanning already tells
//! traces apart by location, and a renamed trace keeps its alerts. Computing
//! every fingerprint of a run in one pass, in one order, is what lets SARIF and
//! the baseline agree on `n`.

use std::collections::{BTreeMap, HashMap};

use crate::report::{Finding, Outcome, Report};

/// The fingerprint's key in a SARIF result's `partialFingerprints`, and the
/// scheme a baseline file names.
///
/// The version suffix moves with any change to how a fingerprint is formed, so
/// a consumer's alerts (or a baseline) reopen once rather than silently
/// mismatch.
pub const KEY: &str = "mcpConformanceFinding/v2";

/// The rule of wire-schema findings: lower case, so it can never collide with a
/// requirement ID.
pub(crate) const WIRE_SCHEMA_RULE: &str = "wire-schema";

/// Which list of findings, within one report, a finding is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Place {
    /// The requirement row at this index of [`Report::requirements`].
    Row(usize),
    /// The failed wire schema's findings.
    WireSchema,
    /// A requirement row of an extension of [`Report::extensions`].
    Extension { extension: usize, row: usize },
}

impl Place {
    /// The findings this place names in `report`, when it names any.
    pub(crate) fn findings(self, report: &Report) -> Option<&[Finding]> {
        match self {
            Self::Row(row) => Some(&report.requirements.get(row)?.findings),
            Self::WireSchema => Some(&report.wire_schema.as_ref()?.findings),
            Self::Extension { extension, row } => Some(
                &report
                    .extensions
                    .get(extension)?
                    .requirements
                    .get(row)?
                    .findings,
            ),
        }
    }

    /// As [`Self::findings`], to change them.
    pub(crate) fn findings_mut(self, report: &mut Report) -> Option<&mut Vec<Finding>> {
        match self {
            Self::Row(row) => Some(&mut report.requirements.get_mut(row)?.findings),
            Self::WireSchema => Some(&mut report.wire_schema.as_mut()?.findings),
            Self::Extension { extension, row } => Some(
                &mut report
                    .extensions
                    .get_mut(extension)?
                    .requirements
                    .get_mut(row)?
                    .findings,
            ),
        }
    }
}

/// One finding's address in a run: the trace, the report of that trace (one per
/// judged revision), the list of findings, and the position in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Slot {
    pub(crate) trace: usize,
    pub(crate) report: usize,
    pub(crate) place: Place,
    pub(crate) finding: usize,
}

/// A finding of the run with its fingerprint.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) slot: Slot,
    pub(crate) fingerprint: String,
    /// Whether the finding counts toward a verdict: false for an extension's,
    /// which is reported and never scored (ADR-0023).
    pub(crate) scored: bool,
}

/// The fingerprints taken so far, and how many findings of each stem each trace
/// has had: the count that ends a fingerprint.
#[derive(Default)]
struct Counter {
    seen: BTreeMap<(String, String), usize>,
    found: Vec<Found>,
}

impl Counter {
    /// Fingerprints `findings`, a list of the report at `(trace, report, place)`
    /// of the trace named `name`, under `rule_id`, in order.
    fn list(
        &mut self,
        (name, rule_id): (&str, &str),
        (trace, report, place): (usize, usize, Place),
        findings: &[Finding],
        scored: bool,
    ) {
        for (finding_index, finding) in findings.iter().enumerate() {
            let stem = format!(
                "{rule_id}:{}:{}",
                finding.check,
                fold_numbers(&finding.detail)
            );
            let n = self
                .seen
                .entry((name.to_owned(), stem.clone()))
                .or_insert(0);
            *n += 1;
            self.found.push(Found {
                slot: Slot {
                    trace,
                    report,
                    place,
                    finding: finding_index,
                },
                fingerprint: format!("{stem}:{n}"),
                scored,
            });
        }
    }
}

/// Every fingerprint of a run, computed once and addressed by [`Slot`].
pub(crate) struct Fingerprints<'r> {
    rule_ids: BTreeMap<(&'r str, &'r str), String>,
    found: Vec<Found>,
    index: HashMap<Slot, usize>,
}

impl<'r> Fingerprints<'r> {
    /// The fingerprints of the findings of `traces`: each trace's name as the
    /// caller gave it (`None` for stdin) and its reports.
    ///
    /// Visits, per report, the failing and warning requirement rows in registry
    /// order, then a failed wire schema, then the extensions' failing and
    /// warning rows — the order SARIF writes its results in.
    pub(crate) fn of(traces: &[(Option<&str>, &'r [Report])]) -> Self {
        let rule_ids = rule_ids(traces.iter().flat_map(|(_, reports)| reports.iter()));
        let mut counter = Counter::default();
        for (trace, (name, reports)) in traces.iter().enumerate() {
            for (report_index, report) in reports.iter().enumerate() {
                let mut visit = |place: Place, id: &str, findings: &[Finding], scored: bool| {
                    let rule_id = &rule_ids[&(id, report.revision.as_str())];
                    counter.list(
                        (name.unwrap_or_default(), rule_id),
                        (trace, report_index, place),
                        findings,
                        scored,
                    );
                };
                for (row_index, row) in report.requirements.iter().enumerate() {
                    if matches!(row.outcome, Outcome::Fail | Outcome::Warn) {
                        visit(Place::Row(row_index), &row.id, &row.findings, true);
                    }
                }
                if let Some(schema) = report
                    .wire_schema
                    .as_ref()
                    .filter(|_| report.wire_schema_failed())
                {
                    visit(Place::WireSchema, WIRE_SCHEMA_RULE, &schema.findings, true);
                }
                for (extension_index, extension) in report.extensions.iter().enumerate() {
                    for (row_index, row) in extension.requirements.iter().enumerate() {
                        if matches!(row.outcome, Outcome::Fail | Outcome::Warn) {
                            let place = Place::Extension {
                                extension: extension_index,
                                row: row_index,
                            };
                            visit(place, &row.id, &row.findings, false);
                        }
                    }
                }
            }
        }
        let found = counter.found;
        let index = found
            .iter()
            .enumerate()
            .map(|(position, one)| (one.slot, position))
            .collect();
        Self {
            rule_ids,
            found,
            index,
        }
    }

    /// The rule ID of requirement `id` judged at `revision`: `id` itself unless
    /// it has findings under more than one revision. A row with no finding has
    /// no rule of its own, and gets `id`.
    pub(crate) fn rule_id(&self, id: &str, revision: &str) -> String {
        self.rule_ids
            .get(&(id, revision))
            .map_or_else(|| id.to_owned(), Clone::clone)
    }

    /// The fingerprint of the finding at `slot`.
    pub(crate) fn get(&self, slot: Slot) -> Option<&str> {
        let position = *self.index.get(&slot)?;
        Some(self.found[position].fingerprint.as_str())
    }

    /// Every finding of the run with its fingerprint, in the order visited.
    pub(crate) fn found(&self) -> &[Found] {
        &self.found
    }
}

/// The rule ID of each `(requirement ID, revision)` with a finding: the
/// requirement ID, suffixed with its revision only where the same ID has
/// findings under more than one.
fn rule_ids<'r>(reports: impl Iterator<Item = &'r Report>) -> BTreeMap<(&'r str, &'r str), String> {
    let mut revisions_of: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for report in reports {
        let schema = report.wire_schema_failed().then_some(WIRE_SCHEMA_RULE);
        let extension_rows = report
            .extensions
            .iter()
            .flat_map(|extension| &extension.requirements);
        let failing = report
            .requirements
            .iter()
            .chain(extension_rows)
            .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
            .map(|row| row.id.as_str());
        for id in failing.chain(schema) {
            let revisions = revisions_of.entry(id).or_default();
            // Several traces judged at one revision are still one revision.
            if !revisions.contains(&report.revision.as_str()) {
                revisions.push(report.revision.as_str());
            }
        }
    }
    let mut ids = BTreeMap::new();
    for (id, revisions) in revisions_of {
        for revision in &revisions {
            let rule_id = if revisions.len() > 1 {
                format!("{id}@{revision}")
            } else {
                id.to_owned()
            };
            ids.insert((id, *revision), rule_id);
        }
    }
    ids
}

/// `text` with every run of ASCII digits replaced by `#`, so a finding's
/// identity does not depend on the `seq` and ids a particular recording carries.
pub(crate) fn fold_numbers(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut in_digits = false;
    for character in text.chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                folded.push('#');
            }
            in_digits = true;
        } else {
            folded.push(character);
            in_digits = false;
        }
    }
    folded
}

#[cfg(test)]
mod tests;
