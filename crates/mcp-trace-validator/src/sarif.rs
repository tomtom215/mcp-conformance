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
//! | a finding a baseline accepts ([`Finding::baselined`](crate::report::Finding::baselined)) | a result at level `note` with `properties.counted: false` and SARIF's own `suppressions` (`kind: external`, `status: accepted`, the entry's reason as `justification`); a baseline entry that matches no finding is a `warning` notification, an expired one a `note` ([`render_run_with_baseline`]) |
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

use mcp_conformance_core::trace::TraceEvent;
use serde_json::{Value, json};

use crate::baseline::Notes;
use crate::fingerprint::{Fingerprints, KEY, Place, Slot, WIRE_SCHEMA_RULE};
use crate::report::{BaselineTally, Outcome, Report, RequirementReport, Unjudged};

mod baseline;
mod location;
mod notify;
mod rule;

use location::location;
#[cfg(test)]
use location::uri_reference;
use rule::{level, rule};

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
    render_run_with_baseline(traces, unjudged, options, &Notes::default())
}

/// Renders a run as [`render_run`] does, with what its baseline said about it
/// ([`crate::baseline`]).
///
/// A finding a baseline accepts is a result with a `suppressions` entry —
/// SARIF's own: `kind: external`, `status: accepted`, the entry's reason as the
/// `justification` — at level `note`, so a consumer that ignores suppressions
/// still does not fail on it. `notes` carries what no result can: a stale
/// entry is a `warning` notification and a lapsed one a `note`, and the run's
/// `properties.baseline` counts the findings accepted and not. No baseline, no
/// such members: the log is [`render_run`]'s.
///
/// ```
/// use mcp_trace_validator::baseline::{Entry, Notes};
/// use mcp_trace_validator::sarif;
///
/// let mut notes = Notes::default();
/// notes.stale.push(Entry::new("BASE-086:check:gone:1", "was fixed"));
/// let log: serde_json::Value = serde_json::from_str(&sarif::render_run_with_baseline(
///     &[],
///     &[],
///     &sarif::Options::default(),
///     &notes,
/// ))?;
/// let note = &log["runs"][0]["invocations"][0]["toolExecutionNotifications"][0];
/// assert_eq!(note["level"], "warning");
/// assert_eq!(note["properties"]["kind"], "baseline-stale");
/// # Ok::<(), serde_json::Error>(())
/// ```
#[must_use]
pub fn render_run_with_baseline(
    traces: &[(Artifact<'_>, &[Report])],
    unjudged: &[Unjudged],
    options: &Options,
    notes: &Notes,
) -> String {
    // A finding's fingerprint is the baseline's key as well ([`crate::fingerprint`]),
    // so both are computed in one place, in one order.
    let named: Vec<_> = traces
        .iter()
        .map(|(artifact, reports)| (artifact.uri, *reports))
        .collect();
    let mut log = Log {
        fingerprints: Fingerprints::of(&named),
        index_of: std::collections::BTreeMap::new(),
        rules: Vec::new(),
        results: Vec::new(),
        notifications: Vec::new(),
        strict: options.strict,
        party_view: None,
        baseline_applied: false,
        noted: std::collections::BTreeSet::new(),
        at: (0, 0),
    };
    for (trace, (artifact, reports)) in traces.iter().enumerate() {
        for (index, report) in reports.iter().enumerate() {
            log.at = (trace, index);
            log.report(report, *artifact);
        }
    }
    for trace in unjudged {
        log.unjudged(trace);
    }
    log.notifications.extend(baseline::notifications(notes));
    let properties = run_properties(traces);
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

/// The run's `properties`: the revisions judged, the party view every report
/// was rendered under (when they agree — one `validate` invocation gives every
/// report the same one), and how the findings fell under a baseline, when one
/// was applied.
fn run_properties(traces: &[(Artifact<'_>, &[Report])]) -> Value {
    let all_reports = || traces.iter().flat_map(|(_, reports)| reports.iter());
    let mut revisions: Vec<&str> = Vec::new();
    for report in all_reports() {
        if !revisions.contains(&report.revision.as_str()) {
            revisions.push(&report.revision);
        }
    }
    let party_view = all_reports()
        .map(|report| report.party_view)
        .reduce(|first, next| if first == next { first } else { None })
        .flatten();
    let mut properties = json!({ "revisions": revisions });
    if let Some(party) = party_view {
        properties["party_view"] = json!(party);
    }
    if all_reports().any(|report| report.baseline_applied) {
        let tally = all_reports().fold(BaselineTally::default(), |sum, report| {
            sum.plus(report.baseline_tally())
        });
        properties["baseline"] = json!(tally);
    }
    properties
}

/// The run as it is assembled: rules in order of first finding, their results,
/// and the notifications for clauses this build cannot judge.
struct Log<'r> {
    /// Every finding's fingerprint, and the rule each requirement's findings
    /// are reported under.
    fingerprints: Fingerprints<'r>,
    index_of: std::collections::BTreeMap<String, usize>,
    rules: Vec<Value>,
    results: Vec<Value>,
    notifications: Vec<Value>,
    /// Warnings are reported at level `error`.
    strict: bool,
    /// The party the report being added is viewed for
    /// ([`Report::party_view`]): a finding it sets aside is a `note`.
    party_view: Option<crate::report::Party>,
    /// Whether a baseline was applied to the report being added: a finding it
    /// accepts is a suppressed `note`.
    baseline_applied: bool,
    /// The unanswered-requests notes already written, per trace.
    noted: std::collections::BTreeSet<(String, String)>,
    /// The trace and report (of that trace) being added.
    at: (usize, usize),
}

impl Log<'_> {
    /// Everything one report adds to the run: its rows' results and
    /// notifications, its failed wire schema, then its extensions'.
    fn report(&mut self, report: &Report, artifact: Artifact<'_>) {
        self.party_view = report.party_view;
        self.baseline_applied = report.baseline_applied;
        if let Some(unanswered) = &report.unanswered_requests {
            self.unanswered(unanswered, artifact);
        }
        for (index, row) in report.requirements.iter().enumerate() {
            match row.outcome {
                Outcome::Fail | Outcome::Warn => {
                    self.results_for(row, Place::Row(index), &report.revision, artifact, None);
                }
                Outcome::Unsupported => self.unsupported(row, &report.revision),
                _ => {}
            }
        }
        if let Some(row) = wire_schema_row(report) {
            self.results_for(&row, Place::WireSchema, &report.revision, artifact, None);
        }
        for (extension_index, extension) in report.extensions.iter().enumerate() {
            for (index, row) in extension.requirements.iter().enumerate() {
                match row.outcome {
                    // At level `note`: reported, never scored.
                    Outcome::Fail | Outcome::Warn => {
                        let place = Place::Extension {
                            extension: extension_index,
                            row: index,
                        };
                        self.results_for(
                            row,
                            place,
                            &report.revision,
                            artifact,
                            Some(&extension.extension),
                        );
                    }
                    Outcome::Unsupported => self.unsupported(row, &report.revision),
                    _ => {}
                }
            }
        }
    }

    /// A result per finding on a `fail` / `warn` row, under the row's rule.
    fn results_for(
        &mut self,
        row: &RequirementReport,
        place: Place,
        revision: &str,
        artifact: Artifact<'_>,
        extension: Option<&str>,
    ) {
        let rule_id = self.fingerprints.rule_id(&row.id, revision);
        let rules = &mut self.rules;
        let rule_index = *self.index_of.entry(rule_id.clone()).or_insert_with(|| {
            rules.push(rule(&rule_id, row, revision, extension));
            rules.len() - 1
        });
        for (position, finding) in row.findings.iter().enumerate() {
            // Stable across recordings of the same behaviour, and shared with
            // the baseline: see [`crate::fingerprint`] for what it is made of.
            let fingerprint = self
                .fingerprints
                .get(Slot {
                    trace: self.at.0,
                    report: self.at.1,
                    place,
                    finding: position,
                })
                .unwrap_or_default();
            // A finding the party view sets aside is shown at level `note`,
            // like an extension's: reported, and failing nothing.
            let set_aside = self
                .party_view
                .is_some_and(|party| !finding.counts_for(party));
            // So is a finding a baseline accepts, which also carries SARIF's own
            // `suppression` saying so and why.
            let accepted = finding.baselined.as_ref();
            let level = if extension.is_some() || set_aside || accepted.is_some() {
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
                "partialFingerprints": { KEY: fingerprint },
                "properties": {
                    "revision": revision,
                    "check": finding.check,
                    "seq": finding.seq,
                },
            });
            if let Some(party) = finding.party {
                result["properties"]["party"] = json!(party);
            }
            if self.party_view.is_some() || self.baseline_applied {
                result["properties"]["counted"] = json!(!set_aside && accepted.is_none());
            }
            if let Some(acceptance) = accepted {
                result["suppressions"] = baseline::suppressions(acceptance);
            }
            if let Some(extension) = extension {
                result["properties"]["extension"] = json!(extension);
                result["properties"]["scored"] = json!(false);
            }
            self.results.push(result);
        }
    }
}

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
