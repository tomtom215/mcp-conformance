// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Artifact I/O for the agreement check: tapped-session traces in, runner
//! summaries in, the reconciliation artifact out. Pure plumbing — every
//! judgment lives in the parent module.

use std::path::Path;

use mcp_conformance_cli::suite::Summary;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::reader::{Limits, parse_trace};
use serde::Serialize;

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

/// Summarizes the runner's per-scenario `checks.json` artifacts, read by the
/// same code `mcp-conformance test` reads them with.
pub(crate) fn summarize_runner(results_dir: &Path) -> Result<Summary, String> {
    let scenarios = mcp_conformance_cli::suite::read_results(results_dir)?;
    if scenarios.is_empty() {
        return Err(format!(
            "no checks.json found under {} — did the runner write its results?",
            results_dir.display()
        ));
    }
    Ok(Summary::of(&scenarios))
}

/// Writes the reconciliation artifact as pretty JSON plus a trailing newline.
pub(crate) fn write_artifact<T: Serialize>(results_dir: &Path, artifact: &T) -> Result<(), String> {
    let path = results_dir.join("agreement.json");
    let json = serde_json::to_string_pretty(artifact)
        .map_err(|error| format!("agreement artifact unserializable: {error}"))?;
    std::fs::write(&path, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}
