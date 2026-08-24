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
//! It opened with fourteen rows and holds none. Closing the last of them turned
//! up the shape worth remembering: for four clauses the conforming case *cannot*
//! be a conforming trace. A `MUST NOT` is witnessed only by a session carrying a
//! permitted message where the forbidden one would be; a server obliged to
//! reject something is witnessed only by a client that sent the something; and a
//! client that probes before falling back is witnessed only against a server
//! that refused the probe. Each of those pass paths lives in a violation trace,
//! which is not a compromise — it is where the antecedent actually occurs.
//!
//! The ledger is exact in both directions: a clause that loses its passing
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
/// A row is a conforming trace nobody has written yet, not a defect — but it is
/// a debt, and retiring one means adding a trace that carries the clause's
/// subject matter.
///
/// Empty, and that is the point of it still existing: the list shrank from
/// fourteen to nothing, and a row appearing again means a check has been added
/// without a conforming trace, or one has lost the trace that proved it accepts
/// conforming input.
const WITHOUT_A_PASSING_TRACE: &[(&str, &str)] = &[];

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

/// Each revision's registry paired with the goldens judged against it.
///
/// The draft half is a separate `cfg`-selected list rather than a conditional
/// push, so the expression type-checks and lints identically in both feature
/// modes — `--no-default-features` clippy is a gate leg of its own, and it is
/// the one that catches a `mut` or a `Vec::new` that only one mode justifies.
fn corpora() -> Vec<(Registry, PathBuf)> {
    #[cfg(feature = "draft-2026-07-28")]
    let draft = vec![(
        mcp_conformance_core::requirement::RegistrySet::builtin()
            .unwrap()
            .registry("2026-07-28".parse().unwrap())
            .expect("the draft feature describes 2026-07-28"),
        corpus_root().join("golden/draft"),
    )];
    #[cfg(not(feature = "draft-2026-07-28"))]
    let draft = Vec::new();

    core::iter::once((
        Registry::builtin_2025_11_25().unwrap(),
        corpus_root().join("golden"),
    ))
    .chain(draft)
    .collect()
}

#[test]
fn the_ledger_of_clauses_without_a_passing_trace_is_exact() {
    let measured: BTreeSet<String> = corpora()
        .iter()
        .flat_map(|(registry, goldens)| gap(registry, goldens))
        .collect();

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

/// The shape a row must have, checked whether or not any exist today.
///
/// It did not check the half its name claims — that the id is a clause this
/// registry actually judges — and a row naming a typo'd or excluded id would
/// have sat in the list forever looking like acknowledged debt while measuring
/// nothing.
#[test]
fn every_ledger_row_names_a_judged_clause_and_gives_a_reason() {
    let judged: BTreeSet<String> = corpora()
        .iter()
        .flat_map(|(registry, _)| judged_ids(registry))
        .collect();
    let ids: BTreeSet<&str> = WITHOUT_A_PASSING_TRACE.iter().map(|&(id, _)| id).collect();
    assert_eq!(
        ids.len(),
        WITHOUT_A_PASSING_TRACE.len(),
        "duplicate ledger row"
    );
    for &(id, reason) in WITHOUT_A_PASSING_TRACE {
        assert!(
            judged.contains(id),
            "{id} is not a clause any built registry judges, so no trace could pass it"
        );
        assert!(
            reason.len() > 30,
            "{id}'s row must say what conforming trace is missing, not just that one is"
        );
    }
}
