// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The ledger of check ids the registry names ahead of their implementation,
//! and the invariant that binds it to [`super::ALL`] in both directions.
//!
//! Split from [`super`] when the check list crossed the 500-line cap
//! (`docs/plan/04-engineering-standards.md`), at the seam the module doc already
//! named: the list and its lookup are one concern, this ledger and the agreement
//! test are another. The list grows with every extracted area; this file does
//! not.
//!
//! Test-only in its entirety. Nothing here ships in a build — the ledger exists
//! to make an *absence* legible during review, not to change what the engine
//! does with it (the engine's `unsupported` verdict already does that).

use super::{ALL, find};
use std::collections::HashSet;

/// Whether `check` is a committed-but-unimplemented id (see [`PLANNED`]).
// Const-evaluable only with the feature off, where the body is a literal `false`;
// with it on, the slice lookup is not const. Following the lint would make the
// signature depend on which features are enabled.
#[allow(clippy::missing_const_for_fn)]
fn is_planned(check: &str) -> bool {
    #[cfg(feature = "draft-2026-07-28")]
    let planned = PLANNED.contains(&check);
    #[cfg(not(feature = "draft-2026-07-28"))]
    let planned = {
        let _ = check;
        false
    };
    planned
}

#[cfg(feature = "draft-2026-07-28")]
/// Checks a registry entry names that this build does not implement yet.
///
/// The engine reports such a requirement as `unsupported` — first-class in the
/// totals, listed with the missing id, outranking pass/fail — so an entry naming
/// one states something true (the clause is verified by this check) alongside a
/// visible build fact (the check is absent). That is the mechanism incremental
/// extraction is meant to use, and it is why an entry must never be given a
/// placeholder *exclusion* instead.
///
/// The list exists so the mechanism cannot hide a typo: a misspelled check id
/// would otherwise degrade silently to `unsupported` and read as planned work.
/// Every row is a commitment, and the test below retires each one the moment its
/// check lands.
///
/// Empty is the healthy state, and the state it is in now: every check the
/// registry names is implemented. The list stays because the next extracted area
/// will land its entries before its checks, and this is where that debt is
/// declared rather than left to read as a typo.
const PLANNED: &[&str] = &[];

#[test]
#[allow(clippy::unwrap_used)]
fn builtin_registry_and_check_inventory_cover_each_other_exactly() {
    // Every check the registry references exists, and every implemented check is
    // referenced — drift in either direction is a defect, not a warning.
    //
    // Driven from the registry *set*, not one revision: checks arrive with the
    // revision that needs them, so a `2026-07-28` check is referenced only by that
    // revision's entries. Both halves still bind — an implemented check no
    // revision names is dead code, and a named check nothing implements would be
    // reported `unsupported` rather than judged.
    use mcp_conformance_core::requirement::Verification;

    let set = mcp_conformance_core::requirement::RegistrySet::builtin().unwrap();
    let mut referenced = HashSet::new();
    for requirement in set.requirements() {
        if let Verification::Checks { checks } = &requirement.verification {
            for check in checks {
                assert!(
                    find(check).is_some() || is_planned(check),
                    "{}: references check {check}, which is neither implemented nor \
                     listed in PLANNED — a typo in a check id would otherwise be \
                     invisible, reported as `unsupported` rather than as a defect",
                    requirement.id
                );
                referenced.insert(check.clone());
            }
        }
    }
    for check in ALL {
        assert!(
            referenced.contains(check.id),
            "check {} is implemented but referenced by no requirement",
            check.id
        );
    }
    // The list retires itself: implementing a planned check without removing
    // its row fails here, so PLANNED can never quietly outlive its purpose.
    // Feature-gated with the data — without it, the revision that names these
    // is not described and nothing could reference them.
    #[cfg(feature = "draft-2026-07-28")]
    for planned in PLANNED {
        assert!(
            find(planned).is_none(),
            "check {planned} is implemented — remove it from PLANNED"
        );
        assert!(
            referenced.contains(*planned),
            "check {planned} is planned but no requirement references it"
        );
    }
}
