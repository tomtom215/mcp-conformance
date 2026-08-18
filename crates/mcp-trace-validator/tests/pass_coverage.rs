// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Every judged clause needs a trace that **passes** it, not only one that kills it.
//!
//! `golden.rs` holds two halves of the corpus contract already: every implemented
//! check has a violation trace that falsifies it, and every check finds subjects
//! on some trace. Both are satisfied by a check that fires on *everything* it
//! examines — its violation trace kills it, its subject count is non-zero, and no
//! conforming trace ever exercises it, because none carries the clause's subject
//! matter. Nothing proves such a check accepts a conforming session.
//!
//! That blind spot is what the corpus's shape makes likely: at `2026-07-28` there
//! are 72 authored violation traces against 2 authored conforming ones. A
//! violation trace is cheap — one message, one clause — while a conforming trace
//! has to carry a whole plausible session, so violations accumulate and passes do
//! not. This measures the consequence rather than assuming it away.
//!
//! It found a real defect on its first run. `base.result-field` counted only
//! malformed responses as subjects, so `BASE-010`/`BASE-047` could report `fail`
//! or `not observed` and never `pass` — a session carrying dozens of well-formed
//! results was told none of its traffic bound to the clause. Fixing that moved
//! capture coverage from 109 to 110 of the 124 judgeable clauses without a single
//! trace being written.
//!
//! The ledger below is exact in both directions: a clause that loses its passing
//! evidence must be added, and one that gains it must be retired. Reading the
//! committed goldens rather than re-validating is deliberate — `golden.rs`
//! already proves those files are what the engine produces, so this test asks a
//! question about the corpus, not about the engine.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mcp_conformance_core::requirement::{Registry, Verification};

/// Judged clauses that no committed trace reports as passing, and why not.
///
/// Every row is a conforming trace nobody has written yet, not a defect — but a
/// row is a debt, and the list is meant to shrink. Retiring one means adding a
/// conforming trace that carries the clause's subject matter.
const WITHOUT_A_PASSING_TRACE: &[(&str, &str)] = &[
    // `2026-07-28`. The shipped revision has no rows: `stdio-feature-session`
    // carries a conforming `_meta` key and a tool result embedding a resource,
    // which was the whole of its debt.
    (
        "CACH-015",
        "needs a paginated list whose pages agree on `cacheScope`; this server's \
         surface has no pagination, so only the violating shape is authored",
    ),
    (
        "CACH-016",
        "shares `caching.page-scope-consistent` with CACH-015",
    ),
    (
        "DISC-002",
        "needs a dual-era client that probes with `server/discover` first; the \
         corpus has the client that skips the probe, not the one that sends it",
    ),
    (
        "TRAN-128",
        "shares `discover.dual-era-probe-first` with DISC-002",
    ),
    (
        "MRTR-024",
        "needs a server that re-asks after a client's input shortfall; the \
         authored round answers `-32602` instead, which is the violation",
    ),
    (
        "PROM-017",
        "needs a prompt carrying audio content, which this reference server does \
         not serve",
    ),
    (
        "TOOL-034",
        "needs an `x-mcp-header`-annotated integer argument inside the IEEE 754 \
         safe range; the server has no annotated tool, so only the out-of-range \
         violation is authored",
    ),
    (
        "TRAN-096",
        "needs a server rejecting a malformed `Mcp-Param-{Name}` value; the \
         authored trace is the server that accepts one",
    ),
    (
        "TRAN-070",
        "a MUST NOT whose conforming case is a server that sends nothing after \
         its response stream closed — the check's subject is a message that \
         should not exist, so a session has to close a stream mid-request to \
         exercise it at all",
    ),
    (
        "TRAN-124",
        "the same shape as TRAN-070 over stdio, where cancellation is a \
         notification rather than a stream close",
    ),
    (
        "VERS-004",
        "needs client capabilities advertising a correctly prefixed extension \
         identifier; the only trace that advertises one omits the prefix, which \
         is the violation",
    ),
];

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Every requirement ID some committed golden under `directory` reports as
/// passing. Sub-directories are not walked: `corpus/golden/draft` is the other
/// revision, and `corpus/golden/exclusions` is a per-revision ledger rather than
/// a trace's report.
fn passing_ids(directory: &Path) -> BTreeSet<String> {
    #[derive(serde::Deserialize)]
    struct Golden {
        requirements: Vec<Row>,
    }
    #[derive(serde::Deserialize)]
    struct Row {
        id: String,
        outcome: String,
    }

    let mut passing = BTreeSet::new();
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
    let mut seen = 0_usize;
    for entry in entries {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        seen += 1;
        let text = fs::read_to_string(&path).unwrap();
        let golden: Golden = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{} is not a golden report: {error}", path.display()));
        for row in golden.requirements {
            if row.outcome == "pass" {
                passing.insert(row.id);
            }
        }
    }
    assert!(seen > 0, "no goldens under {}", directory.display());
    passing
}

