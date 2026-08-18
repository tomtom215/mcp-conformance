// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The suite-pin currency gate: neither pinned conformance-suite version may
//! silently fall behind the dist-tag it tracks.
//!
//! Two pins exist, with different triggers
//! ([03-conformance-strategy](../../docs/plan/03-conformance-strategy.md)
//! §"Official-suite version policy"): [`crate::conformance::SUITE_VERSION`] gates the
//! released revision and follows npm's `latest`, and
//! [`crate::draft_readiness::DRAFT_SUITE_VERSION`] gates the readiness ratchet and
//! follows `alpha`. Both are exact, deliberately — a gate whose input can move
//! underneath it is not a gate — and both are bumped by a considered PR that
//! re-measures and reviews the scenario diff.
//!
//! Exactness costs nothing. *Not noticing* costs: the draft pin sat on
//! `0.2.0-alpha.9` for six weeks while `alpha.10` and `alpha.11` shipped, and
//! `alpha.11` added a `wire-schema-valid` check the ratchet had no way to see.
//! The re-check that would have caught it existed only as a dated line in the
//! deferral ledger — a claim that something would be looked at, which is the
//! shape of claim [ADR-0010](../../docs/plan/decisions/0010-deferral-ledger-and-scheduled-reverification.md)
//! exists to distrust.
//!
//! So the noticing is a gate and the bumping stays a decision. This task asks
//! npm what the two dist-tags currently point at and fails when either differs
//! from its pin. It runs in the weekly `claims-expire` job, beside the ledger
//! and quote gates it belongs with: an upstream release is a maintenance event,
//! not a defect in the pull request that happens to be open, so it pages the
//! schedule rather than blocking unrelated work — and a red weekly run now
//! files a tracking issue, so the news arrives whether or not anyone reads
//! their CI mail.
//!
//! Network, like `spec-drift` and `conformance`: orchestration may dial out,
//! `cargo test` never does.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use crate::conformance::{SUITE_PACKAGE, SUITE_VERSION};
use crate::draft_readiness::DRAFT_SUITE_VERSION;

/// npm's dist-tag endpoint for one package — the whole packument is megabytes
/// and this is the two lines the gate reads.
const DIST_TAGS: &str =
    "https://registry.npmjs.org/-/package/@modelcontextprotocol%2Fconformance/dist-tags";

/// One pin and the dist-tag it tracks.
struct Pin {
    /// The dist-tag npm serves it under.
    tag: &'static str,
    /// The pinned version, as committed.
    pinned: &'static str,
    /// Where the constant lives, so the failure names the edit.
    site: &'static str,
    /// What bumping it entails, per the version policy.
    procedure: &'static str,
}

/// The pins this gate holds, in the order the policy document introduces them.
const PINS: &[Pin] = &[
    Pin {
        tag: "latest",
        pinned: SUITE_VERSION,
        site: "SUITE_VERSION in xtask/src/conformance.rs",
        procedure: "bump it with a scenario diff review, refresh both expected-failures \
                    baselines and the agreement divergence ledger, and update register row 2.4",
    },
    Pin {
        tag: "alpha",
        pinned: DRAFT_SUITE_VERSION,
        site: "DRAFT_SUITE_VERSION in xtask/src/draft_readiness.rs",
        procedure: "bump it with a scenario diff review and a `BLESS=1 cargo xtask \
                    draft-readiness` re-measurement in the same commit",
    },
];

/// Runs the gate; `true` when both pins equal the dist-tags they track.
pub(crate) fn run() -> bool {
    eprintln!("xtask: suite-currency — asking npm what {SUITE_PACKAGE} currently tags");
    let body = match fetch(DIST_TAGS) {
        Ok(body) => body,
        Err(error) => {
            // A fetch failure is a failure: an unchecked pin is not a checked
            // one, the same rule `spec-drift` applies to an unfetched page.
            eprintln!("xtask: suite-currency — cannot read {DIST_TAGS}: {error}");
            return false;
        }
    };
    let tags: std::collections::BTreeMap<String, String> = match serde_json::from_str(&body) {
        Ok(tags) => tags,
        Err(error) => {
            eprintln!("xtask: suite-currency — {DIST_TAGS} is not a tag map: {error}");
            return false;
        }
    };
    let mut ok = true;
    for complaint in complaints(PINS, &tags) {
        eprintln!("xtask: suite-currency — {complaint}");
        ok = false;
    }
    if ok {
        for pin in PINS {
            eprintln!(
                "xtask: suite-currency — {SUITE_PACKAGE}@{} is {}, and {} pins it",
                pin.tag, pin.pinned, pin.site
            );
        }
    }
    ok
}

