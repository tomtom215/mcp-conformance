// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! SARIF 2.1.0 rendering of validation reports, for code-scanning platforms
//! (GitHub code scanning, GitLab, Azure DevOps, IDE SARIF viewers).
//!
//! Mapping:
//!
//! | Report | SARIF |
//! |--------|-------|
//! | a `fail` / `warn` row | a *rule* (`ruleId` = the requirement ID; the clause verbatim, its level, and its published link) |
//! | each finding on that row | a *result* at level `error` (MUST) or `warning` (SHOULD), located at the trace line holding the event it names |
//! | an `unsupported` row | an invocation notification, and `executionSuccessful: false` |
//! | pass / excluded / not-applicable / not-observed | nothing — SARIF records problems, not coverage |
//!
//! Every judged revision's findings are results of one run: code scanning
//! rejects several runs of one tool in one upload. A requirement ID judged under
//! two revisions becomes `ID@revision` so the two rules stay distinct; the
//! shipped registries share no ID, so in practice rule IDs are the plain
//! requirement IDs.
//!
//! Output is deterministic — rules in order of first finding, results in
//! registry and finding order, no timestamps or environment — like every other
//! report format.

use core::fmt::Write as _;
use std::collections::BTreeMap;

use mcp_conformance_core::trace::TraceEvent;
use serde_json::{Value, json};

use crate::report::{Outcome, Report, RequirementReport};

/// Where the findings point.
///
/// The trace as the caller named it (a path relative to the repository root is
/// what code scanning can annotate), and its events, so a finding's `seq` can
/// become the line that holds it.
#[derive(Debug, Clone, Copy)]
pub struct Artifact<'a> {
    /// The trace's location, as a path or URI reference; `None` when it has none
    /// (read from stdin), in which case results carry no location.
    pub uri: Option<&'a str>,
    /// The trace's events, in document order.
    pub events: &'a [TraceEvent],
}

/// Renders the reports as one SARIF 2.1.0 log.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, reader, sarif};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
/// let events = reader::parse_trace(trace, &reader::Limits::default())?;
/// let report = engine::validate(&registry, &events);
/// let log: serde_json::Value = serde_json::from_str(&sarif::render(
///     &[report],
///     sarif::Artifact { uri: Some("session.jsonl"), events: &events },
/// ))?;
/// assert_eq!(log["version"], "2.1.0");
/// assert_eq!(log["runs"][0]["results"][0]["ruleId"], "LIFE-001");
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render(reports: &[Report], artifact: Artifact<'_>) -> String {
    let mut log = Log {
        rule_ids: rule_ids(reports),
        index_of: BTreeMap::new(),
        rules: Vec::new(),
        results: Vec::new(),
        notifications: Vec::new(),
    };
    for report in reports {
        for row in &report.requirements {
            match row.outcome {
                Outcome::Fail | Outcome::Warn => log.findings(row, &report.revision, artifact),
                Outcome::Unsupported => log.unsupported(row, &report.revision),
                _ => {}
            }
        }
    }
    let log = json!({
        "$schema": "https://docs.oasis-open.org/sarif/sarif/v2.1.0/errata01/os/schemas/sarif-schema-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "mcp-trace-validator",
                    "semanticVersion": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/tomtom215/mcp-conformance",
                    "rules": log.rules,
                },
            },
            "invocations": [{
                "executionSuccessful": log.notifications.is_empty(),
                "toolExecutionNotifications": log.notifications,
            }],
            "results": log.results,
            "properties": {
                "revisions": reports.iter().map(|report| &report.revision).collect::<Vec<_>>(),
            },
        }],
    });
    let mut out = serde_json::to_string_pretty(&log).unwrap_or_default();
    out.push('\n');
    out
}

/// The run as it is assembled: rules in order of first finding, their results,
/// and the notifications for clauses this build cannot judge.
struct Log<'r> {
    rule_ids: BTreeMap<(&'r str, &'r str), String>,
    index_of: BTreeMap<String, usize>,
    rules: Vec<Value>,
    results: Vec<Value>,
    notifications: Vec<Value>,
}

