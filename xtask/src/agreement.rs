// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The agreement check: our validator's verdicts reconciled with the official
//! runner's, over the very sessions the runner drove.
//!
//! `conformance.rs` taps the everything-server while the pinned suite runs;
//! this module replays every captured session through `mcp-trace-validator`
//! and enforces the policy in docs/plan/03-conformance-strategy.md
//! §Calibration: **zero unexplained divergence**. A divergence is a MUST-level
//! validator failure on a session the runner passed; explanations live in
//! `conformance/agreement-divergences.json`, where every entry must carry a
//! triage class (`our-bug` | `suite-bug` | `spec-ambiguity`) and an upstream
//! link. The baseline must also stay live: an entry that explains nothing in
//! the current run is stale — the divergence it described no longer occurs —
//! and fails the gate until it is removed, so explanations leave the baseline
//! in the same change that resolves them (e.g. a suite pin bump). The full
//! reconciliation is written to `target/conformance/agreement.json`.
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

use mcp_conformance_core::requirement::Registry;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::report::{Outcome, Report};
use serde::{Deserialize, Serialize};

mod artifacts;
mod manifest;

use artifacts::RunnerSide;

/// Committed explanations for validator-vs-runner divergences, relative to
/// the workspace root.
const DIVERGENCE_BASELINE: &str = "conformance/agreement-divergences.json";

/// The triage classes the policy admits — nothing else parses.
const TRIAGE_CLASSES: [&str; 3] = ["our-bug", "suite-bug", "spec-ambiguity"];

/// One explained divergence in the committed baseline. Unknown fields are
/// rejected so a typo (say, `trace_containz`) cannot silently broaden an
/// entry's reach.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplainedDivergence {
    /// The registry requirement ID the validator flags.
    requirement: String,
    /// Triage class: `our-bug` | `suite-bug` | `spec-ambiguity`.
    class: String,
    /// Upstream issue/PR URL explaining the divergence.
    upstream: String,
    /// Optional substring filter on the trace file name.
    #[serde(default)]
    trace_contains: Option<String>,
    /// Free-text context for reviewers; never consulted by the gate.
    #[serde(default, rename = "_note")]
    #[allow(dead_code)]
    note: Option<String>,
}

impl ExplainedDivergence {
    /// How the entry is named in gate output: requirement plus filter.
    fn describe(&self) -> String {
        self.trace_contains.as_ref().map_or_else(
            || self.requirement.clone(),
            |needle| format!("{} (trace_contains {needle:?})", self.requirement),
        )
    }
}

/// The committed divergence baseline file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DivergenceBaseline {
    /// The policy text (kept in the file so the contract travels with it).
    #[allow(dead_code)]
    policy: String,
    /// The explained divergences.
    divergences: Vec<ExplainedDivergence>,
}

/// One validator failure observed on a tapped session.
#[derive(Debug, Clone, Serialize)]
struct ValidatorFailure {
    /// Trace file name (relative to the tap directory).
    trace: String,
    /// The failing requirement ID.
    requirement: String,
    /// The first finding's detail line.
    detail: String,
}

/// Aggregated outcome counts across every tapped session.
#[derive(Debug, Default, Serialize)]
struct AggregateTotals {
    pass: u32,
    fail: u32,
    warn: u32,
    excluded: u32,
    unsupported: u32,
    not_applicable: u32,
}

/// The reconciliation artifact written to `target/conformance/agreement.json`.
#[derive(Debug, Serialize)]
struct AgreementArtifact {
    suite_version: String,
    spec_revision: String,
    runner: RunnerSide,
    validator: ValidatorSide,
    divergences: DivergenceReport,
    agreement: bool,
}

/// The validator's side of the diff.
#[derive(Debug, Serialize)]
struct ValidatorSide {
    sessions: usize,
    totals: AggregateTotals,
    failures: Vec<ValidatorFailure>,
}

