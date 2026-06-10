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
//! link. The full reconciliation is written to
//! `target/conformance/agreement.json`.
//!
//! The same captured sessions also prove the server's exercised surface: the
//! coverage manifest (`conformance/coverage-manifest.json`) records the
//! capabilities the server declared and the methods the suite drove, checked
//! against the registry's capability gates. `BLESS=1` regenerates it, like
//! every other golden artifact in this repository.

#![allow(clippy::redundant_pub_crate)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mcp_conformance_core::capability::CapabilityParty;
use mcp_conformance_core::requirement::Registry;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::reader::{Limits, parse_trace};
use mcp_trace_validator::report::{Outcome, Report};
use serde::{Deserialize, Serialize};

/// Committed explanations for validator-vs-runner divergences.
const DIVERGENCE_BASELINE: &str = "conformance/agreement-divergences.json";

/// Committed manifest of the server surface the suite exercised.
const MANIFEST_PATH: &str = "conformance/coverage-manifest.json";

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

/// The runner's side of the diff, summarized from `checks.json` files.
#[derive(Debug, Serialize)]
struct RunnerSide {
    scenarios: usize,
    checks: usize,
    checks_by_status: BTreeMap<String, u32>,
}

/// The validator's side of the diff.
#[derive(Debug, Serialize)]
struct ValidatorSide {
    sessions: usize,
    totals: AggregateTotals,
    failures: Vec<ValidatorFailure>,
}

/// Divergences split by whether the baseline explains them.
#[derive(Debug, Serialize)]
struct DivergenceReport {
    unexplained: Vec<ValidatorFailure>,
    explained: Vec<ValidatorFailure>,
}

/// Runs the agreement check and the coverage-manifest check over the tapped
/// sessions in `tap_dir` and the runner results in `results_dir`.
///
/// # Errors
///
/// Returns a human-readable description of the first contract violation:
/// unreadable artifacts, a malformed baseline, an unexplained divergence, or
/// manifest drift.
pub(crate) fn run(tap_dir: &Path, results_dir: &Path, suite_version: &str) -> Result<(), String> {
    let registry = Registry::builtin_2025_11_25()
        .map_err(|error| format!("embedded registry failed to load: {error}"))?;
    let baseline = load_baseline(Path::new(DIVERGENCE_BASELINE))?;
    let sessions = load_sessions(tap_dir)?;
    if sessions.is_empty() {
        return Err(format!(
            "no tapped sessions found in {} — the tap produced nothing, \
             so the agreement check has no evidence to reconcile",
            tap_dir.display()
        ));
    }

    let (totals, failures, reports) = validate_sessions(&registry, &sessions);

    let (explained, unexplained): (Vec<_>, Vec<_>) =
        failures.iter().cloned().partition(|failure| {
            baseline
                .divergences
                .iter()
                .any(|entry| explains(entry, failure))
        });

    let runner = summarize_runner(results_dir)?;
    let artifact = AgreementArtifact {
        suite_version: suite_version.to_owned(),
        spec_revision: registry_revision(&reports),
        runner,
        validator: ValidatorSide {
            sessions: sessions.len(),
            totals,
            failures,
        },
        divergences: DivergenceReport {
            unexplained: unexplained.clone(),
            explained,
        },
        agreement: unexplained.is_empty(),
    };
    write_artifact(results_dir, &artifact)?;

    check_manifest(&registry, &sessions)?;

    if unexplained.is_empty() {
        eprintln!(
            "xtask: agreement — {} session(s) validated; zero unexplained divergence; \
             artifact at {}",
            sessions.len(),
            results_dir.join("agreement.json").display()
        );
        Ok(())
    } else {
        Err(divergence_error(&unexplained))
    }
}