impl Log<'_> {
    /// A result per finding on a `fail` / `warn` row, under the row's rule.
    fn findings(&mut self, row: &RequirementReport, revision: &str, artifact: Artifact<'_>) {
        let rule_id = self.rule_ids[&(row.id.as_str(), revision)].clone();
        let rules = &mut self.rules;
        let rule_index = *self.index_of.entry(rule_id.clone()).or_insert_with(|| {
            rules.push(rule(&rule_id, row, revision));
            rules.len() - 1
        });
        for finding in &row.findings {
            let seq = finding.seq.map_or_else(String::new, |seq| seq.to_string());
            self.results.push(json!({
                "ruleId": rule_id,
                "ruleIndex": rule_index,
                "level": level(row.outcome),
                "message": { "text": finding.detail },
                "locations": location(artifact, finding.seq),
                "partialFingerprints": {
                    "mcpConformanceFinding/v1": format!("{rule_id}:{}:{seq}", finding.check),
                },
                "properties": {
                    "revision": revision,
                    "check": finding.check,
                    "seq": finding.seq,
                },
            }));
        }
    }

    /// An `unsupported` row: the run could not judge this clause.
    fn unsupported(&mut self, row: &RequirementReport, revision: &str) {
        self.notifications.push(json!({
            "level": "error",
            "message": {
                "text": format!(
                    "{} ({revision}): the registry references checks this build does not implement: {}",
                    row.id,
                    row.missing_checks.join(", ")
                ),
            },
        }));
    }
}

/// The SARIF rule ID of each `(requirement ID, revision)` with a finding: the
/// requirement ID, suffixed with its revision only where the same ID has
/// findings under more than one.
fn rule_ids(reports: &[Report]) -> BTreeMap<(&str, &str), String> {
    let mut revisions_of: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for report in reports {
        for row in &report.requirements {
            if matches!(row.outcome, Outcome::Fail | Outcome::Warn) {
                revisions_of
                    .entry(row.id.as_str())
                    .or_default()
                    .push(report.revision.as_str());
            }
        }
    }
    let mut ids = BTreeMap::new();
    for (id, revisions) in revisions_of {
        for revision in &revisions {
            let rule_id = if revisions.len() > 1 {
                format!("{id}@{revision}")
            } else {
                id.to_owned()
            };
            ids.insert((id, *revision), rule_id);
        }
    }
    ids
}

fn rule(rule_id: &str, row: &RequirementReport, revision: &str) -> Value {
    let mut rule = json!({
        "id": rule_id,
        "defaultConfiguration": { "level": level(row.outcome) },
        "properties": {
            "tags": ["mcp", "conformance", row.level],
            "precision": "very-high",
            "revision": revision,
        },
    });
    let (short, help) = row.source.as_ref().map_or_else(
        || (format!("{} ({})", row.id, row.level), None),
        |source| {
            (
                source.quote.clone(),
                Some((
                    json!({
                        "text": format!("{} ({}): \"{}\" — {}", row.id, row.level, source.quote, source.url),
                        "markdown": format!(
                            "**{} ({})**\n\n> {}\n\n[{}]({})",
                            row.id, row.level, source.quote, source.section, source.url
                        ),
                    }),
                    source.url.clone(),
                )),
            )
        },
    );
    rule["shortDescription"] = json!({ "text": short });
    if let Some((help, url)) = help {
        rule["fullDescription"] = json!({ "text": short });
        rule["help"] = help;
        rule["helpUri"] = json!(url);
    }
    rule
}

const fn level(outcome: Outcome) -> &'static str {
    match outcome {
        Outcome::Fail => "error",
        _ => "warning",
    }
}

/// The result's location: the trace, at the line holding the event `seq` names.
/// The reader accepts only one event per line with no blank lines, so the event
/// at index `i` is on line `i + 1`.
fn location(artifact: Artifact<'_>, seq: Option<u64>) -> Value {
    let Some(uri) = artifact.uri else {
        return json!([]);
    };
    let mut physical = json!({ "artifactLocation": { "uri": uri_reference(uri) } });
    let line = seq.and_then(|seq| {
        artifact
            .events
            .binary_search_by_key(&seq, |event| event.seq)
            .ok()
    });
    if let Some(index) = line {
        physical["region"] = json!({ "startLine": index + 1 });
    }
    json!([{ "physicalLocation": physical }])
}