/// Divergences split three ways against the baseline: failures it explains,
/// failures it does not, and entries that explained nothing (stale).
#[derive(Debug, Serialize)]
struct DivergenceReport {
    unexplained: Vec<ValidatorFailure>,
    explained: Vec<ValidatorFailure>,
    stale_baseline_entries: Vec<String>,
}

/// The three-way reconciliation of observed failures against the baseline.
struct Reconciliation {
    explained: Vec<ValidatorFailure>,
    unexplained: Vec<ValidatorFailure>,
    /// Baseline entries (as [`ExplainedDivergence::describe`] strings) that
    /// matched no failure in this run.
    stale: Vec<String>,
}

/// Runs the agreement check and the coverage-manifest check over the tapped
/// sessions in `tap_dir` and the runner results in `results_dir`.
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
    let registry = Registry::builtin_2025_11_25()
        .map_err(|error| format!("embedded registry failed to load: {error}"))?;
    let baseline = load_baseline(&workspace_root.join(DIVERGENCE_BASELINE))?;
    let sessions = artifacts::load_sessions(tap_dir)?;
    if sessions.is_empty() {
        return Err(format!(
            "no tapped sessions found in {} — the tap produced nothing, \
             so the agreement check has no evidence to reconcile",
            tap_dir.display()
        ));
    }

    let (totals, failures, reports) = validate_sessions(&registry, &sessions);
    let reconciliation = reconcile(&baseline, &failures);
    let gate = gate_error(&reconciliation);

    let runner = artifacts::summarize_runner(results_dir)?;
    let artifact = AgreementArtifact {
        suite_version: suite_version.to_owned(),
        spec_revision: registry_revision(&reports),
        runner,
        validator: ValidatorSide {
            sessions: sessions.len(),
            totals,
            failures,
        },
        agreement: gate.is_none(),
        divergences: DivergenceReport {
            unexplained: reconciliation.unexplained,
            explained: reconciliation.explained,
            stale_baseline_entries: reconciliation.stale,
        },
    };
    artifacts::write_artifact(results_dir, &artifact)?;

    manifest::check_manifest(workspace_root, &registry, &sessions)?;

    if let Some(error) = gate {
        return Err(error);
    }
    eprintln!(
        "xtask: agreement — {} session(s) validated; zero unexplained divergence; \
         baseline live; artifact at {}",
        sessions.len(),
        results_dir.join("agreement.json").display()
    );
    Ok(())
}

/// Splits observed failures into explained/unexplained against the baseline,
/// and surfaces baseline entries that explained nothing. Both directions
/// gate: an unexplained failure is an undocumented divergence, and a stale
/// entry is documentation for a divergence that no longer exists — left in
/// place it would silently absorb the *next* failure matching its pattern.
fn reconcile(baseline: &DivergenceBaseline, failures: &[ValidatorFailure]) -> Reconciliation {
    let (explained, unexplained): (Vec<_>, Vec<_>) =
        failures.iter().cloned().partition(|failure| {
            baseline
                .divergences
                .iter()
                .any(|entry| explains(entry, failure))
        });
    let stale = baseline
        .divergences
        .iter()
        .filter(|entry| !failures.iter().any(|failure| explains(entry, failure)))
        .map(ExplainedDivergence::describe)
        .collect();
    Reconciliation {
        explained,
        unexplained,
        stale,
    }
}

/// The gate-failure message, when the reconciliation demands one: every
/// unexplained divergence and every stale baseline entry, with triage
/// instructions for each.
fn gate_error(reconciliation: &Reconciliation) -> Option<String> {
    let mut sections = Vec::new();
    if !reconciliation.unexplained.is_empty() {
        let listing = reconciliation
            .unexplained
            .iter()
            .map(|failure| {
                format!(
                    "  {} on {}: {}",
                    failure.requirement, failure.trace, failure.detail
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "{} unexplained validator-vs-runner divergence(s):\n{listing}\n\
             Triage each (our-bug | suite-bug | spec-ambiguity), fix or file \
             upstream, and record explained ones in {DIVERGENCE_BASELINE}",
            reconciliation.unexplained.len()
        ));
    }
    if !reconciliation.stale.is_empty() {
        let listing = reconciliation
            .stale
            .iter()
            .map(|entry| format!("  {entry}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!(
            "{} stale baseline entr(y/ies) in {DIVERGENCE_BASELINE} — each \
             explains a divergence that no longer occurs:\n{listing}\n\
             Remove them in this same change (typically the suite pin bump or \
             fix that resolved them)",
            reconciliation.stale.len()
        ));
    }
    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n"))
    }
}

