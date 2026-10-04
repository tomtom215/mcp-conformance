// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `validate` subcommand: judge one or more traces and report on them.
//!
//! Judging and rendering are separate steps. Each trace is judged on its own —
//! its revision chosen from what it declares, unless `--revision` says
//! otherwise — and then either rendered alone (one trace: the report exactly as
//! it has always been) or together with the others (several traces: one
//! document per format, so CI uploads one `JUnit` file and one SARIF log). The
//! exit status is the worst of the traces': a trace that could not be judged
//! counts, so a broken recording in a batch is never hidden by its neighbours.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fmt::Write as _;
use std::path::Path;

use mcp_conformance_core::requirement::{Registry, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::declared::{self, RevisionSource};
use mcp_trace_validator::multi::{self, MultiReport};
use mcp_trace_validator::report::{Report, Verdict};
use mcp_trace_validator::{engine, junit, reader, sarif, sessions};
use serde_json::json;

use crate::emit::emit;
use crate::{
    EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, Format, input, judgeable, load_registry, load_registry_set,
    parse_revisions,
};

/// How `validate` presents its report.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Output {
    pub(crate) format: Format,
    /// SHOULD-level findings fail the run.
    pub(crate) strict: bool,
    /// Human output lists every clause, not only those needing attention.
    pub(crate) all: bool,
}

/// Where the registries come from, resolved once for every trace in the run.
pub(crate) struct Sources<'a> {
    pub(crate) registry: Option<&'a Path>,
    pub(crate) registry_set: Option<&'a Path>,
    pub(crate) revisions: &'a [String],
}

enum Registries {
    /// `--registry`: one custom registry, every trace judged against it.
    Custom(Registry),
    /// The built-in set (or `--registry-set`), with any `--revision` requests.
    Set {
        set: RegistrySet,
        requested: Vec<ProtocolRevision>,
    },
}

/// One trace, judged.
struct Judged {
    trace: String,
    events: Vec<TraceEvent>,
    /// One report per judged revision, in order.
    reports: Vec<Report>,
    /// The cross-revision view, when more than one revision was judged.
    multi: Option<MultiReport>,
}

impl Judged {
    fn verdict(&self) -> Verdict {
        self.multi
            .as_ref()
            .map_or_else(|| self.reports[0].verdict(), MultiReport::verdict)
    }

    fn render_human(&self, all: bool) -> String {
        match (&self.multi, all) {
            (Some(multi), true) => multi.render_human(),
            (Some(multi), false) => multi.render_findings(),
            (None, true) => self.reports[0].render_human(),
            (None, false) => self.reports[0].render_findings(),
        }
    }

    fn to_json(&self) -> serde_json::Result<serde_json::Value> {
        self.multi.as_ref().map_or_else(
            || serde_json::to_value(&self.reports[0]),
            serde_json::to_value,
        )
    }
}