/// `path` as a URI reference: `/`-separated, with every byte outside RFC 3986's
/// unreserved set and `/` percent-encoded.
fn uri_reference(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.replace('\\', "/").bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::reader::{Limits, parse_trace};
    use mcp_conformance_core::requirement::{Registry, RegistrySet};

    const WRONG_VERSION: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"1.0","id":1,"method":"ping"}}"#;

    fn log(reports: &[Report], uri: Option<&str>, events: &[TraceEvent]) -> Value {
        serde_json::from_str(&render(reports, Artifact { uri, events })).unwrap()
    }

    /// Two revisions sharing `BASE-001`, and `ONLY-001` at the later one only.
    fn two_revision_set() -> RegistrySet {
        RegistrySet::from_json(
            r#"{
            "revisions": ["2025-11-25", "2026-07-28"],
            "requirements": [
                {"id": "BASE-001", "level": "MUST", "actor": "both",
                 "source": {"section": "basic#x", "quote": "MUST be 2.0"},
                 "checks": ["base.jsonrpc-version"]},
                {"id": "ONLY-001", "level": "SHOULD", "actor": "both",
                 "applies": {"introduced": "2026-07-28"},
                 "source": {"section": "basic#y", "quote": "SHOULD be 2.0"},
                 "checks": ["base.jsonrpc-version"]}
            ]
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn an_id_judged_under_two_revisions_gets_a_rule_per_revision() {
        let set = two_revision_set();
        let events = parse_trace(WRONG_VERSION, &Limits::default()).unwrap();
        let reports: Vec<Report> = set
            .revisions()
            .iter()
            .map(|revision| crate::engine::validate(&set.registry(*revision).unwrap(), &events))
            .collect();
        let log = log(&reports, Some("t.jsonl"), &events);
        let ids: Vec<&str> = log["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|rule| rule["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            ["BASE-001@2025-11-25", "BASE-001@2026-07-28", "ONLY-001"]
        );
        let results = log["runs"][0]["results"].as_array().unwrap();
        let levels: Vec<(&str, &str, u64)> = results
            .iter()
            .map(|result| {
                (
                    result["ruleId"].as_str().unwrap(),
                    result["level"].as_str().unwrap(),
                    result["ruleIndex"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            levels,
            [
                ("BASE-001@2025-11-25", "error", 0),
                ("BASE-001@2026-07-28", "error", 1),
                ("ONLY-001", "warning", 2)
            ]
        );
        assert_eq!(
            log["runs"][0]["tool"]["driver"]["rules"][2]["defaultConfiguration"]["level"],
            "warning"
        );
        assert_eq!(
            log["runs"][0]["properties"]["revisions"],
            json!(["2025-11-25", "2026-07-28"])
        );
        assert_eq!(
            log["runs"][0]["invocations"][0]["executionSuccessful"],
            true
        );
    }

    #[test]
    fn an_unsupported_clause_is_a_failed_invocation_with_a_notification() {
        let registry = Registry::from_json(
            r#"{"revision": "2025-11-25", "requirements": [
                {"id": "FUTR-001", "level": "MUST", "actor": "both",
                 "source": {"section": "future#x", "quote": "MUST do future things"},
                 "checks": ["future.not-built-yet"]}]}"#,
        )
        .unwrap();
        let report = crate::engine::validate(&registry, &[]);
        let log = log(&[report], None, &[]);
        let invocation = &log["runs"][0]["invocations"][0];
        assert_eq!(invocation["executionSuccessful"], false);
        assert_eq!(
            invocation["toolExecutionNotifications"][0]["message"]["text"],
            "FUTR-001 (2025-11-25): the registry references checks this build does not \
             implement: future.not-built-yet"
        );
        assert_eq!(log["runs"][0]["results"], json!([]));
    }

    #[test]
    fn a_finding_without_a_seq_or_an_unknown_one_has_no_region() {
        let events = parse_trace(WRONG_VERSION, &Limits::default()).unwrap();
        let artifact = Artifact {
            uri: Some("t.jsonl"),
            events: &events,
        };
        assert_eq!(
            location(artifact, Some(0))[0]["physicalLocation"]["region"]["startLine"],
            1
        );
        assert!(
            location(artifact, None)[0]["physicalLocation"]
                .get("region")
                .is_none()
        );
        assert!(
            location(artifact, Some(9))[0]["physicalLocation"]
                .get("region")
                .is_none()
        );
    }

    #[test]
    fn paths_become_uri_references() {
        assert_eq!(
            uri_reference("corpus/a-b_c.~1.jsonl"),
            "corpus/a-b_c.~1.jsonl"
        );
        assert_eq!(
            uri_reference(r"traces\my run #2.jsonl"),
            "traces/my%20run%20%232.jsonl"
        );
        assert_eq!(uri_reference("é"), "%C3%A9");
    }
}
