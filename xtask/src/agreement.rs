// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The agreement check: our validator's verdicts reconciled with the official
//! runner's, over the very sessions the runner drove.
//!
//! `conformance.rs` taps the everything-server while the pinned suite runs;
//! this module replays every captured session through `mcp-trace-validator`
//! and enforces the policy in docs/plan/03-conformance-strategy.md
//! §Calibration: **zero unexplained divergence**, and a baseline that stays
//! live. The policy itself — what explains a failure, what makes an entry
//! stale, what a baseline must carry — is `mcp_conformance::agreement`, which
//! `mcp-conformance test` reports with for any subject; this module is the
//! repository's gate around it. Explanations live in
//! `conformance/agreement-divergences.json`. The full reconciliation is
//! written to `target/conformance/agreement.json`.
//!
//! The same reconciliation runs for the wire schema (`schema.rs`,
//! `schema_ledger.rs`): the runner's `wire-schema-valid` verdict on every
//! message against the validator's report `wire_schema` member, with
//! divergences explained in `conformance/wire-schema-divergences.json` or
//! failing the gate. It is the `wire_schema` member of `agreement.json`.
//!
//! The same captured sessions also prove the server's exercised surface: the
//! coverage manifest (`conformance/coverage-manifest.json`, `manifest.rs`)
//! records the capabilities the server declared and the methods the suite
//! drove, checked against the registry's capability gates.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::Path;

use mcp_conformance::agreement::{self, Baseline, Reconciliation, Totals, ValidatorFailure};
use mcp_conformance::suite::Summary;
use mcp_conformance_core::requirement::Registry;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::report::Report;
use serde::Serialize;

mod artifacts;
mod manifest;
pub(crate) mod schema;
mod schema_ledger;

pub(crate) use schema_ledger::run_wire_schema_only;
use schema_ledger::session_views;

/// Committed explanations for validator-vs-runner divergences, relative to
/// the workspace root.
const DIVERGENCE_BASELINE: &str = "conformance/agreement-divergences.json";

/// The reconciliation artifact written to `target/conformance/agreement.json`.
#[derive(Debug, Serialize)]
struct AgreementArtifact {
    suite_version: String,
    spec_revision: String,
    runner: Summary,
    validator: ValidatorSide,
    divergences: Reconciliation,
    wire_schema: schema_ledger::SchemaAgreement,
    agreement: bool,
}

/// The validator's side of the diff.
#[derive(Debug, Serialize)]
struct ValidatorSide {
    sessions: usize,
    totals: Totals,
    failures: Vec<ValidatorFailure>,
}

/// Runs the agreement check and the coverage-manifest check over the tapped
/// sessions in `tap_dir` and the runner results in `results_dir` — the
/// server-side shape, with the server baseline and the manifest gate.
///
/// # Errors
///
/// Returns a human-readable description of the first contract violation:
/// unreadable artifacts, a malformed baseline, an unexplained divergence, a
/// stale baseline entry, or manifest drift.
pub(crate) fn run(
    workspace_root: &Path,
    tap_dir: &Path,
    results_dir: &Path,
    suite_version: &str,
) -> Result<(), String> {
    run_with(
        workspace_root,
        tap_dir,
        results_dir,
        suite_version,
        DIVERGENCE_BASELINE,
        true,
        &schema::SERVER_LEG,
    )
}

/// The agreement check over any captured-session directory, against the
/// named divergence baseline. The coverage manifest is server-party
/// bookkeeping (declared server capabilities vs registry gates), so the
/// client leg runs with `check_coverage_manifest` off — host-side capability
/// honesty is pinned by the host's own tests instead. `leg` names the run
/// for the wire-schema reconciliation and says which party is under test.
///
/// # Errors
///
/// As [`run`].
pub(crate) fn run_with(
    workspace_root: &Path,
    tap_dir: &Path,
    results_dir: &Path,
    suite_version: &str,
    baseline_rel: &str,
    check_coverage_manifest: bool,
    leg: &schema::Leg,
) -> Result<(), String> {
    let registry = Registry::builtin_2025_11_25()
        .map_err(|error| format!("embedded registry failed to load: {error}"))?;
    let baseline = load_baseline(&workspace_root.join(baseline_rel))?;
    let sessions = artifacts::load_sessions(tap_dir)?;
    if sessions.is_empty() {
        return Err(format!(
            "no tapped sessions found in {} — the tap produced nothing, \
             so the agreement check has no evidence to reconcile",
            tap_dir.display()
        ));
    }

    let (totals, failures, reports) = validate_sessions(&registry, &sessions);
    let reconciliation = agreement::reconcile(&baseline, &failures);
    let wire_schema = schema_ledger::check(
        workspace_root,
        leg,
        results_dir,
        &session_views(&sessions, &reports),
    )?;
    let gate = join_gates(
        reconciliation.gate_error(baseline_rel),
        wire_schema.gate_error(),
    );
    eprintln!("xtask: agreement — {}", wire_schema.summary());

    let runner = artifacts::summarize_runner(results_dir)?;
    let artifact = AgreementArtifact {
        suite_version: suite_version.to_owned(),
        spec_revision: registry.revision().to_string(),
        runner,
        validator: ValidatorSide {
            sessions: sessions.len(),
            totals,
            failures,
        },
        agreement: gate.is_none(),
        divergences: reconciliation,
        wire_schema,
    };
    artifacts::write_artifact(results_dir, &artifact)?;

    if check_coverage_manifest {
        manifest::check_manifest(workspace_root, &registry, &sessions)?;
    }

    if let Some(error) = gate {
        return Err(error);
    }
    eprintln!(
        "xtask: agreement — {} session(s) validated; zero unexplained divergence \
         (requirement rows and wire schema); baselines live; artifact at {}",
        sessions.len(),
        results_dir.join("agreement.json").display()
    );
    Ok(())
}

/// Both gates' messages, when either demands one.
fn join_gates(rows: Option<String>, wire_schema: Option<String>) -> Option<String> {
    match (rows, wire_schema) {
        (Some(rows), Some(wire_schema)) => Some(format!("{rows}\n{wire_schema}")),
        (rows, wire_schema) => rows.or(wire_schema),
    }
}

/// Validates every session, returning aggregate totals, the MUST-level
/// failures, and the per-session reports (the wire-schema reconciliation
/// reads them).
fn validate_sessions(
    registry: &Registry,
    sessions: &[(String, Vec<TraceEvent>)],
) -> (Totals, Vec<ValidatorFailure>, Vec<(String, Report)>) {
    let mut totals = Totals::default();
    let mut failures = Vec::new();
    let mut reports = Vec::new();
    for (name, events) in sessions {
        let report = mcp_trace_validator::engine::validate(registry, events);
        totals.add(&report);
        failures.extend(agreement::failures(name, &report));
        reports.push((name.clone(), report));
    }
    (totals, failures, reports)
}

/// Reads and checks the divergence baseline (`mcp_conformance::agreement`'s
/// rules: a known triage class and an upstream link on every entry).
fn load_baseline(path: &Path) -> Result<Baseline, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Baseline::parse(&text).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
