// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What "agrees" means, as code: `mcp-conformance test`'s JSON report held to
//! the gate's own artifacts for the same subject.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mcp_conformance::suite;
use serde::Deserialize;

/// Which run of `test` is being compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leg {
    Http,
    Stdio,
    Client,
}

impl Leg {
    const fn name(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Stdio => "stdio",
            Self::Client => "client",
        }
    }
}

/// Each scenario's check statuses, sorted: what two runs must share.
type Statuses = BTreeMap<String, Vec<String>>;

/// The gate's side: its runner results and the requirements its recordings fail.
#[derive(Debug)]
pub(crate) struct Gate {
    scenarios: Statuses,
    failing: BTreeSet<String>,
}

impl Gate {
    /// The runner results under `results` (top level only: the client leg's
    /// and the capture leg's runs live in subdirectories) and the failures in
    /// the agreement artifact `agreement`.
    pub(crate) fn read(results: &Path, agreement: &Path) -> Result<Self, String> {
        let scenarios = suite::read_results(results)?
            .into_iter()
            .filter(|scenario| !scenario.name.contains('/'))
            .map(|scenario| {
                let mut statuses: Vec<String> = scenario
                    .checks
                    .into_iter()
                    .map(|check| check.status)
                    .collect();
                statuses.sort();
                (scenario.name, statuses)
            })
            .collect();
        let text = std::fs::read_to_string(agreement)
            .map_err(|error| format!("cannot read {}: {error}", agreement.display()))?;
        let artifact: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", agreement.display()))?;
        let failing = artifact["validator"]["failures"]
            .as_array()
            .ok_or_else(|| format!("{} has no validator.failures", agreement.display()))?
            .iter()
            .filter_map(|failure| failure["requirement"].as_str().map(ToOwned::to_owned))
            .collect();
        Ok(Self { scenarios, failing })
    }
}

/// `test`'s side, from its JSON report.
#[derive(Debug, Default)]
pub(crate) struct Report {
    scenarios: Statuses,
    not_run: Vec<String>,
    /// `(trace, requirement)` for every failing clause.
    failures: Vec<(String, String)>,
}

impl Report {
    pub(crate) fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let report: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", path.display()))?;
        Ok(Self::from_json(&report))
    }

    fn from_json(report: &serde_json::Value) -> Self {
        let list = |value: &serde_json::Value| value.as_array().cloned().unwrap_or_default();
        let text = |value: &serde_json::Value| value.as_str().unwrap_or_default().to_owned();
        let scenarios = list(&report["suite"]["scenarios"])
            .iter()
            .map(|scenario| {
                let mut statuses: Vec<String> = list(&scenario["checks"])
                    .iter()
                    .map(|check| text(&check["status"]))
                    .collect();
                statuses.sort();
                (text(&scenario["name"]), statuses)
            })
            .collect();
        let not_run = list(&report["suite"]["not_run"])
            .iter()
            .map(|entry| text(&entry["scenario"]))
            .collect();
        let divergences = &report["agreement"]["divergences"];
        let failures = list(&divergences["explained"])
            .iter()
            .chain(list(&divergences["unexplained"]).iter())
            .map(|failure| (text(&failure["trace"]), text(&failure["requirement"])))
            .collect();
        Self {
            scenarios,
            not_run,
            failures,
        }
    }
}

/// The committed findings only a wire-level recording shows.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireFindings {
    #[allow(dead_code)]
    policy: String,
    findings: Vec<WireFinding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFinding {
    leg: String,
    requirement: String,
    #[serde(default)]
    trace_contains: Option<String>,
    /// Why the gate's recorders cannot see it; for reviewers.
    #[allow(dead_code)]
    why: String,
    #[serde(skip)]
    observed: Cell<bool>,
}