/// Runs `validate` over `traces`, returning the exit status.
pub(crate) fn run(
    traces: &[String],
    limits: &reader::Limits,
    output: Output,
    sources: &Sources<'_>,
) -> u8 {
    if traces.len() > 1 && traces.iter().any(|trace| trace == "-") {
        eprintln!("error: `-` (stdin) can only be validated on its own, not with other traces");
        return EXIT_USAGE;
    }
    let registries = match resolve(sources) {
        Ok(registries) => registries,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let many = traces.len() > 1;
    let mut code = EXIT_OK;
    let mut judged = Vec::new();
    for trace in traces {
        match judge(trace, limits, &registries, many) {
            Ok(one) => judged.push(one),
            Err(failed) => code = code.max(failed),
        }
    }
    let rendered = if many {
        render_many(&judged, traces.len(), output)
    } else if let [one] = judged.as_slice() {
        render_one(one, output)
    } else {
        return code; // The one trace could not be judged; its error is printed.
    };
    rendered.map_or(EXIT_USAGE, |rendered| code.max(rendered))
}

fn resolve(sources: &Sources<'_>) -> Result<Registries, String> {
    if let Some(path) = sources.registry {
        if sources.registry_set.is_some() || !sources.revisions.is_empty() {
            return Err(
                "--registry names one custom registry; it cannot be combined with \
                 --revision or --registry-set"
                    .to_owned(),
            );
        }
        return load_registry(path).map(Registries::Custom);
    }
    let set = load_registry_set(sources.registry_set)?;
    let requested = parse_revisions(sources.revisions)?;
    if let Some(unknown) = requested
        .iter()
        .find(|revision| !set.revisions().contains(revision))
    {
        return Err(format!(
            "registry set does not describe revision {unknown} (supported: {})",
            set.revisions()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(Registries::Set { set, requested })
}

/// Reads and judges one trace. `Err` carries the exit status of a trace that
/// could not be judged; its reason is already on stderr.
fn judge(
    trace: &str,
    limits: &reader::Limits,
    registries: &Registries,
    many: bool,
) -> Result<Judged, u8> {
    let prefix = if many {
        format!("{trace}: ")
    } else {
        String::new()
    };
    let events = input::read_events(trace, limits)?;
    judgeable::note_sessions(&prefix, sessions::recorded_sessions(&events));
    let (reports, multi) = match registries {
        Registries::Custom(registry) => (vec![engine::validate(registry, &events)], None),
        Registries::Set { set, requested } => judge_in_set(set, requested, &events, &prefix)?,
    };
    let totals = multi
        .as_ref()
        .map_or(reports[0].totals, judgeable::combined);
    if judgeable::reject(totals, trace) {
        return Err(EXIT_USAGE);
    }
    Ok(Judged {
        trace: trace.to_owned(),
        events,
        reports,
        multi,
    })
}

/// Judges `events` against the set: at the revisions requested, else at those
/// the trace declares. One report per revision, and the cross-revision view
/// when there are several.
fn judge_in_set(
    set: &RegistrySet,
    requested: &[ProtocolRevision],
    events: &[TraceEvent],
    prefix: &str,
) -> Result<(Vec<Report>, Option<MultiReport>), u8> {
    let (chosen, source) = if requested.is_empty() {
        declared::select(set.revisions(), events)
            .map(|selection| (selection.revisions, selection.source))
            .map_err(|error| {
                eprintln!(
                    "error: {prefix}{error}\nhint: pass --revision YYYY-MM-DD to judge it \
                     against a supported revision anyway"
                );
                EXIT_USAGE
            })?
    } else {
        (requested.to_vec(), RevisionSource::Requested)
    };
    // A trace with no messages is refused as contentless right after this; a
    // note about the revision it did not declare would only bury that.
    let has_messages = events.iter().any(|event| event.message_payload().is_some());
    if source == RevisionSource::Default && has_messages {
        eprintln!(
            "note: {prefix}the trace declares no protocol revision; judging it against {}, \
             the newest supported (use --revision to choose)",
            chosen[0]
        );
    }
    let mut reports: Vec<Report> = chosen
        .iter()
        .filter_map(|revision| set.registry(*revision))
        .map(|registry| engine::validate(&registry, events))
        .collect();
    if let [report] = reports.as_mut_slice() {
        report.revision_source = Some(source);
        return Ok((reports, None));
    }
    let mut multi = multi::validate_revisions(set, &chosen, events).map_err(|error| {
        eprintln!("error: {prefix}{error}");
        EXIT_USAGE
    })?;
    multi.revision_source = Some(source);
    Ok((reports, Some(multi)))
}

/// One trace's report, exactly as `validate` has always rendered it; `None`
/// when it could not be written.
fn render_one(one: &Judged, output: Output) -> Option<u8> {
    let written = match output.format {
        Format::Human => emit(&one.render_human(output.all)),
        Format::Json => match one
            .to_json()
            .and_then(|json| serde_json::to_string_pretty(&json))
        {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return None;
            }
        },
        Format::Junit => emit(&junit::render_with(
            &one.reports,
            &junit_options(&one.trace, output.strict),
        )),
        Format::Sarif => emit(&sarif::render_with(
            &one.reports,
            artifact(&one.trace, &one.events),
            &sarif::Options::default().strict(output.strict),
        )),
    };
    written.then(|| verdict_to_code(one.verdict(), output.strict))
}

/// Several traces' reports as one document per format; `None` when it could
/// not be written.
fn render_many(judged: &[Judged], requested: usize, output: Output) -> Option<u8> {
    let overall = worst(judged.iter().map(Judged::verdict));
    let unjudged = requested - judged.len();
    let written = match output.format {
        Format::Human => emit(&human_many(judged, unjudged, overall, output.all)),
        Format::Json => emit(&json_many(judged, unjudged, overall)?),
        Format::Junit => {
            let suites: Vec<(junit::Options, &[Report])> = judged
                .iter()
                .map(|one| {
                    (
                        junit_options(&one.trace, output.strict),
                        one.reports.as_slice(),
                    )
                })
                .collect();
            emit(&junit::render_traces(&suites))
        }
        Format::Sarif => {
            let traces: Vec<(sarif::Artifact<'_>, &[Report])> = judged
                .iter()
                .map(|one| (artifact(&one.trace, &one.events), one.reports.as_slice()))
                .collect();
            emit(&sarif::render_traces(
                &traces,
                &sarif::Options::default().strict(output.strict),
            ))
        }
    };
    written.then(|| {
        judged
            .iter()
            .map(|one| verdict_to_code(one.verdict(), output.strict))
            .max()
            .unwrap_or(EXIT_OK)
    })
}

/// Several traces' human reports, each under a `==> trace <==` header, then
/// how many reached each verdict and the overall one.
fn human_many(judged: &[Judged], unjudged: usize, overall: Option<Verdict>, all: bool) -> String {
    let mut text = String::new();
    for one in judged {
        let _ = writeln!(text, "==> {} <==", one.trace);
        text.push_str(&one.render_human(all));
        text.push('\n');
    }
    text.push_str(&summary(judged, unjudged));
    let _ = writeln!(
        text,
        "overall verdict: {}",
        overall.map_or_else(|| "none".to_owned(), |verdict| verdict.to_string())
    );
    text
}

/// Several traces' JSON reports in one document: `{"verdict", "traces":
/// [{"trace", "report"}], "unjudged"}` (`report.schema.json`'s third shape).
/// `None` when a report could not be serialized.
fn json_many(judged: &[Judged], unjudged: usize, overall: Option<Verdict>) -> Option<String> {
    let mut traces = Vec::new();
    for one in judged {
        match one.to_json() {
            Ok(report) => traces.push(json!({ "trace": one.trace, "report": report })),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return None;
            }
        }
    }
    let document = json!({ "verdict": overall, "traces": traces, "unjudged": unjudged });
    match serde_json::to_string_pretty(&document) {
        Ok(json) => Some(format!("{json}\n")),
        Err(error) => {
            eprintln!("error: cannot serialize report: {error}");
            None
        }
    }
}

/// The closing line of a multi-trace human report: how many traces reached
/// each verdict, and how many could not be judged at all.
fn summary(judged: &[Judged], unjudged: usize) -> String {
    let count = |verdict: Verdict| judged.iter().filter(|one| one.verdict() == verdict).count();
    let mut line = format!(
        "{} traces: {} pass, {} pass-with-warnings, {} fail",
        judged.len() + unjudged,
        count(Verdict::Pass),
        count(Verdict::PassWithWarnings),
        count(Verdict::Fail),
    );
    if unjudged > 0 {
        let _ = write!(line, ", {unjudged} not judged (see the errors above)");
    }
    line.push('\n');
    line
}

/// The most severe verdict, in the order a report derives its own: unsupported
/// over fail over warnings over pass.
fn worst(verdicts: impl Iterator<Item = Verdict>) -> Option<Verdict> {
    let rank = |verdict: &Verdict| match verdict {
        Verdict::Pass => 0,
        Verdict::PassWithWarnings => 1,
        Verdict::Fail => 2,
        _ => 3,
    };
    verdicts.max_by_key(rank)
}

/// The exit code a verdict maps to, shared by every path so the 0/1/2 contract
/// has one definition. `--strict` promotes warnings to findings.
///
/// When it does, it says so on stderr. The report's own `verdict:` line is a
/// property of the trace and is deliberately not rewritten by an invocation
/// flag — a golden report must not depend on how the CLI was called — which
/// left a run ending `verdict: pass-with-warnings` and exiting 1, with nothing
/// anywhere connecting the two. The note is the missing sentence, and stderr is
/// where it belongs: stdout carries the report, including the JSON and `JUnit` a
/// machine reads.
fn verdict_to_code(verdict: Verdict, strict: bool) -> u8 {
    if strict && verdict == Verdict::PassWithWarnings {
        eprintln!(
            "note: --strict — SHOULD-level findings are treated as failures, \
             so this run exits {EXIT_FINDINGS} despite a verdict of {verdict}"
        );
    }
    match verdict {
        Verdict::Fail => EXIT_FINDINGS,
        Verdict::PassWithWarnings if strict => EXIT_FINDINGS,
        Verdict::PassWithWarnings | Verdict::Pass => EXIT_OK,
        // Unsupported — and, since Verdict is #[non_exhaustive], any future verdict — is
        // conservatively an invocation-level problem (registry/build mismatch).
        _ => EXIT_USAGE,
    }
}

/// `JUnit` options naming the trace (not stdin, which has no name) and carrying
/// `--strict`, so the report agrees with the exit status.
fn junit_options(trace: &str, strict: bool) -> junit::Options {
    let options = if trace == "-" {
        junit::Options::default()
    } else {
        junit::Options::for_trace(trace)
    };
    options.strict(strict)
}

/// What SARIF results point at: the trace as named, unless it was stdin.
fn artifact<'a>(trace: &'a str, events: &'a [TraceEvent]) -> sarif::Artifact<'a> {
    sarif::Artifact {
        uri: (trace != "-").then_some(trace),
        events,
    }
}
