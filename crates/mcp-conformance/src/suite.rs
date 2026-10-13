// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The official conformance suite this toolkit runs: its pin, and its results.
//!
//! The pin is the one place the version is written. `mcp-conformance test` runs
//! it, `cargo xtask conformance` gates the repository on it, and `cargo xtask
//! suite-currency` asks npm whether it is still current — all three read
//! [`VERSION`] from here.
//!
//! The suite writes one directory per scenario it ran, named
//! `<scenario>-<timestamp>` (server scenarios `server-<scenario>-<timestamp>`),
//! each holding a `checks.json`: an array of checks with an `id`, a `status`
//! (`SUCCESS`, `FAILURE`, `WARNING` or `INFO`) and, for a failure, an
//! `errorMessage`. [`read_results`] reads them back.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The npm package of the official suite.
pub const PACKAGE: &str = "@modelcontextprotocol/conformance";

/// The pinned official suite version.
///
/// Bumps are deliberate changes: review the
/// upstream diff, refresh `conformance/expected-failures.yaml`, and update
/// register row 2.4 in the same commit. `MCP_SUITE_VERSION` overrides it for
/// `cargo xtask conformance` in the scheduled alpha-tracking job only;
/// `mcp-conformance test` always runs this version.
pub const VERSION: &str = "0.1.16";

/// The protocol revision the suite's scenarios are selected for by default
/// (`--spec-version`): the newest one the pinned suite implements.
pub const SPEC_VERSION: &str = "2025-11-25";

/// The client scenarios run when none is named: the pinned suite's protocol
/// scenarios for [`SPEC_VERSION`].
///
/// The `auth/*` scenarios are left out — they
/// exercise an authorization server the suite runs beside the MCP one, which a
/// recording tap in front of the MCP server does not carry (deferral ledger row
/// `auth-client-scenarios`); name them with `--scenario` to run them anyway.
pub const CLIENT_SCENARIOS: [&str; 4] = [
    "initialize",
    "tools_call",
    "elicitation-sep1034-client-defaults",
    "sse-retry",
];

/// The pinned suite's server scenarios that exercise the streamable-HTTP
/// transport itself (`Host` validation, SSE stream handling).
///
/// A stdio server has no such transport: run through the bridge
/// `mcp-conformance test --command` puts in front of it, these would judge the
/// bridge, so they are not run for a stdio server.
pub const HTTP_TRANSPORT_SCENARIOS: [&str; 3] = [
    "dns-rebinding-protection",
    "server-sse-polling",
    "server-sse-multiple-streams",
];

/// The pinned suite's *pending* server scenarios: listed by its `list`
/// command, but left out of the `active` set its `server` command runs by
/// default.
///
/// `mcp-conformance test --command` chooses its scenarios from `list` (one
/// invocation per scenario), and leaves these out so a stdio server is run
/// against the same set an HTTP one is. `cargo xtask conformance` checks this
/// list against what the pinned suite's default run actually runs.
pub const PENDING_SERVER_SCENARIOS: [&str; 2] = ["json-schema-2020-12", "server-sse-polling"];

/// One check the suite reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Check {
    /// The check's id, e.g. `server-initialize`.
    pub id: String,
    /// `SUCCESS`, `FAILURE`, `WARNING` or `INFO`, as the suite wrote it.
    pub status: String,
    /// What failed, when the suite said.
    #[serde(
        rename = "errorMessage",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub error_message: Option<String>,
}

/// One scenario's checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Scenario {
    /// The scenario's name, from its directory, e.g. `server-initialize`.
    pub name: String,
    /// Its checks, in the order the suite wrote them.
    pub checks: Vec<Check>,
}

impl Scenario {
    /// Whether any check failed or warned — what makes the suite's run fail.
    #[must_use]
    pub fn failed(&self) -> bool {
        self.checks
            .iter()
            .any(|check| check.status == "FAILURE" || check.status == "WARNING")
    }
}

/// Reads every scenario's `checks.json` under `results_dir`, in directory-name
/// order. A directory without one is not a scenario and is skipped.
///
/// # Errors
///
/// The directory cannot be read, or a `checks.json` cannot be read or is not
/// an array of checks. Each message names the file.
pub fn read_results(results_dir: &Path) -> Result<Vec<Scenario>, String> {
    let mut dirs = Vec::new();
    for (name, path) in subdirectories(results_dir)? {
        if path.join("checks.json").is_file() {
            dirs.push(name);
        } else {
            // A scenario named `auth/metadata-default` is written as
            // `auth/metadata-default-<timestamp>/`: one level down.
            for (inner, path) in subdirectories(&path)? {
                if path.join("checks.json").is_file() {
                    dirs.push(format!("{name}/{inner}"));
                }
            }
        }
    }
    dirs.sort();
    let mut scenarios = Vec::new();
    for dir in dirs {
        let path = results_dir.join(&dir).join("checks.json");
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let checks: Vec<Check> = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", path.display()))?;
        // A server run prefixes each directory with `server-`, scenarios whose
        // own names begin `server-` included; no client scenario's name
        // begins `server-`.
        let name = scenario_name(&dir);
        let name = name.strip_prefix("server-").unwrap_or(name);
        scenarios.push(Scenario {
            name: name.to_owned(),
            checks,
        });
    }
    Ok(scenarios)
}

/// The directories directly under `dir`, by name.
fn subdirectories(dir: &Path) -> Result<Vec<(String, std::path::PathBuf)>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            found.push((entry.file_name().to_string_lossy().into_owned(), path));
        }
    }
    Ok(found)
}

/// A result directory's name without the suite's timestamp suffix
/// (`-2026-10-13T11-01-48-651Z`). A name that does not end in one is kept
/// whole.
#[must_use]
pub fn scenario_name(dir: &str) -> &str {
    timestamp_start(dir).map_or(dir, |start| &dir[..start])
}

/// Where a trailing `-YYYY-MM-DDTHH-MM-SS-mmmZ` begins, if `dir` ends in one.
fn timestamp_start(dir: &str) -> Option<usize> {
    const SHAPE: &str = "-dddd-dd-ddTdd-dd-dd-dddZ";
    let start = dir.len().checked_sub(SHAPE.len())?;
    let tail = dir.get(start..)?;
    let matches = tail
        .bytes()
        .zip(SHAPE.bytes())
        .all(|(byte, shape)| match shape {
            b'd' => byte.is_ascii_digit(),
            other => byte == other,
        });
    matches.then_some(start)
}

/// The suite's side of a run, counted: what the agreement artifact records.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Summary {
    /// Scenarios with results.
    pub scenarios: usize,
    /// Checks across them.
    pub checks: usize,
    /// Checks by status, as the suite wrote it.
    pub checks_by_status: BTreeMap<String, u32>,
}

impl Summary {
    /// Counts `scenarios`' checks.
    #[must_use]
    pub fn of(scenarios: &[Scenario]) -> Self {
        let mut checks_by_status: BTreeMap<String, u32> = BTreeMap::new();
        for check in scenarios.iter().flat_map(|scenario| &scenario.checks) {
            *checks_by_status.entry(check.status.clone()).or_insert(0) += 1;
        }
        Self {
            scenarios: scenarios.len(),
            checks: scenarios.iter().map(|scenario| scenario.checks.len()).sum(),
            checks_by_status,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