/// The IDs `registry` judges by a named check — the ones a trace can pass.
fn judged_ids(registry: &Registry) -> BTreeSet<String> {
    registry
        .requirements()
        .iter()
        .filter(|requirement| matches!(requirement.verification, Verification::Checks { .. }))
        .map(|requirement| requirement.id.to_string())
        .collect()
}

/// The gap for one revision: judged clauses with no passing evidence.
fn gap(registry: &Registry, goldens: &Path) -> BTreeSet<String> {
    &judged_ids(registry) - &passing_ids(goldens)
}

#[test]
fn the_ledger_of_clauses_without_a_passing_trace_is_exact() {
    let mut measured = gap(
        &Registry::builtin_2025_11_25().unwrap(),
        &corpus_root().join("golden"),
    );
    #[cfg(feature = "draft-2026-07-28")]
    {
        use mcp_conformance_core::requirement::RegistrySet;
        let draft = RegistrySet::builtin()
            .unwrap()
            .registry("2026-07-28".parse().unwrap())
            .expect("the draft feature describes 2026-07-28");
        measured.extend(gap(&draft, &corpus_root().join("golden/draft")));
    }

    let ledger: BTreeSet<String> = WITHOUT_A_PASSING_TRACE
        .iter()
        .map(|&(id, _)| id.to_owned())
        .filter(|id| cfg!(feature = "draft-2026-07-28") || id_is_shipped(id))
        .collect();

    let unlisted: Vec<&String> = measured.difference(&ledger).collect();
    assert!(
        unlisted.is_empty(),
        "these judged clauses have no trace that passes them, and no ledger row \
         says why — add a conforming trace, or a row: {unlisted:#?}"
    );
    let retired: Vec<&String> = ledger.difference(&measured).collect();
    assert!(
        retired.is_empty(),
        "these clauses now have a passing trace; delete their ledger rows: {retired:#?}"
    );
}

/// Whether `id` belongs to the always-built `2025-11-25` registry.
///
/// Without the draft feature the second revision's goldens are still on disk but
/// its registry is not loadable, so its rows are not measurable and are skipped
/// rather than reported as retired.
fn id_is_shipped(id: &str) -> bool {
    judged_ids(&Registry::builtin_2025_11_25().unwrap()).contains(id)
}

#[test]
fn every_ledger_row_names_a_judged_clause_and_gives_a_reason() {
    let mut ids: Vec<&str> = WITHOUT_A_PASSING_TRACE.iter().map(|&(id, _)| id).collect();
    let unique: BTreeSet<&str> = ids.iter().copied().collect();
    assert_eq!(unique.len(), ids.len(), "duplicate ledger row");
    ids.sort_unstable();
    for &(id, reason) in WITHOUT_A_PASSING_TRACE {
        assert!(
            reason.len() > 30,
            "{id}'s row must say what conforming trace is missing, not just that one is"
        );
    }
}
