// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The readiness shape: `41 passing / 0 failing / 0 informational`, as prose
//! quotes what the official runner scored against the everything server.
//!
//! `draft-readiness` ratchets that measurement in CI — any change in either
//! direction fails — but the ratchet guards the *baseline file*, and every
//! document that quotes it kept the number by hand. It rotted exactly where
//! that predicts: when the suite pin moved `alpha.9` → `alpha.11` on
//! 2026-08-18 the legs separated and the plan documents were updated, while the
//! root README went on saying the scenarios pass `23/23` — the superseded
//! figure, in the most-read file in the repository.
//!
//! So the readiness numbers are held to the same rule as the corpus counts: a
//! pair stated in prose must be one the committed baseline produced, and a pair
//! that is being *quoted* rather than asserted goes in backticks, where
//! [`super::prose`] blanks it. That distinction is what lets these documents
//! keep their history — the roadmap's "first measurement" and the register's
//! superseded `alpha.9` score are the point of those rows, not stale text to be
//! swept away.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::Deserialize;

/// The committed measurement, as `draft-readiness` writes it.
#[derive(Debug, Deserialize)]
pub(super) struct Baseline {
    /// Every check's status, keyed `<served revision>/<scenario>`.
    scenarios: BTreeMap<String, BTreeMap<String, String>>,
}

/// One `<passing> passing / <failing> failing` pair as prose quotes it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Score {
    passing: u32,
    failing: u32,
    /// Quoted only when the sentence needs it.
    informational: Option<u32>,
    line: usize,
}

impl Baseline {
    /// Reads the committed baseline.
    pub(super) fn load(root: &Path) -> Result<Self, String> {
        let path = root.join(PATH);
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        serde_json::from_str(&text).map_err(|error| format!("cannot parse {PATH}: {error}"))
    }

    /// Every score a document may state — each leg's, and the whole run's — as
    /// `(passing, failing, informational)`.
    ///
    /// Both granularities, because prose quotes both: the register reports the
    /// legs separately and the roadmap has said "both score" when they agreed.
    fn scores(&self) -> BTreeSet<(u32, u32, u32)> {
        let mut legs: BTreeMap<&str, (u32, u32, u32)> = BTreeMap::new();
        let mut run = (0, 0, 0);
        for (key, checks) in &self.scenarios {
            let revision = key.split('/').next().unwrap_or(key);
            let leg = legs.entry(revision).or_default();
            for status in checks.values() {
                // Anything the runner reports that is neither a success nor a
                // failure is informational — the baseline keeps statuses
                // verbatim, and a denominator that absorbs INFO is the vacuous
                // accounting this project refuses.
                let (in_leg, in_run) = match status.as_str() {
                    SUCCESS => (&mut leg.0, &mut run.0),
                    FAILURE => (&mut leg.1, &mut run.1),
                    _ => (&mut leg.2, &mut run.2),
                };
                *in_leg += 1;
                *in_run += 1;
            }
        }
        legs.into_values().chain([run]).collect()
    }
}

const PATH: &str = "conformance/draft-readiness.json";
const SUCCESS: &str = "SUCCESS";
const FAILURE: &str = "FAILURE";

/// The labels a readiness score is written with, in the order prose writes them.
const LABELS: [&str; 3] = ["passing", "failing", "informational"];