/// Renders the gate-failure message: every unexplained divergence plus the
/// triage instructions.
fn divergence_error(unexplained: &[ValidatorFailure]) -> String {
    let listing = unexplained
        .iter()
        .map(|failure| {
            format!(
                "  {} on {}: {}",
                failure.requirement, failure.trace, failure.detail
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "{} unexplained validator-vs-runner divergence(s):\n{listing}\n\
         Triage each (our-bug | suite-bug | spec-ambiguity), fix or file \
         upstream, and record explained ones in {DIVERGENCE_BASELINE}",
        unexplained.len()
    )
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

/// Loads every tapped session trace, sorted by file name (creation order —
/// the tap prefixes an ordinal).
fn load_sessions(tap_dir: &Path) -> Result<Vec<(String, Vec<TraceEvent>)>, String> {
    let mut names = Vec::new();
    let entries = std::fs::read_dir(tap_dir)
        .map_err(|error| format!("cannot read {}: {error}", tap_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", tap_dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if std::path::Path::new(&name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("jsonl"))
        {
            names.push(name);
        }
    }
    names.sort();
    let mut sessions = Vec::new();
    for name in names {
        let path = tap_dir.join(&name);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let events = parse_trace(&text, &Limits::default())
            .map_err(|error| format!("{} is not a valid trace: {error}", path.display()))?;
        sessions.push((name, events));
    }
    Ok(sessions)
}

/// Summarizes the runner's per-scenario `checks.json` artifacts.
fn summarize_runner(results_dir: &Path) -> Result<RunnerSide, String> {
    let mut scenarios = 0;
    let mut checks = 0;
    let mut by_status: BTreeMap<String, u32> = BTreeMap::new();
    let entries = std::fs::read_dir(results_dir)
        .map_err(|error| format!("cannot read {}: {error}", results_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", results_dir.display()))?;
        let checks_path = entry.path().join("checks.json");
        if !checks_path.is_file() {
            continue;
        }
        scenarios += 1;
        let text = std::fs::read_to_string(&checks_path)
            .map_err(|error| format!("cannot read {}: {error}", checks_path.display()))?;
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", checks_path.display()))?;
        checks += parsed.len();
        for check in &parsed {
            let status = check
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("UNKNOWN");
            *by_status.entry(status.to_owned()).or_insert(0) += 1;
        }
    }
    if scenarios == 0 {
        return Err(format!(
            "no checks.json found under {} — did the runner write its results?",
            results_dir.display()
        ));
    }
    Ok(RunnerSide {
        scenarios,
        checks,
        checks_by_status: by_status,
    })
}

/// Writes the reconciliation artifact.
fn write_artifact(results_dir: &Path, artifact: &AgreementArtifact) -> Result<(), String> {
    let path = results_dir.join("agreement.json");
    let json = serde_json::to_string_pretty(artifact)
        .map_err(|error| format!("agreement artifact unserializable: {error}"))?;
    std::fs::write(&path, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

// ── Coverage manifest ────────────────────────────────────────────────────────

/// The committed manifest: what surface the tapped suite sessions prove.
#[derive(Debug, Serialize)]
struct CoverageManifest {
    /// How to regenerate (kept in the artifact so it explains itself).
    #[serde(rename = "_generated")]
    generated: String,
    /// The registry revision the gates come from.
    spec_revision: String,
    /// The server's declared capabilities, from the initialize result.
    server_capabilities: serde_json::Value,
    /// Every server-party capability gate in the registry, with whether the
    /// server declared it. All must be true: an undeclared gate means a slice
    /// of the registry silently became not-applicable.
    capability_gates: BTreeMap<String, bool>,
    /// Request methods the suite drove, as observed on the wire.
    methods_observed: BTreeSet<String>,
}

/// Builds the manifest from the tapped sessions and checks it against the
/// committed copy (or rewrites the committed copy under `BLESS=1`).
fn check_manifest(
    registry: &Registry,
    sessions: &[(String, Vec<TraceEvent>)],
) -> Result<(), String> {
    let server_capabilities = sessions
        .iter()
        .find_map(|(_, events)| initialize_capabilities(events))
        .ok_or_else(|| {
            "no initialize result with capabilities found in any tapped session".to_owned()
        })?;

    let capability_gates = server_gates(registry, &server_capabilities);
    let methods_observed = observed_methods(sessions);

    let manifest = CoverageManifest {
        generated: "cargo xtask conformance (BLESS=1 to regenerate)".to_owned(),
        spec_revision: "2025-11-25".to_owned(),
        server_capabilities,
        capability_gates: capability_gates.clone(),
        methods_observed,
    };

    if let Some((gate, _)) = capability_gates.iter().find(|(_, declared)| !**declared) {
        return Err(format!(
            "registry gate {gate} is not declared by the server — a slice of \
             the registry silently became not-applicable; declare the \
             capability or document the exclusion"
        ));
    }

    let rendered = serde_json::to_string_pretty(&manifest)
        .map_err(|error| format!("manifest unserializable: {error}"))?
        + "\n";
    let path = Path::new(MANIFEST_PATH);
    if std::env::var_os("BLESS").is_some_and(|v| v == "1") {
        std::fs::write(path, &rendered)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        eprintln!("xtask: agreement — blessed {}", path.display());
        return Ok(());
    }
    let committed = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "cannot read {}: {error} (BLESS=1 to create it)",
            path.display()
        )
    })?;
    if committed == rendered {
        eprintln!(
            "xtask: agreement — coverage manifest in sync ({})",
            path.display()
        );
        Ok(())
    } else {
        Err(format!(
            "{} is out of sync with the tapped sessions — review the change \
             and regenerate with BLESS=1 cargo xtask conformance",
            path.display()
        ))
    }
}

/// Every server-party capability gate in the registry, with whether the
/// server's declared capabilities satisfy it.
fn server_gates(
    registry: &Registry,
    server_capabilities: &serde_json::Value,
) -> BTreeMap<String, bool> {
    let mut gates = BTreeMap::new();
    for requirement in registry.requirements() {
        if let Some(gate) = &requirement.capability
            && gate.party() == CapabilityParty::Server
        {
            gates.insert(
                gate.as_str().to_owned(),
                gate.is_declared(Some(server_capabilities)),
            );
        }
    }
    gates
}

/// Every request method observed across the tapped sessions.
fn observed_methods(sessions: &[(String, Vec<TraceEvent>)]) -> BTreeSet<String> {
    let mut methods = BTreeSet::new();
    for (_, events) in sessions {
        for event in events {
            if let Some(method) = event
                .message_payload()
                .and_then(|payload| payload.get("method"))
                .and_then(serde_json::Value::as_str)
            {
                methods.insert(method.to_owned());
            }
        }
    }
    methods
}

/// The `capabilities` object from the first initialize *result* in a session.
fn initialize_capabilities(events: &[TraceEvent]) -> Option<serde_json::Value> {
    // The initialize request's id, so the matching result can be found.
    let init_id = events.iter().find_map(|event| {
        let payload = event.message_payload()?;
        (payload.get("method")? == "initialize").then(|| payload.get("id").cloned())?
    })?;
    events.iter().find_map(|event| {
        let payload = event.message_payload()?;
        if payload.get("id") == Some(&init_id) {
            payload.get("result")?.get("capabilities").cloned()
        } else {
            None
        }
    })
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

    #[test]
    fn baseline_entry_explains_by_requirement_and_optional_trace_filter() {
        let entry = ExplainedDivergence {
            requirement: "LIFE-009".to_owned(),
            class: "suite-bug".to_owned(),
            upstream: "https://github.com/example/issues/1".to_owned(),
            trace_contains: Some("003-".to_owned()),
            note: None,
        };
        assert!(explains(&entry, &failure("003-abc.jsonl", "LIFE-009")));
        assert!(!explains(&entry, &failure("004-abc.jsonl", "LIFE-009")));
        assert!(!explains(&entry, &failure("003-abc.jsonl", "TOOL-001")));
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

    #[test]
    fn initialize_capabilities_pairs_request_id_with_result() {
        let trace = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":7,"method":"initialize","params":{}}}
{"seq":1,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":7,"result":{"capabilities":{"tools":{}}}}}"#;
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let caps = initialize_capabilities(&events).unwrap();
        assert_eq!(caps, serde_json::json!({"tools": {}}));
    }
}
