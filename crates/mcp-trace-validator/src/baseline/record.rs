// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Writing a baseline from what a run found.

use std::collections::{HashMap, HashSet};

use super::{Baseline, Entry, GENERATED_REASON};
use crate::fingerprint::Fingerprints;
use crate::report::{Party, Report};

/// A baseline written from a run, and how it relates to the one it refreshes.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Recorded {
    /// The baseline to write.
    pub baseline: Baseline,
    /// Entries of the previous baseline that still match a finding, kept with
    /// their reason and `expires`.
    pub carried: usize,
    /// Findings that had no entry, now accepted with [`GENERATED_REASON`].
    pub added: usize,
    /// Entries of the previous baseline that match nothing and were left out.
    pub dropped: usize,
}

/// Records the findings of `traces` as a baseline.
///
/// A finding is recorded when it counts toward a verdict — a failing or
/// warning row's, or a failed wire schema's, never an extension's — and, under
/// a `view`, is held against that party or against nobody. What `--strict`
/// does to an exit status is no part of it: strictness changes what a finding
/// costs, not what it is.
///
/// With a `previous` baseline (a refresh), each of its entries that matches
/// any finding of the run is kept as it is — reason, `expires` and all, so a
/// refresh never renews an expiry or loses a reviewer's words, and an entry for
/// a finding the `view` sets aside is not lost — and the rest are dropped. A
/// finding without an entry is added with [`GENERATED_REASON`]. Entries are
/// unique by fingerprint: a finding present in several traces is one entry.
#[must_use]
pub fn record(
    traces: &[(Option<&str>, &[Report])],
    view: Option<Party>,
    previous: Option<&Baseline>,
) -> Recorded {
    let known: HashMap<&str, &Entry> = previous
        .into_iter()
        .flat_map(Baseline::entries)
        .map(|entry| (entry.fingerprint.as_str(), entry))
        .collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let found = Fingerprints::of(traces);
    let mut entries = Vec::new();
    let (mut carried, mut added) = (0, 0);
    for one in found.found().iter().filter(|one| one.scored) {
        let (_, reports) = traces[one.slot.trace];
        let counts = one
            .slot
            .place
            .findings(&reports[one.slot.report])
            .and_then(|findings| findings.get(one.slot.finding))
            .is_some_and(|finding| view.is_none_or(|party| finding.counts_for(party)));
        let known = known.get(one.fingerprint.as_str());
        if known.is_none() && !counts {
            continue;
        }
        // A finding present in several traces is one entry, counted once.
        if !seen.insert(one.fingerprint.as_str()) {
            continue;
        }
        if let Some(entry) = known {
            carried += 1;
            entries.push((*entry).clone());
        } else {
            added += 1;
            entries.push(Entry::new(one.fingerprint.clone(), GENERATED_REASON));
        }
    }
    let dropped = previous.map_or(0, |baseline| baseline.entries().len() - carried);
    Recorded {
        baseline: Baseline::new(entries),
        carried,
        added,
        dropped,
    }
}
