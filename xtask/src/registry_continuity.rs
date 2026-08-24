// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Registry continuity: a clause carried across revisions, entered twice, must
//! be entered the same way twice.
//!
//! Requirement ids are unique across the whole set rather than per revision, so
//! a sentence the specification keeps unchanged gets a *new* entry at the new
//! revision — `BASE-001` and `BASE-043` are one clause under two ids. The two
//! entries are then two independent readings of the same sentence, written
//! months apart, and until 2026-08-21 nothing compared them.
//!
//! Four had drifted. `PAGE-003` said a trace cannot judge "invalid cursors
//! SHOULD result in `-32602`" while `PAGE-011`, the same sentence, judged it
//! with a check — a coverage gap that had to be found by hand. Three more
//! disagreed about which party the clause binds: `LOG-002`/`LOG-012`,
//! `TOOL-013`/`TOOL-028`, `TOOL-015`/`TOOL-017`. Nothing reads `actor`, which
//! is exactly why nothing noticed.
//!
//! **What is compared, and what is not.** Level, actor, and whether the clause
//! is judged or excluded. Not the check *ids*: the same clause can need a
//! different implementation at a different revision, and five pairs legitimately
//! do — `2026-07-28` reads capability declarations off `server/discover` where
//! `2025-11-25` reads them off an `initialize` result, so the capability checks
//! must differ or report a vacuous pass.
//!
//! A level disagreement is always a defect: the RFC 2119 keyword is inside the
//! quote, and the quotes are identical. Actor and verification are judgments,
//! so they get a ledger — with a reason, reviewed once, rather than a silence.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::collections::BTreeMap;

use mcp_conformance_core::requirement::{Registry, RegistrySet, Requirement, Verification};
use mcp_conformance_core::revision::ProtocolRevision;

use crate::spec_drift::quote::normalize;

/// Pairs that deliberately differ, and why.
///
/// Exact in both directions: a new disagreement must be entered with a reason
/// or fixed, and a row whose pair has stopped disagreeing must be retired. A
/// row is a decision someone made on the record — not a way to quiet the gate.
///
/// Empty, and that is the point of it still existing: the four disagreements
/// that existed when this was written were all defects, and the next one has
/// somewhere to be justified rather than nowhere to be noticed.
const JUSTIFIED: &[(&str, &str, &str)] = &[];

/// One clause as the registry states it, reduced to the facts both entries owe
/// each other.
struct Entry<'r> {
    id: &'r str,
    level: &'r str,
    actor: String,
    judged: bool,
}

fn entry(requirement: &Requirement) -> Entry<'_> {
    Entry {
        id: requirement.id.as_str(),
        level: requirement.level.keyword(),
        actor: format!("{:?}", requirement.actor).to_lowercase(),
        judged: matches!(requirement.verification, Verification::Checks { .. }),
    }
}

/// The key two entries of one clause share: its page anchor and its normalized
/// quote. Section as well as quote, because a sentence the specification repeats
/// on two pages is two clauses, entered separately on purpose.
fn key(requirement: &Requirement) -> (String, String) {
    (
        requirement.source.section.clone(),
        normalize(&requirement.source.quote),
    )
}

/// Checks every clause the two revisions share.
pub(crate) fn run() -> bool {
    let set = match RegistrySet::builtin() {
        Ok(set) => set,
        Err(error) => {
            eprintln!("xtask: registry-continuity — registry set: {error}");
            return false;
        }
    };
    let revisions = set.revisions().to_vec();
    let [older, newer] = revisions[..] else {
        eprintln!(
            "xtask: registry-continuity — the set describes {} revision(s), not two; run this \
             task through the `cargo xtask` alias, which enables the draft feature",
            revisions.len()
        );
        return false;
    };
    let (Some(old), Some(new)) = (set.registry(older), set.registry(newer)) else {
        eprintln!("xtask: registry-continuity — the set named a revision it does not describe");
        return false;
    };
    report(&old, older, &new, newer)
}

