// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `JUnit` XML rendering of validation reports, for CI systems that ingest test
//! result files.
//!
//! Mapping (documented because `JUnit` has no native concept of a warning):
//!
//! | Outcome | `JUnit` representation |
//! |---------|----------------------|
//! | `pass` | passing `<testcase>` |
//! | `fail` | `<failure>` per requirement, findings in the body |
//! | `warn` | passing `<testcase>` with findings in `<system-out>` — SHOULD-level findings do not fail CI unless promoted by `--strict`, and that promotion is an exit-code concern, not a report concern |
//! | `excluded` / `unsupported` / `not-applicable` | `<skipped>` with the reason as its message |
//! | an extension's clauses ([`Report::extensions`]) | a suite of their own per extension, named `… (revision, extension ID, not scored)`, whose `fail` and `warn` rows are passing test cases with the findings in `<system-out>` — an extension never fails a run (ADR-0023), so no `<failure>` is written for one, `--strict` or not |
//! | a trace that could not be judged ([`render_run`]) | a suite of one test case with an `<error>` giving the reason |
//! | the requests the trace ended without answering ([`Report::unanswered_requests`]) | a suite `<property>` named `unanswered-requests`, failing nothing |
//! | a finding a baseline accepts ([`Finding::baselined`](crate::report::Finding::baselined)) | never a `<failure>` and never a pass: a row all of whose findings are accepted is `<skipped>` (counted in `skipped`, not in `failures`) with the findings and their reasons in `<system-out>`; in a row that also has a counted finding, the accepted ones are in `<system-out>` and the rest are `<failure>`s |
//! | a baseline entry that matches no finding, or whose day has passed ([`render_run_with_baseline`]) | a suite `baseline`: a stale entry is a failing test case (skipped when stale entries are allowed), a lapsed one a skipped case |
//! | who broke it | a `fail` or `warn` case carries `<properties>`: `actor`, the party the clause binds, and `party`, the parties its findings are held against; each finding's location names its party (`[check] at seq 3 (client)`) |
//! | a party view ([`Report::party_view`]) | a `fail` or `warn` row whose every finding is the other party's is a passing case with the findings in `<system-out>`, headed `set aside`, and is not counted in `failures` — so the document agrees with the exit status the view gives |
//!
//! The output is deterministic (registry order, no timestamps, no hostnames) for the
//! same reason every other report format is: reports are artifacts.

use core::fmt::Write as _;

use crate::baseline::Notes;
use crate::report::{Report, Unjudged};

mod baseline;
mod extension;
mod rows;
mod text;

use baseline::{note_counts, render_notes};
use extension::{extension_counts, render_extension};
use rows::{counts, render_row, render_wire_schema};
use text::escape;
#[cfg(test)]
use text::location;

/// Renders the report as a single-suite `JUnit` XML document.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, junit};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let xml = junit::render(&engine::validate(&registry, &[]));
/// assert!(xml.starts_with(r#"<?xml version="1.0" encoding="UTF-8"?>"#));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render(report: &Report) -> String {
    render_all(core::slice::from_ref(report))
}

/// Renders several single-revision reports as one `JUnit` document.
///
/// One trace judged under each revision gives one `<testsuite>` per revision, in the
/// order given; the document-level counts are the sums of the suites'.
///
/// ```
/// use mcp_conformance_core::requirement::RegistrySet;
/// use mcp_trace_validator::{engine, junit};
///
/// let set = RegistrySet::builtin()?;
/// let reports: Vec<_> = set
///     .revisions()
///     .iter()
///     .filter_map(|revision| set.registry(*revision))
///     .map(|registry| engine::validate(&registry, &[]))
///     .collect();
/// let xml = junit::render_all(&reports);
/// assert_eq!(xml.matches("<testsuite ").count(), 2);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_all(reports: &[Report]) -> String {
    render_with(reports, &Options::default())
}

/// How [`render_with`] presents a run.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Options {
    /// The trace the reports judge, as the caller named it. Names each suite and
    /// becomes each test case's `file` and `classname` prefix, so a CI report
    /// aggregating several traces can tell their identically named clauses apart.
    pub trace: Option<String>,
    /// Render SHOULD-level findings (warnings) as `<failure>`s, matching a run
    /// whose exit status `--strict` makes them fail. Without it a warning is a
    /// passing test case with its findings in `<system-out>`.
    pub strict: bool,
}

impl Options {
    /// Options naming `trace`, otherwise the defaults.
    #[must_use]
    pub fn for_trace(trace: impl Into<String>) -> Self {
        Self {
            trace: Some(trace.into()),
            strict: false,
        }
    }

    /// These options, with warnings rendered as failures.
    #[must_use]
    pub const fn strict(mut self, strict: bool) -> Self {
        self.strict = strict;
        self
    }
}

/// Renders several single-revision reports as one `JUnit` document, as
/// [`render_all`] does, with the presentation `options` choose.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, junit};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let report = engine::validate(&registry, &[]);
/// let xml = junit::render_with(&[report], &junit::Options::for_trace("session.jsonl"));
/// assert!(xml.contains(r#"<testsuite name="session.jsonl (2025-11-25)""#));
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_with(reports: &[Report], options: &Options) -> String {
    render_traces(&[(options.clone(), reports)])
}

