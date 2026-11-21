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

use crate::declared::{self, RevisionSource};
use crate::multi::{self, MultiReport};
use crate::report::{Filter, HumanOptions, Party, Report, Unjudged, Verdict};
use crate::{engine, html, junit, reader, sarif, sessions};
use mcp_conformance_core::requirement::{Registry, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::trace::TraceEvent;

use crate::cli::emit::emit;
use crate::cli::input::Refusal;
use crate::cli::{
    EXIT_FINDINGS, EXIT_OK, EXIT_USAGE, Format, input, judgeable, load_registry, load_registry_set,
    parse_revisions,
};

pub(crate) mod baseline;
mod many;
mod page;

/// The most findings the default human report lists under one clause. A
/// recording that repeats one defect throughout is otherwise hundreds of
/// megabytes of the same line; the rest are counted, and `--all` or
/// `--format json` lists every one.
const MAX_FINDINGS: usize = 20;

/// How `validate` presents its report.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Output<'a> {
    pub(crate) format: Format,
    /// SHOULD-level findings fail the run.
    pub(crate) strict: bool,
    /// Human output lists every clause, not only those needing attention.
    pub(crate) all: bool,
    /// Only this party's findings (and findings held against nobody) count
    /// toward the verdict and the exit status (`--party`).
    pub(crate) party: Option<Party>,
    /// Human output lists only these clauses (`--area`, `--level`); nothing a
    /// report decides depends on it.
    pub(crate) filter: &'a Filter,
    /// Also write the HTML report here (`--html-report`).
    pub(crate) page: Option<&'a Path>,
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
pub(crate) struct Judged {
    trace: String,
    events: Vec<TraceEvent>,
    /// One report per judged revision, in order.
    reports: Vec<Report>,
    /// The cross-revision view, when more than one revision was judged.
    multi: Option<MultiReport>,
}

impl Judged {
    /// The verdict the exit status follows: the report's, as it is viewed.
    fn verdict(&self) -> Verdict {
        self.multi
            .as_ref()
            .map_or_else(|| self.reports[0].effective_verdict(), MultiReport::verdict)
    }

    /// The human report: with `--all`, every clause and every finding;
    /// otherwise the clauses needing attention, each with its first
    /// [`MAX_FINDINGS`] findings.
    fn render_human(&self, output: Output<'_>) -> String {
        let options = HumanOptions::default()
            .all(output.all)
            .max_findings((!output.all).then_some(MAX_FINDINGS))
            .cli(true);
        self.multi.as_ref().map_or_else(
            || self.reports[0].render_with_filter(&options, output.filter),
            |multi| multi.render_with_filter(&options, output.filter),
        )
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
    output: Output<'_>,
    sources: &Sources<'_>,
    settings: &baseline::Settings<'_>,
) -> u8 {
    if traces.len() > 1 && traces.iter().any(|trace| trace == "-") {
        eprintln!("error: `-` (stdin) can only be validated on its own, not with other traces");
        return EXIT_USAGE;
    }
    let previous = match baseline::check(settings, output.format, !output.filter.is_empty())
        .and_then(|()| baseline::load(settings))
    {
        Ok(previous) => previous,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let registries = match resolve(sources) {
        Ok(registries) => registries,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let many = traces.len() > 1;
    let (mut judged, unjudged, code) = judge_all(traces, limits, &registries, output.party);
    if judged.is_empty() {
        return code; // Nothing was judged; each error is printed.
    }
    if let Some(unknown) = unknown_area(&judged, output.filter) {
        eprintln!("error: no judged revision has an area {unknown} (`requirements` lists them)");
        return EXIT_USAGE;
    }
    let run = match baseline::finish(
        &mut judged,
        !unjudged.is_empty(),
        settings,
        previous,
        output.party,
    ) {
        Ok(run) => run,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    if let Some(path) = output.page
        && !page::write(path, &judged, &unjudged, &run)
    {
        return EXIT_USAGE;
    }
    let rendered = match judged.as_slice() {
        [one] if !many => render_one(one, output, &run),
        _ => many::render(&judged, &unjudged, output, &run),
    };
    rendered.map_or(EXIT_USAGE, |rendered| code.max(rendered))
}

/// Judges each trace on its own: those that could be, those that could not
/// (printed as they happen, and kept for the report), and the worst exit code
/// of the refusals.
fn judge_all(
    traces: &[String],
    limits: &reader::Limits,
    registries: &Registries,
    party: Option<Party>,
) -> (Vec<Judged>, Vec<Unjudged>, u8) {
    let many = traces.len() > 1;
    let mut code = EXIT_OK;
    let mut judged = Vec::new();
    let mut unjudged = Vec::new();
    for trace in traces {
        match judge(trace, limits, registries, many, party) {
            Ok(one) => judged.push(one),
            Err(refusal) => {
                refusal.print();
                code = code.max(refusal.code);
                unjudged.push(Unjudged::new(trace.as_str(), refusal.error));
            }
        }
    }
    (judged, unjudged, code)
}

/// The first area `filter` names that no judged report has a clause in: almost
/// always a typo, which would otherwise list nothing and read as a clean trace.
fn unknown_area(judged: &[Judged], filter: &Filter) -> Option<String> {
    filter
        .areas
        .iter()
        .find(|area| {
            !judged.iter().flat_map(|one| &one.reports).any(|report| {
                report
                    .requirements
                    .iter()
                    .any(|row| crate::report::area_of(&row.id) == area.as_str())
            })
        })
        .cloned()
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
    // Each revision once: naming one twice is not asking for two columns of
    // the same judgment.
    let mut requested: Vec<ProtocolRevision> = Vec::new();
    for revision in parse_revisions(sources.revisions)? {
        if !requested.contains(&revision) {
            requested.push(revision);
        }
    }
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

/// Reads and judges one trace, or says why it could not be; the reports are
/// viewed for `party` when one was asked for.
fn judge(
    trace: &str,
    limits: &reader::Limits,
    registries: &Registries,
    many: bool,
    party: Option<Party>,
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
    let (mut reports, mut multi) = match registries {
        Registries::Custom(registry) => (vec![engine::validate(registry, &events)], None),
        Registries::Set { set, requested } => judge_in_set(set, requested, &events, &prefix)?,
    };
    // The view is applied after judging, never during it: the rows and totals
    // are the trace's facts, and only what counts toward the verdict changes.
    if let Some(party) = party {
        for report in &mut reports {
            report.party_view = Some(party);
        }
        if let Some(multi) = &mut multi {
            multi.view_party(party);
        }
    }
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
    let (chosen, source, proposed) = if requested.is_empty() {
        declared::select(set.revisions(), events)
            .map(|selection| (selection.revisions, selection.source, selection.proposed))
            .map_err(|error| Refusal {
                error: format!("{prefix}{error}"),
                hints: vec![
                    "pass --revision YYYY-MM-DD to judge it against a supported revision anyway"
                        .to_owned(),
                ],
                code: EXIT_USAGE,
            })?
    } else {
        (requested.to_vec(), RevisionSource::Requested, None)
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
    // The session ran under what the server negotiated, not what the client
    // asked for; the reader deserves to know the report is not of the latter.
    if let Some(proposed) = proposed {
        eprintln!(
            "note: {prefix}the client proposed protocol revision {proposed}; the server's \
             initialize result negotiated {}, which the session then ran under, so it is \
             judged there (pass --revision {proposed} to judge it under the proposal instead)",
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
    // Each revision's report records the choice too: the HTML page shows the
    // reports themselves, and the multi-revision JSON is unchanged by it.
    for report in &mut reports {
        report.revision_source = Some(source);
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
fn render_one(one: &Judged, output: Output<'_>, run: &baseline::Run) -> Option<u8> {
    let written = match output.format {
        Format::Human => emit(&format!("{}{}", one.render_human(output), run.human())),
        Format::Json => match one.to_json().and_then(|mut json| {
            run.annotate(&mut json);
            serde_json::to_string_pretty(&json)
        }) {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return None;
            }
        },
        Format::Junit => emit(&junit::render_run_with_baseline(
            &[(junit_options(&one.trace, output.strict), &one.reports[..])],
            &[],
            &run.notes,
        )),
        Format::Sarif => emit(&sarif::render_run_with_baseline(
            &[(artifact(&one.trace, &one.events), &one.reports[..])],
            &[],
            &sarif::Options::default().strict(output.strict),
            &run.notes,
        )),
        Format::Html => emit(&html::render_run_with_baseline(
            &[(page_trace(&one.trace, &one.events), &one.reports[..])],
            &[],
            &run.notes,
        )),
    };
    written.then(|| {
        if promoted(one.verdict(), output.strict) {
            note_strict("");
        }
        run.note_failure();
        run_code(verdict_to_code(one.verdict(), output.strict), run)
    })
}

/// A verdict's exit code, raised to `1` when the baseline alone fails the run.
const fn run_code(code: u8, run: &baseline::Run) -> u8 {
    if run.fails() && code == EXIT_OK {
        EXIT_FINDINGS
    } else {
        code
    }
}

/// Whether `--strict` is what fails a trace of this verdict.
fn promoted(verdict: Verdict, strict: bool) -> bool {
    strict && verdict == Verdict::PassWithWarnings
}

/// Says that `--strict` failed a run its report calls passing; `whose` names
/// the traces when there are several (` for a.jsonl, b.jsonl`), so a batch
/// gets one note rather than one per trace with nothing to tell them apart.
///
/// The report's own `verdict:` line is a property of the trace and is
/// deliberately not rewritten by an invocation flag — a golden report must not
/// depend on how the CLI was called — which left a run ending `verdict:
/// pass-with-warnings` and exiting 1, with nothing anywhere connecting the
/// two. The note is the missing sentence, and stderr is where it belongs:
/// stdout carries the report, including the JSON and `JUnit` a machine reads.
fn note_strict(whose: &str) {
    eprintln!(
        "note: --strict — SHOULD-level findings are treated as failures, so this run \
         exits {EXIT_FINDINGS} despite a verdict of {}{whose}",
        Verdict::PassWithWarnings
    );
}

/// The exit code a verdict maps to, shared by every path so the 0/1/2 contract
/// has one definition. `--strict` promotes warnings to findings.
const fn verdict_to_code(verdict: Verdict, strict: bool) -> u8 {
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

/// What the HTML page names the trace: as given, unless it was stdin.
fn page_trace<'a>(trace: &'a str, events: &'a [TraceEvent]) -> html::Trace<'a> {
    html::Trace {
        name: (trace != "-").then_some(trace),
        events,
    }
}
