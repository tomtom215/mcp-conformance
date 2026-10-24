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
//! | a failed wire schema ([`Report::wire_schema`]) | the rule `wire-schema` (the specification's statement that the schema is normative, and the vendored schema's address), a result per invalid message, at level `error` |
//! | a finding on an extension's `fail` / `warn` row ([`Report::extensions`]) | a result at level `note` under the clause's rule, its `properties` naming the extension and `scored: false` — an extension never fails a run (ADR-0023), `--strict` or not |
//! | who broke it | each rule's `properties.actor` is the party the clause binds; each result's `properties.party` is the party its finding is held against, absent when the trace cannot say |
//! | a party view ([`Report::party_view`]) | the run's `properties.party_view`; a result the view sets aside is at level `note` with `properties.counted: false`, every other result `counted: true` |
//! | an `unsupported` row | an invocation notification, and `executionSuccessful: false` |
//! | a trace that could not be judged ([`render_run`]) | an invocation notification located at the trace, and `executionSuccessful: false` |
//! | the requests a trace ended without answering ([`Report::unanswered_requests`]) | an invocation notification at level `note`, located at the trace, with the requests in its `properties`; `executionSuccessful` is untouched |
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

use std::collections::BTreeMap;

use mcp_conformance_core::trace::TraceEvent;
use serde_json::{Value, json};

use crate::report::{Outcome, Report, RequirementReport, Unjudged};

mod location;
mod rule;

use location::location;
#[cfg(test)]
use location::uri_reference;
use rule::{fold_numbers, level, rule};

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
    render_traces(&[(artifact, reports)], options)
}

/// Renders the reports of several traces as one SARIF 2.1.0 log: one run, its
/// rules shared, each result located in the trace it came from — the shape code
/// scanning takes in one upload.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, reader, sarif};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list"}}"#;
/// let events = reader::parse_trace(trace, &reader::Limits::default())?;
/// let reports = [engine::validate(&registry, &events)];
/// let artifact = |uri| sarif::Artifact { uri: Some(uri), events: &events };
/// let log: serde_json::Value = serde_json::from_str(&sarif::render_traces(
///     &[(artifact("a.jsonl"), &reports[..]), (artifact("b.jsonl"), &reports[..])],
///     &sarif::Options::default(),
/// ))?;
/// assert_eq!(log["runs"].as_array().map(Vec::len), Some(1));
/// let uris: Vec<_> = log["runs"][0]["results"]
///     .as_array()
///     .into_iter()
///     .flatten()
///     .map(|result| &result["locations"][0]["physicalLocation"]["artifactLocation"]["uri"])
///     .collect();
/// assert!(uris.contains(&&serde_json::json!("a.jsonl")) && uris.contains(&&serde_json::json!("b.jsonl")));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_traces(traces: &[(Artifact<'_>, &[Report])], options: &Options) -> String {
    render_run(traces, &[], options)
}