/// Renders the reports of several traces as one `JUnit` document: a
/// `<testsuite>` per trace and revision, each named by its trace's
/// [`Options::trace`], with the document-level counts summed across all.
///
/// ```
/// use mcp_conformance_core::requirement::Registry;
/// use mcp_trace_validator::{engine, junit};
///
/// let registry = Registry::builtin_2025_11_25()?;
/// let reports = [engine::validate(&registry, &[])];
/// let xml = junit::render_traces(&[
///     (junit::Options::for_trace("a.jsonl"), &reports[..]),
///     (junit::Options::for_trace("b.jsonl"), &reports[..]),
/// ]);
/// assert_eq!(xml.matches("<testsuite ").count(), 2);
/// # Ok::<(), Box<dyn core::error::Error>>(())
/// ```
#[must_use]
pub fn render_traces(traces: &[(Options, &[Report])]) -> String {
    render_run(traces, &[])
}

/// Renders a run over several traces as one `JUnit` document, with a suite
/// for each trace that could not be judged.
///
/// The judged traces render as [`render_traces`] renders them. Each unjudged
/// one is a `<testsuite>` of one test case carrying the reason as an
/// `<error>`, so a CI report shows the trace rather than silently leaving it
/// out; the root gains an `errors` count when there is one.
///
/// ```
/// use mcp_trace_validator::junit;
/// use mcp_trace_validator::report::Unjudged;
///
/// let xml = junit::render_run(&[], &[Unjudged::new("bad.jsonl", "malformed trace: line 4")]);
/// assert!(xml.contains(r#"<testsuites tests="1" failures="0" skipped="0" errors="1">"#));
/// assert!(xml.contains(r#"<error message="malformed trace: line 4"/>"#));
/// ```
#[must_use]
pub fn render_run(traces: &[(Options, &[Report])], unjudged: &[Unjudged]) -> String {
    render_run_with_baseline(traces, unjudged, &Notes::default())
}

/// Renders a run as [`render_run`] does, with what its baseline said about it
/// ([`crate::baseline`]).
///
/// The reports carry the findings a baseline accepts; `notes` carries what no
/// report can, the entries that match no finding of the run or whose day has
/// passed. They are a `<testsuite name="baseline">` of their own, after the
/// traces' — a stale entry a failing test case, as it fails the run, unless
/// the notes allow it, and a lapsed one a skipped case — and are counted in
/// the root. No notes, no suite: the document is [`render_run`]'s.
///
/// ```
/// use mcp_trace_validator::baseline::{Entry, Notes};
/// use mcp_trace_validator::junit;
///
/// let mut notes = Notes::default();
/// notes.stale.push(Entry::new("BASE-086:check:gone:1", "was fixed"));
/// let xml = junit::render_run_with_baseline(&[], &[], &notes);
/// assert!(xml.contains(r#"<testsuites tests="1" failures="1" skipped="0">"#));
/// assert!(xml.contains("stale baseline entry: BASE-086:check:gone:1"));
/// ```
#[must_use]
pub fn render_run_with_baseline(
    traces: &[(Options, &[Report])],
    unjudged: &[Unjudged],
    notes: &Notes,
) -> String {
    let suites: Vec<(&Options, &Report, (u32, u32, u32))> = traces
        .iter()
        .flat_map(|(options, reports)| {
            reports
                .iter()
                .map(move |report| (options, report, counts(report, options.strict)))
        })
        .collect();
    let extension_counts = suites
        .iter()
        .flat_map(|(_, report, _)| report.extensions.iter().map(extension_counts));
    let (tests, failures, skipped) = suites
        .iter()
        .map(|(_, _, count)| *count)
        .chain(extension_counts)
        .chain((!notes.is_empty()).then(|| note_counts(notes)))
        .fold((0, 0, 0), |sum, count| {
            (sum.0 + count.0, sum.1 + count.1, sum.2 + count.2)
        });
    let mut out = String::new();
    out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    out.push('\n');
    // `errors` only when there are some, so a run that judged every trace
    // renders exactly as it did before the attribute existed.
    let errors = if unjudged.is_empty() {
        String::new()
    } else {
        format!(r#" errors="{}""#, unjudged.len())
    };
    let _ = writeln!(
        out,
        r#"<testsuites tests="{}" failures="{failures}" skipped="{skipped}"{errors}>"#,
        tests as usize + unjudged.len()
    );
    for (options, report, counts) in suites {
        render_suite(&mut out, options, report, counts);
    }
    render_notes(&mut out, notes);
    for trace in unjudged {
        let name = escape(&trace.trace);
        let _ = writeln!(
            out,
            r#"  <testsuite name="{name}" tests="1" failures="0" errors="1" skipped="0">"#
        );
        let _ = writeln!(
            out,
            r#"    <testcase classname="{name}" file="{name}" name="trace could not be judged"><error message="{}"/></testcase>"#,
            escape(&trace.error)
        );
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

/// One report's suite, then a suite per extension it carries.
fn render_suite(out: &mut String, options: &Options, report: &Report, counts: (u32, u32, u32)) {
    let (tests, failures, skipped) = counts;
    let suite = options.trace.as_deref().unwrap_or("mcp-trace-validator");
    let _ = writeln!(
        out,
        r#"  <testsuite name="{} ({})" tests="{tests}" failures="{failures}" skipped="{skipped}">"#,
        escape(suite),
        escape(&report.revision)
    );
    // A note on the suite, where `JUnit` keeps what is not a test case: the
    // requests the trace ended without answering, failing nothing.
    if let Some(unanswered) = &report.unanswered_requests {
        let _ = writeln!(
            out,
            r#"    <properties><property name="unanswered-requests" value="{}"/></properties>"#,
            escape(&unanswered.one_line())
        );
    }
    for row in &report.requirements {
        render_row(out, report, row, options);
    }
    if let Some(wire_schema) = &report.wire_schema {
        render_wire_schema(out, report, wire_schema, options);
    }
    out.push_str("  </testsuite>\n");
    for extension in &report.extensions {
        render_extension(out, suite, report, extension, options);
    }
}

#[cfg(test)]
mod tests;
