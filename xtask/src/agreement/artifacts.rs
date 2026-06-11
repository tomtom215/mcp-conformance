// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Artifact I/O for the agreement check: tapped-session traces in, runner
//! summaries in, the reconciliation artifact out. Pure plumbing — every
//! judgment lives in the parent module.

use std::collections::BTreeMap;
use std::path::Path;

use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::reader::{Limits, parse_trace};
use serde::Serialize;

/// The runner's side of the diff, summarized from `checks.json` files.
#[derive(Debug, Serialize)]
pub(crate) struct RunnerSide {
    pub(crate) scenarios: usize,
    pub(crate) checks: usize,
    pub(crate) checks_by_status: BTreeMap<String, u32>,
}

/// Loads every tapped session trace, sorted by file name (creation order —
/// the tap prefixes an ordinal).
pub(crate) fn load_sessions(tap_dir: &Path) -> Result<Vec<(String, Vec<TraceEvent>)>, String> {
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
pub(crate) fn summarize_runner(results_dir: &Path) -> Result<RunnerSide, String> {
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

/// Writes the reconciliation artifact as pretty JSON plus a trailing newline.
pub(crate) fn write_artifact<T: Serialize>(results_dir: &Path, artifact: &T) -> Result<(), String> {
    let path = results_dir.join("agreement.json");
    let json = serde_json::to_string_pretty(artifact)
        .map_err(|error| format!("agreement artifact unserializable: {error}"))?;
    std::fs::write(&path, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}