impl WireFindings {
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let findings: Self = serde_json::from_str(&text)
            .map_err(|error| format!("{} is not valid: {error}", path.display()))?;
        if let Some(bad) = findings
            .findings
            .iter()
            .find(|finding| !["http", "stdio", "client"].contains(&finding.leg.as_str()))
        {
            return Err(format!(
                "{}: leg {:?} is not http, stdio or client",
                path.display(),
                bad.leg
            ));
        }
        Ok(findings)
    }

    /// Marks and returns whether an entry explains `requirement` on `trace`.
    fn explains(&self, leg: Leg, trace: &str, requirement: &str) -> bool {
        let mut explained = false;
        for finding in &self.findings {
            if finding.leg == leg.name()
                && finding.requirement == requirement
                && finding
                    .trace_contains
                    .as_ref()
                    .is_none_or(|needle| trace.contains(needle.as_str()))
            {
                finding.observed.set(true);
                explained = true;
            }
        }
        explained
    }

    /// Every entry explained something in this run.
    pub(crate) fn all_observed(&self) -> Result<(), String> {
        let stale: Vec<String> = self
            .findings
            .iter()
            .filter(|finding| !finding.observed.get())
            .map(|finding| format!("  {} {}", finding.leg, finding.requirement))
            .collect();
        if stale.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "{} entr(y/ies) in {} no longer occur — remove them:\n{}",
                stale.len(),
                super::WIRE_FINDINGS,
                stale.join("\n")
            ))
        }
    }
}

/// Holds one run of `test` to the gate.
pub(crate) fn check(
    leg: Leg,
    gate: &Gate,
    report: &Report,
    wire: &WireFindings,
) -> Result<(), String> {
    let mut expected = gate.scenarios.clone();
    if leg == Leg::Stdio {
        expected.retain(|name, _| !suite::HTTP_TRANSPORT_SCENARIOS.contains(&name.as_str()));
    }
    if report.scenarios != expected {
        return Err(format!(
            "{}: the suite's results differ from the gate's run on the same subject:\n  \
             gate: {expected:?}\n  test: {:?}",
            leg.name(),
            report.scenarios
        ));
    }
    let failing: BTreeSet<&str> = report
        .failures
        .iter()
        .map(|(_, requirement)| requirement.as_str())
        .collect();
    // The stdio run drives no HTTP scenario, so the gate's HTTP-session
    // failures have nothing to be compared with there.
    if leg != Leg::Stdio
        && let Some(missing) = gate
            .failing
            .iter()
            .find(|requirement| !failing.contains(requirement.as_str()))
    {
        return Err(format!(
            "{}: the gate's recordings fail {missing}, and test's report does not",
            leg.name()
        ));
    }
    let unexplained: Vec<String> = report
        .failures
        .iter()
        .filter(|(_, requirement)| leg == Leg::Stdio || !gate.failing.contains(requirement))
        .filter(|(trace, requirement)| !wire.explains(leg, trace, requirement))
        .map(|(trace, requirement)| format!("  {requirement} on {trace}"))
        .collect();
    if unexplained.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}: test fails clauses the gate's recordings do not, and {} does not list:\n{}",
            leg.name(),
            super::WIRE_FINDINGS,
            unexplained.join("\n")
        ))
    }
}

/// The pending list `test --command` subtracts is exactly what the suite's
/// default run left out: the stdio run's scenarios and its not-run ones are
/// `list`'s, and less the pending ones they are the gate's run.
pub(crate) fn pending_is_what_the_default_run_leaves_out(
    gate: &Gate,
    stdio: &Report,
) -> Result<(), String> {
    let listed: BTreeSet<&str> = stdio
        .scenarios
        .keys()
        .map(String::as_str)
        .chain(stdio.not_run.iter().map(String::as_str))
        .collect();
    let active: BTreeSet<&str> = listed
        .iter()
        .copied()
        .filter(|name| !suite::PENDING_SERVER_SCENARIOS.contains(name))
        .collect();
    let ran: BTreeSet<&str> = gate.scenarios.keys().map(String::as_str).collect();
    if active == ran {
        Ok(())
    } else {
        Err(format!(
            "mcp_conformance::suite::PENDING_SERVER_SCENARIOS is stale: the suite lists \
             {listed:?}, and its default run ran {ran:?}"
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