/// Everything the served dist-tags say about the pins that is not "unchanged".
///
/// Separate from the fetch so the comparison is testable without a network,
/// which is also the reason it takes the pins rather than reading [`PINS`].
fn complaints(pins: &[Pin], tags: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    let mut complaints = Vec::new();
    for pin in pins {
        let Some(served) = tags.get(pin.tag) else {
            complaints.push(format!(
                "npm serves no `{}` dist-tag for {SUITE_PACKAGE}; the pin cannot be \
                 checked against a tag that does not exist",
                pin.tag
            ));
            continue;
        };
        // Inequality, not "newer than": a dist-tag moved backwards (an
        // unpublish, a mistaken re-tag) is news too, and detecting it needs no
        // semver comparison and so no dependency to do the comparing.
        if served != pin.pinned {
            complaints.push(format!(
                "{SUITE_PACKAGE}@{} is now {served}; {} still pins {}. Re-decide: {}. \
                 Holding the pin is a legitimate outcome — record it where the pin lives, \
                 not by leaving this red",
                pin.tag, pin.site, pin.pinned, pin.procedure
            ));
        }
    }
    complaints
}

/// Fetches one URL via curl — a checked tool dependency CI runners already
/// have, in the same shape `spec-drift` uses.
fn fetch(url: &str) -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args(["-sSf", "--max-time", "30", url])
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "curl exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "response is not UTF-8".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(tag, version)| ((*tag).to_owned(), (*version).to_owned()))
            .collect()
    }

    #[test]
    fn pins_equal_to_their_dist_tags_produce_no_complaint() {
        let served = tags(&[("latest", SUITE_VERSION), ("alpha", DRAFT_SUITE_VERSION)]);
        assert_eq!(complaints(PINS, &served), Vec::<String>::new());
    }

    #[test]
    fn a_moved_dist_tag_names_the_constant_and_the_procedure() {
        let served = tags(&[("latest", "0.2.0"), ("alpha", DRAFT_SUITE_VERSION)]);
        let complaints = complaints(PINS, &served);
        assert_eq!(complaints.len(), 1, "{complaints:?}");
        assert!(complaints[0].contains("0.2.0"), "{}", complaints[0]);
        assert!(complaints[0].contains("SUITE_VERSION"), "{}", complaints[0]);
        assert!(complaints[0].contains("agreement"), "{}", complaints[0]);
    }

    #[test]
    fn a_dist_tag_moved_backwards_is_news_too() {
        // Not "is the served version newer": an unpublish or a mistaken re-tag
        // leaves the pin describing something npm no longer serves.
        let served = tags(&[("latest", SUITE_VERSION), ("alpha", "0.2.0-alpha.1")]);
        let complaints = complaints(PINS, &served);
        assert_eq!(complaints.len(), 1, "{complaints:?}");
        assert!(complaints[0].contains("0.2.0-alpha.1"), "{}", complaints[0]);
    }

    #[test]
    fn a_missing_dist_tag_fails_rather_than_passing_vacuously() {
        let complaints = complaints(PINS, &tags(&[("latest", SUITE_VERSION)]));
        assert_eq!(complaints.len(), 1, "{complaints:?}");
        assert!(
            complaints[0].contains("no `alpha` dist-tag"),
            "{}",
            complaints[0]
        );
    }

    #[test]
    fn both_pins_can_be_stale_at_once_and_both_are_reported() {
        let served = tags(&[("latest", "9.9.9"), ("alpha", "9.9.9-alpha.1")]);
        assert_eq!(complaints(PINS, &served).len(), 2);
    }

    #[test]
    fn the_endpoint_names_the_package_the_pins_are_for() {
        // The URL percent-encodes the scope separator, so the two spellings
        // cannot be compared directly; this pins that they still agree.
        assert!(
            DIST_TAGS.contains(&SUITE_PACKAGE.replace('/', "%2F")),
            "{DIST_TAGS}"
        );
    }
}