/// Renders a run over several traces as one SARIF 2.1.0 log, with a
/// notification for each trace that could not be judged.
///
/// The judged traces render as [`render_traces`] renders them. Each unjudged
/// one is an error-level tool execution notification, located at the trace
/// with the reason as its message, and the invocation reports
/// `executionSuccessful: false`.
///
/// ```
/// use mcp_trace_validator::report::Unjudged;
/// use mcp_trace_validator::sarif;
///
/// let unjudged = [Unjudged::new("bad.jsonl", "malformed trace: line 4")];
/// let log: serde_json::Value =
///     serde_json::from_str(&sarif::render_run(&[], &unjudged, &sarif::Options::default()))?;
/// let invocation = &log["runs"][0]["invocations"][0];
/// assert_eq!(invocation["executionSuccessful"], false);
/// assert_eq!(
///     invocation["toolExecutionNotifications"][0]["message"]["text"],
///     "malformed trace: line 4"
/// );
/// # Ok::<(), serde_json::Error>(())
/// ```
#[must_use]
pub fn render_run(
    traces: &[(Artifact<'_>, &[Report])],
    unjudged: &[Unjudged],
    options: &Options,
) -> String {
    let all_reports = || traces.iter().flat_map(|(_, reports)| reports.iter());
    let mut log = Log {
        rule_ids: rule_ids(all_reports()),
        index_of: BTreeMap::new(),
        rules: Vec::new(),
        results: Vec::new(),
        notifications: Vec::new(),
        strict: options.strict,
        party_view: None,
        seen: BTreeMap::new(),
        noted: std::collections::BTreeSet::new(),
    };
    for (artifact, reports) in traces {
        for report in *reports {
            log.report(report, *artifact);
        }
    }
    for trace in unjudged {
        log.unjudged(trace);
    }
    let mut revisions: Vec<&str> = Vec::new();
    for report in all_reports() {
        if !revisions.contains(&report.revision.as_str()) {
            revisions.push(&report.revision);
        }
    }
    // The view every report of the run was rendered under, when they agree
    // (one `validate` invocation gives every report the same one).
    let party_view = all_reports()
        .map(|report| report.party_view)
        .reduce(|first, next| if first == next { first } else { None })
        .flatten();
    let mut properties = json!({ "revisions": revisions });
    if let Some(party) = party_view {
        properties["party_view"] = json!(party);
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
                // A note is information, not a failed execution.
                "executionSuccessful": !log.notifications.iter().any(|note| note["level"] == "error"),
                "toolExecutionNotifications": log.notifications,
            }],
            "results": log.results,
            "properties": properties,
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
    /// The party the report being added is viewed for
    /// ([`Report::party_view`]): a finding it sets aside is a `note`.
    party_view: Option<crate::report::Party>,
    /// How many results so far share each fingerprint stem, per trace.
    seen: BTreeMap<(String, String), usize>,
    /// The unanswered-requests notes already written, per trace.
    noted: std::collections::BTreeSet<(String, String)>,
}

impl Log<'_> {
    /// Everything one report adds to the run: its rows' results and
    /// notifications, its failed wire schema, then its extensions'.
    fn report(&mut self, report: &Report, artifact: Artifact<'_>) {
        self.party_view = report.party_view;
        if let Some(unanswered) = &report.unanswered_requests {
            self.unanswered(unanswered, artifact);
        }
        for row in &report.requirements {
            match row.outcome {
                Outcome::Fail | Outcome::Warn => self.findings(row, &report.revision, artifact),
                Outcome::Unsupported => self.unsupported(row, &report.revision),
                _ => {}
            }
        }
        if let Some(row) = wire_schema_row(report) {
            self.findings(&row, &report.revision, artifact);
        }
        for extension in &report.extensions {
            for row in &extension.requirements {
                match row.outcome {
                    Outcome::Fail | Outcome::Warn => self.extension_findings(
                        row,
                        &report.revision,
                        artifact,
                        &extension.extension,
                    ),
                    Outcome::Unsupported => self.unsupported(row, &report.revision),
                    _ => {}
                }
            }
        }
    }

    /// A result per finding on a `fail` / `warn` row, under the row's rule.
    fn findings(&mut self, row: &RequirementReport, revision: &str, artifact: Artifact<'_>) {
        self.results_for(row, revision, artifact, None);
    }

    /// A result per finding on an extension's `fail` / `warn` row, at level
    /// `note`: reported, never scored.
    fn extension_findings(
        &mut self,
        row: &RequirementReport,
        revision: &str,
        artifact: Artifact<'_>,
        extension: &str,
    ) {
        self.results_for(row, revision, artifact, Some(extension));
    }

    fn results_for(
        &mut self,
        row: &RequirementReport,
        revision: &str,
        artifact: Artifact<'_>,
        extension: Option<&str>,
    ) {
        let rule_id = self.rule_ids[&(row.id.as_str(), revision)].clone();
        let rules = &mut self.rules;
        let rule_index = *self.index_of.entry(rule_id.clone()).or_insert_with(|| {
            rules.push(rule(&rule_id, row, revision, extension));
            rules.len() - 1
        });
        for finding in &row.findings {
            // Stable across recordings of the same behaviour: a re-recorded
            // trace moves every `seq`, and a fingerprint keyed on it would
            // close and reopen every alert on each run. The detail with its
            // numbers folded identifies the defect; the occurrence index keeps
            // repeated identical findings distinct.
            // Counted per trace: code scanning already tells traces apart by
            // their location, so the trace's path stays out of the fingerprint
            // and a renamed trace keeps its alerts.
            let stem = format!(
                "{rule_id}:{}:{}",
                finding.check,
                fold_numbers(&finding.detail)
            );
            let occurrence = self
                .seen
                .entry((artifact.uri.unwrap_or_default().to_owned(), stem.clone()))
                .or_insert(0);
            *occurrence += 1;
            // A finding the party view sets aside is shown at level `note`,
            // like an extension's: reported, and failing nothing.
            let set_aside = self
                .party_view
                .is_some_and(|party| !finding.counts_for(party));
            let level = if extension.is_some() || set_aside {
                NOT_SCORED
            } else if self.strict && row.outcome == Outcome::Warn {
                "error"
            } else {
                level(row.outcome)
            };
            let mut result = json!({
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
            });
            if let Some(party) = finding.party {
                result["properties"]["party"] = json!(party);
            }
            if self.party_view.is_some() {
                result["properties"]["counted"] = json!(!set_aside);
            }
            if let Some(extension) = extension {
                result["properties"]["extension"] = json!(extension);
                result["properties"]["scored"] = json!(false);
            }
            self.results.push(result);
        }
    }

    /// The requests a trace ended without answering: a `note`-level
    /// notification located at the trace, once per trace — the reports of one
    /// trace judged under several revisions all carry the same note.
    fn unanswered(
        &mut self,
        unanswered: &crate::report::UnansweredRequests,
        artifact: Artifact<'_>,
    ) {
        let text = unanswered.one_line();
        let key = (artifact.uri.unwrap_or_default().to_owned(), text.clone());
        if !self.noted.insert(key) {
            return;
        }
        self.notifications.push(json!({
            "level": "note",
            "message": { "text": text },
            "locations": location(artifact, None),
            "properties": {
                "kind": "unanswered-requests",
                "count": unanswered.count,
                "ending": unanswered.ending,
                "requests": unanswered.requests,
            },
        }));
    }

    /// A trace the run could not judge at all, located at the trace.
    fn unjudged(&mut self, trace: &Unjudged) {
        let at = Artifact {
            uri: Some(&trace.trace),
            events: &[],
        };
        self.notifications.push(json!({
            "level": "error",
            "message": { "text": trace.error },
            "locations": location(at, None),
        }));
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
fn rule_ids<'r>(reports: impl Iterator<Item = &'r Report>) -> BTreeMap<(&'r str, &'r str), String> {
    let mut revisions_of: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for report in reports {
        let schema = report.wire_schema_failed().then_some(WIRE_SCHEMA_RULE);
        let extension_rows = report
            .extensions
            .iter()
            .flat_map(|extension| &extension.requirements);
        let failing = report
            .requirements
            .iter()
            .chain(extension_rows)
            .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
            .map(|row| row.id.as_str());
        for id in failing.chain(schema) {
            let revisions = revisions_of.entry(id).or_default();
            // Several traces judged at one revision are still one revision.
            if !revisions.contains(&report.revision.as_str()) {
                revisions.push(report.revision.as_str());
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

/// The rule ID of wire-schema results: lower case, so it can never collide
/// with a requirement ID.
const WIRE_SCHEMA_RULE: &str = "wire-schema";

/// A failed wire schema as a row the result path already knows how to render;
/// `None` unless it failed.
fn wire_schema_row(report: &Report) -> Option<RequirementReport> {
    let wire_schema = report
        .wire_schema
        .as_ref()
        .filter(|_| report.wire_schema_failed())?;
    Some(RequirementReport {
        id: WIRE_SCHEMA_RULE.to_owned(),
        level: "schema".to_owned(),
        // The schema is no clause and binds no actor; each finding carries
        // the sender it is held against.
        actor: None,
        outcome: Outcome::Fail,
        findings: wire_schema.findings.clone(),
        exclusion: None,
        missing_checks: Vec::new(),
        capability: None,
        source: Some(wire_schema.source.clone()),
    })
}

/// The level of an extension's results: shown, and failing nothing.
const NOT_SCORED: &str = "note";

#[cfg(test)]
mod tests;