fn report(
    old: &Registry,
    older: ProtocolRevision,
    new: &Registry,
    newer: ProtocolRevision,
) -> bool {
    let index: BTreeMap<(String, String), &Requirement> =
        new.requirements().iter().map(|r| (key(r), r)).collect();

    let mut problems = Vec::new();
    let mut shared = 0_u32;
    for requirement in old.requirements() {
        let Some(continuation) = index.get(&key(requirement)) else {
            continue;
        };
        shared += 1;
        compare(requirement, continuation, (older, newer), &mut problems);
    }

    // A gate that matched nothing would pass on anything: the two shipped
    // revisions restate dozens of clauses verbatim, so an empty intersection
    // means the key stopped matching, not that the registries agree.
    if shared < 10 {
        eprintln!(
            "xtask: registry-continuity — only {shared} clause(s) matched across revisions; \
             the section/quote key is wrong"
        );
        return false;
    }
    retire_stale_rows(&mut problems, old, new);

    if problems.is_empty() {
        eprintln!(
            "xtask: registry-continuity — {shared} clause(s) entered under both revisions agree \
             on level, actor and judgment"
        );
        true
    } else {
        eprintln!("xtask: registry-continuity — entries for one sentence disagree:");
        for problem in &problems {
            eprintln!("{problem}");
        }
        false
    }
}

/// Compares one clause's two entries, appending a line per disagreement.
fn compare(
    older_entry: &Requirement,
    newer_entry: &Requirement,
    (older, newer): (ProtocolRevision, ProtocolRevision),
    problems: &mut Vec<String>,
) {
    let (before, after) = (entry(older_entry), entry(newer_entry));
    if before.level != after.level {
        // Never ledgered: the keyword is inside the quote, and the quotes are
        // the same string.
        problems.push(format!(
            "  {older}/{} is {} and {newer}/{} is {} — the same sentence cannot be both",
            before.id, before.level, after.id, after.level
        ));
    }
    if JUSTIFIED
        .iter()
        .any(|&(from, to, _)| from == before.id && to == after.id)
    {
        return;
    }
    if before.actor != after.actor {
        problems.push(format!(
            "  {older}/{} binds {} and {newer}/{} binds {} — decide which, or enter the pair in \
             JUSTIFIED with the reason",
            before.id, before.actor, after.id, after.actor
        ));
    }
    if before.judged != after.judged {
        let (judged, excluded) = if before.judged {
            (before.id, after.id)
        } else {
            (after.id, before.id)
        };
        problems.push(format!(
            "  {excluded} carries an exclusion and {judged} judges the same sentence with a check \
             — either the check reaches both, or the exclusion says why not"
        ));
    }
}

/// A ledger row whose pair no longer disagrees is a stale justification, and a
/// row naming a pair that is not a pair at all is a typo that quiets the gate
/// forever.
fn retire_stale_rows(problems: &mut Vec<String>, old: &Registry, new: &Registry) {
    let by_id = |registry: &Registry, id: &str| -> Option<(String, String, bool)> {
        registry
            .requirements()
            .iter()
            .find(|r| r.id.as_str() == id)
            .map(|r| {
                let e = entry(r);
                (e.level.to_owned(), e.actor, e.judged)
            })
    };
    for &(from, to, _) in JUSTIFIED {
        match (by_id(old, from), by_id(new, to)) {
            (Some(before), Some(after)) if before == after => problems.push(format!(
                "  JUSTIFIED names {from}/{to}, which no longer disagree — retire the row"
            )),
            (None, _) | (_, None) => problems.push(format!(
                "  JUSTIFIED names {from}/{to}, which is not a clause pair in the two registries"
            )),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "draft-2026-07-28")]
    fn the_shipped_registries_are_continuous() {
        // The gate, run against the real tree: the assertion that would have
        // caught PAGE-003 and the three actor disagreements.
        assert!(run(), "see the report above");
    }

    #[test]
    fn the_key_separates_the_same_sentence_on_two_pages() {
        // Section as well as quote: the specification repeats sentences, and
        // two pages saying the same thing are two clauses.
        let a = (
            "basic#requests".to_owned(),
            normalize("Requests MUST have."),
        );
        let b = ("basic#results".to_owned(), normalize("Requests MUST have."));
        assert_ne!(a, b);
        // And normalization is what makes a re-wrapped quote still the same one.
        assert_eq!(
            normalize("the client\n**MUST** send it"),
            normalize("the client MUST send it")
        );
    }
}
