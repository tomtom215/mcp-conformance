// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Applying a baseline to the reports of a run.

use std::collections::HashMap;

use super::{Baseline, BaselineError, Date, Entry};
use crate::fingerprint::{Fingerprints, Found, Slot};
use crate::report::{Acceptance, BaselineTally, Report};

/// The reports of one trace, to mark: the trace as the caller named it (`None`
/// for stdin) and one report per judged revision.
#[derive(Debug)]
#[non_exhaustive]
pub struct TraceReports<'a> {
    /// The trace's name. It is part of how findings are counted
    /// ([`crate::fingerprint`]), never of what they are.
    pub name: Option<&'a str>,
    /// The trace's reports.
    pub reports: &'a mut [Report],
}

impl<'a> TraceReports<'a> {
    /// The reports of the trace named `name` (`None` for stdin).
    #[must_use]
    pub const fn new(name: Option<&'a str>, reports: &'a mut [Report]) -> Self {
        Self { name, reports }
    }
}

/// What a baseline says about a run beyond which findings it accepts: the
/// entries that did not do their job.
///
/// Judged over the whole run — an entry matching a finding in any trace is
/// neither stale nor, unless its day has passed, lapsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Notes {
    /// Entries matching no finding of the run: what they accepted no longer
    /// occurs, so they should go. An entry that has lapsed *and* matches
    /// nothing is stale, not lapsed: its finding is not coming back.
    pub stale: Vec<Entry>,
    /// Entries that matched a finding but whose last day has passed: they
    /// accept nothing, and the finding counts as it would without them.
    pub lapsed: Vec<Entry>,
    /// Whether stale entries are allowed (`--allow-stale-baseline`): they are
    /// still listed, but do not fail the run.
    pub allow_stale: bool,
}

impl Notes {
    /// These notes, with stale entries allowed (`true`) or failing (`false`).
    #[must_use]
    pub const fn allowing_stale(mut self, allow: bool) -> Self {
        self.allow_stale = allow;
        self
    }

    /// Whether there is nothing to say.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.stale.is_empty() && self.lapsed.is_empty()
    }

    /// Whether the notes alone fail the run: a stale entry that is not allowed.
    /// (A lapsed entry fails nothing by itself; its finding counts again.)
    #[must_use]
    pub const fn fails(&self) -> bool {
        !self.stale.is_empty() && !self.allow_stale
    }
}

/// The outcome of [`Baseline::apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Applied {
    /// The entries that did not do their job.
    pub notes: Notes,
    /// How the run's findings fell, summed over every report.
    pub tally: BaselineTally,
}

impl Baseline {
    /// Marks the findings of `traces` that this baseline accepts, and sets
    /// each report to count only the rest toward its verdict.
    ///
    /// An entry accepts a finding when its fingerprint is the finding's and
    /// `today` is not past its `expires`; a lapsed entry accepts nothing. Only
    /// the findings that count toward a verdict can be accepted — an
    /// extension's are never scored (ADR-0023), and an entry matching only one
    /// of those is stale. `today` is read by the caller, because the engine
    /// reads no clock; it may be `None` only if no entry has an `expires`.
    ///
    /// # Errors
    ///
    /// [`BaselineError::NoDate`] when an entry has an `expires` and `today` is
    /// `None`: accepting forever what was meant to lapse is the one mistake a
    /// date exists to prevent.
    pub fn apply(
        &self,
        traces: &mut [TraceReports<'_>],
        today: Option<Date>,
    ) -> Result<Applied, BaselineError> {
        if today.is_none() && self.has_expiry() {
            return Err(BaselineError::NoDate);
        }
        // Computed on the reports as they are, then the borrow ends: the marks
        // are written through the slots afterwards.
        let found = {
            let named: Vec<(Option<&str>, &[Report])> = traces
                .iter()
                .map(|trace| (trace.name, &*trace.reports))
                .collect();
            Fingerprints::of(&named).found().to_vec()
        };
        let (matched, lapsed) = self.mark(traces, &found, today);
        let mut tally = BaselineTally::default();
        for trace in traces.iter_mut() {
            for report in trace.reports.iter_mut() {
                report.baseline_applied = true;
                tally = tally.plus(report.baseline_tally());
            }
        }
        Ok(Applied {
            notes: self.notes(&matched, &lapsed),
            tally,
        })
    }

    /// Marks each scored finding of `found` that an entry accepts, writing the
    /// acceptance through the finding's slot. Returns, per entry, whether it
    /// matched any finding and whether it had lapsed as of `today`.
    fn mark(
        &self,
        traces: &mut [TraceReports<'_>],
        found: &[Found],
        today: Option<Date>,
    ) -> (Vec<bool>, Vec<bool>) {
        let by_fingerprint: HashMap<&str, usize> = self
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.fingerprint.as_str(), index))
            .collect();
        let mut matched = vec![false; self.entries().len()];
        let mut lapsed = vec![false; self.entries().len()];
        for one in found.iter().filter(|one| one.scored) {
            let Some(&index) = by_fingerprint.get(one.fingerprint.as_str()) else {
                continue;
            };
            matched[index] = true;
            let entry = &self.entries()[index];
            if today.is_some_and(|today| entry.lapsed(today)) {
                lapsed[index] = true;
                continue;
            }
            let Slot {
                trace,
                report,
                place,
                finding,
            } = one.slot;
            if let Some(target) = place
                .findings_mut(&mut traces[trace].reports[report])
                .and_then(|findings| findings.get_mut(finding))
            {
                target.baselined = Some(Acceptance {
                    reason: entry.reason.clone(),
                    expires: entry.expires.map(|date| date.to_string()),
                });
            }
        }
        (matched, lapsed)
    }

    /// The entries that did not do their job: those that matched nothing, and
    /// those that matched but had lapsed.
    fn notes(&self, matched: &[bool], lapsed: &[bool]) -> Notes {
        let pick = |wanted: &dyn Fn(usize) -> bool| -> Vec<Entry> {
            self.entries()
                .iter()
                .enumerate()
                .filter(|(index, _)| wanted(*index))
                .map(|(_, entry)| entry.clone())
                .collect()
        };
        Notes {
            stale: pick(&|index| !matched[index]),
            lapsed: pick(&|index| matched[index] && lapsed[index]),
            allow_stale: false,
        }
    }
}
