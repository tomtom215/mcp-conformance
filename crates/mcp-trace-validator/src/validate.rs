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

use std::path::Path;

use mcp_conformance_core::requirement::{Registry, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;
use mcp_trace_validator::declared::{self, RevisionSource};
use mcp_trace_validator::multi::{self, MultiReport};
use mcp_trace_validator::report::{Report, Unjudged, Verdict};
use mcp_trace_validator::{engine, junit, reader, sarif, sessions};

use crate::emit::emit;
use crate::input::Refusal;
use crate::{
    EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, Format, input, judgeable, load_registry, load_registry_set,
    parse_revisions,
};

mod many;

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

    fn render_human(&self, output: Output) -> String {
        match (&self.multi, output.all) {
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
    let mut unjudged = Vec::new();
    for trace in traces {
        match judge(trace, limits, &registries, many) {
            Ok(one) => judged.push(one),
            Err(refusal) => {
                refusal.print();
                code = code.max(refusal.code);
                unjudged.push(Unjudged::new(trace.as_str(), refusal.error));
            }
        }
    }
    let rendered = if many {
        many::render(&judged, &unjudged, output)
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

/// Reads and judges one trace, or says why it could not be.
fn judge(
    trace: &str,
    limits: &reader::Limits,
    registries: &Registries,
    many: bool,
) -> Result<Judged, Refusal> {
    let prefix = if many {
        format!("{trace}: ")
    } else {
        String::new()
    };
    let events = input::read_events(trace, limits, &prefix)?;
    if let Some(message) = judgeable::never_answered(&events, trace) {
        return Err(Refusal::from_diagnostic(&message, EXIT_USAGE));
    }
    judgeable::note_sessions(&prefix, sessions::recorded_sessions(&events));
    let (reports, multi) = match registries {
        Registries::Custom(registry) => (vec![engine::validate(registry, &events)], None),
        Registries::Set { set, requested } => judge_in_set(set, requested, &events, &prefix)?,
    };
    let totals = multi
        .as_ref()
        .map_or(reports[0].totals, judgeable::combined);
    if let Some(message) = judgeable::refusal(totals, trace) {
        return Err(Refusal::from_diagnostic(&message, EXIT_USAGE));
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
) -> Result<(Vec<Report>, Option<MultiReport>), Refusal> {
    let (chosen, source) = if requested.is_empty() {
        declared::select(set.revisions(), events)
            .map(|selection| (selection.revisions, selection.source))
            .map_err(|error| Refusal {
                error: format!("{prefix}{error}"),
                hints: vec![
                    "pass --revision YYYY-MM-DD to judge it against a supported revision anyway"
                        .to_owned(),
                ],
                code: EXIT_USAGE,
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
    let mut multi = multi::validate_revisions(set, &chosen, events).map_err(|error| Refusal {
        error: format!("{prefix}{error}"),
        hints: Vec::new(),
        code: EXIT_USAGE,
    })?;
    multi.revision_source = Some(source);
    Ok((reports, Some(multi)))
}

/// One trace's report, exactly as `validate` has always rendered it; `None`
/// when it could not be written.
fn render_one(one: &Judged, output: Output) -> Option<u8> {
    let written = match output.format {
        Format::Human => emit(&one.render_human(output)),
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
