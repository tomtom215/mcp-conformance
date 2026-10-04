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
    render_with(reports, artifact, &Options::default())
}

/// How [`render_with`] presents a run.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct Options {
    /// Report SHOULD-level findings (warnings) at level `error`, matching a run
    /// whose exit status `--strict` makes them fail.
    pub strict: bool,
}

impl Options {
    /// These options, with warnings reported as errors.
    #[must_use]
    pub const fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

/// Renders the reports as one SARIF 2.1.0 log, as [`render`] does, with the
/// presentation `options` choose.
#[must_use]
pub fn render_with(reports: &[Report], artifact: Artifact<'_>, options: &Options) -> String {
    let mut log = Log {
        rule_ids: rule_ids(reports),
        index_of: BTreeMap::new(),
        rules: Vec::new(),
        results: Vec::new(),
        notifications: Vec::new(),
        strict: options.strict,
        seen: BTreeMap::new(),
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
    /// Warnings are reported at level `error`.
    strict: bool,
    /// How many results so far share each fingerprint stem.
    seen: BTreeMap<String, usize>,
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
            // Stable across recordings of the same behaviour: a re-recorded
            // trace moves every `seq`, and a fingerprint keyed on it would
            // close and reopen every alert on each run. The detail with its
            // numbers folded identifies the defect; the occurrence index keeps
            // repeated identical findings distinct.
            let stem = format!(
                "{rule_id}:{}:{}",
                finding.check,
                fold_numbers(&finding.detail)
            );
            let occurrence = self.seen.entry(stem.clone()).or_insert(0);
            *occurrence += 1;
            let level = if self.strict && row.outcome == Outcome::Warn {
                "error"
            } else {
                level(row.outcome)
            };
            self.results.push(json!({
                "ruleId": rule_id,
                "ruleIndex": rule_index,
                "level": level,
                "message": { "text": finding.detail },
                "locations": location(artifact, finding.seq),
                "partialFingerprints": {
                    "mcpConformanceFinding/v2": format!("{stem}:{occurrence}"),
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

/// `text` with every run of ASCII digits replaced by `#`, so a finding's
/// identity does not depend on the `seq` and ids a particular recording carries.
fn fold_numbers(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut in_digits = false;
    for character in text.chars() {
        if character.is_ascii_digit() {
            if !in_digits {
                folded.push('#');
            }
            in_digits = true;
        } else {
            folded.push(character);
            in_digits = false;
        }
    }
    folded
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

/// `path` as a URI reference, `/`-separated with every byte outside RFC 3986's
/// unreserved set and `/` percent-encoded.
///
/// A relative path stays relative — the form code scanning resolves against the
/// repository root — and its colons stay encoded, so none can be read as a
/// scheme. An absolute path becomes a `file:` URI (RFC 8089): `/tmp/t.jsonl` is
/// `file:///tmp/t.jsonl`, and Windows' `D:\t.jsonl` is `file:///D:/t.jsonl`, its
/// drive letter intact rather than the relative `D%3A/t.jsonl` it would
/// otherwise read as.
fn uri_reference(path: &str) -> String {
    let path = path.replace('\\', "/");
    let bytes = path.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    let (mut out, rest) = if drive {
        (format!("file:///{}:", char::from(bytes[0])), &path[2..])
    } else if path.starts_with('/') {
        ("file://".to_owned(), path.as_str())
    } else {
        (String::with_capacity(path.len()), path.as_str())
    };
    for byte in rest.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests;