/// Verifies every readiness score stated in `text`; `true` when all agree.
pub(super) fn check(name: &str, text: &str, baseline: &Baseline) -> bool {
    let scores = baseline.scores();
    let mut ok = true;
    for score in claims(text) {
        let matched = scores.iter().any(|&(passing, failing, informational)| {
            passing == score.passing
                && failing == score.failing
                && score
                    .informational
                    .is_none_or(|quoted| quoted == informational)
        });
        if matched {
            continue;
        }
        eprintln!(
            "xtask: draft-coverage — {name}:{} states a readiness score of {} passing / {} failing \
             that {PATH} does not record; it measured {}",
            score.line,
            score.passing,
            score.failing,
            scores
                .iter()
                .map(|&(pass, fail, info)| format!("{pass}/{fail}/{info}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        ok = false;
    }
    ok
}

/// Every readiness score in `text`, with 1-based line numbers.
fn claims(text: &str) -> Vec<Score> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let plain = line.replace('*', "");
        let counts = super::verdict::counts_of(&plain, &LABELS);
        let mut at = 0;
        while at < counts.len() {
            let (label, passing) = counts[at];
            at += 1;
            if label != "passing" {
                continue;
            }
            let Some(&("failing", failing)) = counts.get(at) else {
                continue;
            };
            at += 1;
            let informational = match counts.get(at) {
                Some(&("informational", count)) => {
                    at += 1;
                    Some(count)
                }
                _ => None,
            };
            found.push(Score {
                passing,
                failing,
                informational,
                line: index + 1,
            });
        }
    }
    found
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn baseline() -> Baseline {
        serde_json::from_str(
            r#"{"scenarios": {
                "2025-11-25/a": {"a": "SUCCESS", "wire": "FAILURE"},
                "2025-11-25/b": {"b": "SUCCESS"},
                "2026-07-28/a": {"a": "SUCCESS", "wire": "SUCCESS"},
                "2026-07-28/b": {"b": "SUCCESS"}
            }}"#,
        )
        .unwrap()
    }

    #[test]
    fn each_leg_and_the_whole_run_are_statable() {
        // Legs are 2/1 and 3/0; the run is 5/1. All three are things a document
        // legitimately says, and nothing else is.
        assert_eq!(
            baseline().scores(),
            BTreeSet::from([(2, 1, 0), (3, 0, 0), (5, 1, 0)])
        );
    }

    #[test]
    fn a_score_is_read_out_of_the_shapes_prose_uses() {
        let text = "\
the legs separate — **37 passing / 4 failing** for the legacy leg
both score **23 passing / 0 failing / 0 informational across 20 scenarios**
scores 41 passing, 0 failing against the stateless one
";
        let found = claims(text);
        assert_eq!(
            found
                .iter()
                .map(|s| (s.passing, s.failing, s.informational, s.line))
                .collect::<Vec<_>>(),
            vec![(37, 4, None, 1), (23, 0, Some(0), 2), (41, 0, None, 3),]
        );
    }

    #[test]
    fn prose_that_states_no_score_is_left_alone() {
        for text in [
            // Half a score is not one.
            "1 passing scenario, and the rest were skipped\n",
            // The label must end a word.
            "3 passings\n",
            // A number that belongs to the noun, not the label.
            "across 20 scenarios, failing\n",
        ] {
            assert!(claims(text).is_empty(), "{text:?} parsed as a score");
        }
    }

    #[test]
    fn a_superseded_score_fails_unless_it_is_quoted_as_one() {
        let baseline = baseline();
        // Asserted in prose: checked, and 23/0 is not what was measured.
        assert!(!check(
            "f.md",
            "both score 23 passing / 0 failing\n",
            &baseline
        ));
        // Quoted as a specimen: `prose` blanks it before the check sees it.
        let quoted = super::super::prose::without_code("it read `23 passing / 0 failing`\n");
        assert!(check("f.md", &quoted, &baseline));
        // And a real leg passes either way.
        assert!(check(
            "f.md",
            "the legacy leg scores 2 passing / 1 failing\n",
            &baseline
        ));
        // A quoted informational count is checked too when the sentence gives
        // one, so "2 passing / 1 failing / 3 informational" is not a free pass.
        assert!(check(
            "f.md",
            "2 passing / 1 failing / 0 informational\n",
            &baseline
        ));
        assert!(!check(
            "f.md",
            "2 passing / 1 failing / 3 informational\n",
            &baseline
        ));
    }
}