/// Validates every session, returning aggregate totals, the MUST-level
/// failures, and the per-session reports.
fn validate_sessions(
    registry: &Registry,
    sessions: &[(String, Vec<TraceEvent>)],
) -> (
    AggregateTotals,
    Vec<ValidatorFailure>,
    Vec<(String, Report)>,
) {
    let mut totals = AggregateTotals::default();
    let mut failures = Vec::new();
    let mut reports = Vec::new();
    for (name, events) in sessions {
        let report = mcp_trace_validator::engine::validate(registry, events);
        accumulate(&mut totals, &report);
        for row in &report.requirements {
            if row.outcome == Outcome::Fail {
                failures.push(ValidatorFailure {
                    trace: name.clone(),
                    requirement: row.id.clone(),
                    detail: row
                        .findings
                        .first()
                        .map_or_else(String::new, |finding| finding.detail.clone()),
                });
            }
        }
        reports.push((name.clone(), report));
    }
    (totals, failures, reports)
}

/// The registry revision, taken from any report (they all share it).
fn registry_revision(reports: &[(String, Report)]) -> String {
    reports
        .first()
        .map_or_else(String::new, |(_, report)| report.revision.clone())
}

/// Adds one report's totals into the aggregate.
const fn accumulate(totals: &mut AggregateTotals, report: &Report) {
    totals.pass += report.totals.pass;
    totals.fail += report.totals.fail;
    totals.warn += report.totals.warn;
    totals.excluded += report.totals.excluded;
    totals.unsupported += report.totals.unsupported;
    totals.not_applicable += report.totals.not_applicable;
}

/// Whether a baseline entry explains a failure.
fn explains(entry: &ExplainedDivergence, failure: &ValidatorFailure) -> bool {
    entry.requirement == failure.requirement
        && entry
            .trace_contains
            .as_ref()
            .is_none_or(|needle| failure.trace.contains(needle))
}

/// Loads and structurally validates the divergence baseline: every entry
/// must carry a known triage class and an HTTP(S) upstream link.
fn load_baseline(path: &Path) -> Result<DivergenceBaseline, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let baseline: DivergenceBaseline = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not valid: {error}", path.display()))?;
    for entry in &baseline.divergences {
        if !TRIAGE_CLASSES.contains(&entry.class.as_str()) {
            return Err(format!(
                "{}: entry for {} has class {:?}; the policy admits {TRIAGE_CLASSES:?}",
                path.display(),
                entry.requirement,
                entry.class
            ));
        }
        if !entry.upstream.starts_with("https://") && !entry.upstream.starts_with("http://") {
            return Err(format!(
                "{}: entry for {} needs an upstream issue/PR URL, got {:?}",
                path.display(),
                entry.requirement,
                entry.upstream
            ));
        }
    }
    Ok(baseline)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn failure(trace: &str, requirement: &str) -> ValidatorFailure {
        ValidatorFailure {
            trace: trace.to_owned(),
            requirement: requirement.to_owned(),
            detail: String::new(),
        }
    }

    fn entry(requirement: &str, trace_contains: Option<&str>) -> ExplainedDivergence {
        ExplainedDivergence {
            requirement: requirement.to_owned(),
            class: "suite-bug".to_owned(),
            upstream: "https://github.com/example/issues/1".to_owned(),
            trace_contains: trace_contains.map(ToOwned::to_owned),
            note: None,
        }
    }

    fn baseline(divergences: Vec<ExplainedDivergence>) -> DivergenceBaseline {
        DivergenceBaseline {
            policy: "p".to_owned(),
            divergences,
        }
    }

    #[test]
    fn baseline_entry_explains_by_requirement_and_optional_trace_filter() {
        let entry = entry("LIFE-009", Some("003-"));
        assert!(explains(&entry, &failure("003-abc.jsonl", "LIFE-009")));
        assert!(!explains(&entry, &failure("004-abc.jsonl", "LIFE-009")));
        assert!(!explains(&entry, &failure("003-abc.jsonl", "TOOL-001")));
    }

    #[test]
    fn reconcile_partitions_and_finds_no_stale_when_baseline_is_live() {
        let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
        let failures = [
            failure("003-dns-rebinding.jsonl", "LIFE-003"),
            failure("001-init.jsonl", "TOOL-001"),
        ];
        let result = reconcile(&baseline, &failures);
        assert_eq!(result.explained.len(), 1);
        assert_eq!(result.unexplained.len(), 1);
        assert_eq!(result.unexplained[0].requirement, "TOOL-001");
        assert!(result.stale.is_empty());
        // Unexplained failures gate; the message carries the triage path.
        let message = gate_error(&result).unwrap();
        assert!(message.contains("unexplained"), "{message}");
        assert!(message.contains("TOOL-001"), "{message}");
    }

    #[test]
    fn reconcile_flags_entries_that_explain_nothing_as_stale() {
        // The divergence this entry described was fixed upstream: nothing in
        // the run matches it, and the gate must demand its removal rather
        // than leave a pattern lying in wait for the next LIFE-003 failure.
        let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
        let result = reconcile(&baseline, &[]);
        assert!(result.explained.is_empty());
        assert!(result.unexplained.is_empty());
        assert_eq!(result.stale, ["LIFE-003 (trace_contains \"dns\")"]);
        let message = gate_error(&result).unwrap();
        assert!(message.contains("stale"), "{message}");
        assert!(message.contains("LIFE-003"), "{message}");
    }

    #[test]
    fn stale_entry_does_not_mask_a_new_failure_outside_its_filter() {
        // Same requirement, different session: the filtered entry explains
        // nothing (stale) and the new failure must surface as unexplained —
        // both directions reported in one gate message.
        let baseline = baseline(vec![entry("LIFE-003", Some("dns"))]);
        let failures = [failure("001-init.jsonl", "LIFE-003")];
        let result = reconcile(&baseline, &failures);
        assert!(result.explained.is_empty());
        assert_eq!(result.unexplained.len(), 1);
        assert_eq!(result.stale.len(), 1);
        let message = gate_error(&result).unwrap();
        assert!(message.contains("unexplained"), "{message}");
        assert!(message.contains("stale"), "{message}");
    }

    #[test]
    fn unfiltered_entries_match_any_trace_and_stay_live_across_sessions() {
        // No trace_contains: the entry explains the requirement wherever it
        // fails, so it is live as long as any session still fails it.
        let baseline = baseline(vec![entry("LIFE-003", None)]);
        let failures = [failure("anything.jsonl", "LIFE-003")];
        let result = reconcile(&baseline, &failures);
        assert_eq!(result.explained.len(), 1);
        assert!(result.stale.is_empty());
        assert!(gate_error(&result).is_none());
    }

    #[test]
    fn baseline_rejects_unknown_class_and_non_url_upstream() {
        let dir = std::env::temp_dir().join(format!("agreement-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("baseline.json");

        std::fs::write(
            &path,
            r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"wontfix","upstream":"https://e.example"}]}"#,
        )
        .unwrap();
        assert!(load_baseline(&path).unwrap_err().contains("class"));

        std::fs::write(
            &path,
            r#"{"policy":"p","divergences":[{"requirement":"X-001","class":"our-bug","upstream":"see notes"}]}"#,
        )
        .unwrap();
        assert!(load_baseline(&path).unwrap_err().contains("upstream"));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
