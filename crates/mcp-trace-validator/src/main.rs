// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-trace-validator` — validate recorded MCP protocol traces offline.
//!
//! Exit codes (stable interface, relied on by CI integrations):
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Validation ran; no MUST-level violations (warnings allowed unless `--strict`) |
//! | 1    | MUST-level violations — or SHOULD-level ones under `--strict`, which says so on stderr |
//! | 2    | Invocation, registry, or check-inventory problem (including `unsupported` outcomes) |
//! | 3    | The trace document itself was malformed |

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mcp_conformance_core::requirement::{Registry, RegistrySet, Verification};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_trace_validator::declared::{self, RevisionSource};
use mcp_trace_validator::report::{Report, Verdict};
use mcp_trace_validator::{engine, multi, reader};

mod input;
mod judgeable;

const EXIT_OK: u8 = 0;
const EXIT_FINDINGS: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_MALFORMED_TRACE: u8 = 3;

/// Offline conformance validation for recorded Model Context Protocol traces.
#[derive(Debug, Parser)]
#[command(name = "mcp-trace-validator", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a JSON Lines trace against the specification.
    ///
    /// The protocol revision is the one the trace declares (in `initialize`, per-request
    /// `_meta`, or the `MCP-Protocol-Version` header); a trace declaring none is judged
    /// against the newest supported revision. `--revision` overrides the choice; naming
    /// several judges the trace under each, clause by clause.
    Validate {
        /// Path to the trace document, or `-` for stdin.
        trace: String,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
        /// Treat SHOULD-level findings (warnings) as failures.
        #[arg(long)]
        strict: bool,
        /// Human output: print only failing, warning and unsupported clauses (the
        /// totals still count every clause). JSON and `JUnit` are unaffected.
        #[arg(short, long)]
        quiet: bool,
        /// Path to a custom single-revision registry JSON document, used instead of the
        /// built-in registries. Mutually exclusive with `--revision` and `--registry-set`.
        #[arg(long)]
        registry: Option<PathBuf>,
        /// A protocol revision (`YYYY-MM-DD`) to judge against instead of the one the
        /// trace declares; repeatable, to judge under several at once.
        #[arg(long = "revision", value_name = "YYYY-MM-DD")]
        revisions: Vec<String>,
        /// Path to a custom multi-revision registry *set* JSON document, used instead of
        /// the built-in one.
        #[arg(long)]
        registry_set: Option<PathBuf>,
        /// The longest trace line accepted, in bytes. The default reads back
        /// anything `mcp-trace-capture` writes at its default message limit.
        #[arg(long, value_name = "BYTES", default_value_t = reader::Limits::default().max_line_bytes)]
        max_line_bytes: usize,
        /// The most events accepted in one trace.
        #[arg(long, value_name = "N", default_value_t = reader::Limits::default().max_events)]
        max_events: usize,
    },
    /// Print the requirement registry this build validates against.
    Requirements {
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
        /// Path to a registry JSON document, used instead of the built-in one.
        /// Mutually exclusive with `--revision`.
        #[arg(long)]
        registry: Option<PathBuf>,
        /// Which built-in revision to print; defaults to the newest.
        #[arg(long, value_name = "YYYY-MM-DD", conflicts_with = "registry")]
        revision: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    /// Terminal-oriented text.
    Human,
    /// Pretty-printed JSON.
    Json,
    /// `JUnit` XML (validate only), for CI test-report ingestion.
    Junit,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Validate {
            trace,
            format,
            strict,
            quiet,
            registry,
            revisions,
            registry_set,
            max_line_bytes,
            max_events,
        } => run_validate_command(
            &trace,
            &reader::Limits::new(max_events, max_line_bytes),
            Output {
                format,
                strict,
                quiet,
            },
            registry.as_deref(),
            registry_set.as_deref(),
            &revisions,
        ),
        Command::Requirements {
            format,
            registry,
            revision,
        } => run_requirements(format, registry.as_deref(), revision.as_deref()),
    };
    ExitCode::from(code)
}

/// How `validate` presents its report.
#[derive(Debug, Clone, Copy)]
struct Output {
    format: Format,
    /// SHOULD-level findings fail the run.
    strict: bool,
    /// Human output lists only the clauses that need attention.
    quiet: bool,
}

/// Runs `validate`: reads the trace, chooses the revisions to judge it against, and
/// dispatches to single- or multi-revision judgment.
fn run_validate_command(
    trace: &str,
    limits: &reader::Limits,
    output: Output,
    registry: Option<&std::path::Path>,
    registry_set: Option<&std::path::Path>,
    revisions: &[String],
) -> u8 {
    if registry.is_some() && (registry_set.is_some() || !revisions.is_empty()) {
        eprintln!(
            "error: --registry names one custom registry; it cannot be combined with \
             --revision or --registry-set"
        );
        return EXIT_USAGE;
    }
    let events = match input::read_events(trace, limits) {
        Ok(events) => events,
        Err(code) => return code,
    };
    if let Some(path) = registry {
        return match load_registry(path) {
            Ok(registry) => emit_single(&engine::validate(&registry, &events), trace, output),
            Err(message) => {
                eprintln!("error: {message}");
                EXIT_USAGE
            }
        };
    }
    let set = match load_registry_set(registry_set) {
        Ok(set) => set,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let (chosen, source) = match choose_revisions(&set, revisions, &events) {
        Ok(choice) => choice,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    if source == RevisionSource::Default {
        eprintln!(
            "note: the trace declares no protocol revision; judging it against {}, the \
             newest supported (use --revision to choose)",
            chosen[0]
        );
    }
    if let [revision] = chosen.as_slice() {
        let Some(registry) = set.registry(*revision) else {
            eprintln!("error: registry set does not describe revision {revision}");
            return EXIT_USAGE;
        };
        let mut report = engine::validate(&registry, &events);
        report.revision_source = Some(source);
        return emit_single(&report, trace, output);
    }
    run_validate_multi(&events, trace, output, &set, &chosen, source)
}

/// The revisions to judge against: the `--revision` flags when given, otherwise the
/// trace's own declaration ([`declared::select`]).
fn choose_revisions(
    set: &RegistrySet,
    revisions: &[String],
    events: &[mcp_conformance_core::trace::TraceEvent],
) -> Result<(Vec<ProtocolRevision>, RevisionSource), String> {
    if revisions.is_empty() {
        return declared::select(set.revisions(), events)
            .map(|selection| (selection.revisions, selection.source))
            .map_err(|error| {
                format!(
                    "{error}\nhint: pass --revision YYYY-MM-DD to judge it against a \
                     supported revision anyway"
                )
            });
    }
    let parsed = parse_revisions(revisions)?;
    if let Some(unknown) = parsed
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
    Ok((parsed, RevisionSource::Requested))
}

/// Loads a custom single-revision registry document.
fn load_registry(path: &std::path::Path) -> Result<Registry, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Registry::from_json(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn load_registry_set(path: Option<&std::path::Path>) -> Result<RegistrySet, String> {
    match path {
        None => RegistrySet::builtin().map_err(|error| error.to_string()),
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            RegistrySet::from_json(&text).map_err(|error| format!("{}: {error}", path.display()))
        }
    }
}

/// Parses each `--revision` argument, naming the offending one on failure.
fn parse_revisions(revisions: &[String]) -> Result<Vec<ProtocolRevision>, String> {
    revisions
        .iter()
        .map(|revision| {
            revision
                .parse::<ProtocolRevision>()
                .map_err(|error| error.to_string())
        })
        .collect()
}

/// The exit code a verdict maps to, shared by single- and multi-revision runs so the
/// 0/1/2 contract has one definition. `--strict` promotes warnings to findings.
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
            "note: --strict — the SHOULD-level findings above are treated as failures, \
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

/// Renders a single-revision report and maps its verdict to an exit code.
fn emit_single(report: &Report, trace_source: &str, output: Output) -> u8 {
    if judgeable::reject(report.totals, trace_source) {
        return EXIT_USAGE;
    }
    match output.format {
        Format::Human if output.quiet => emit(&report.render_findings()),
        Format::Human => emit(&report.render_human()),
        Format::Json => match serde_json::to_string_pretty(report) {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Junit => emit(&mcp_trace_validator::junit::render(report)),
    }
    verdict_to_code(report.verdict(), output.strict)
}

/// Multi-revision judgment: one trace against several revisions of a registry set, with
/// per-clause applicability differences in the report. `JUnit` renders one suite per
/// revision.
fn run_validate_multi(
    events: &[mcp_conformance_core::trace::TraceEvent],
    trace_source: &str,
    output: Output,
    set: &RegistrySet,
    revisions: &[ProtocolRevision],
    source: RevisionSource,
) -> u8 {
    let mut report = match multi::validate_revisions(set, revisions, events) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("error: {error}");
            return EXIT_USAGE;
        }
    };
    report.revision_source = Some(source);
    if judgeable::reject(judgeable::combined(&report), trace_source) {
        return EXIT_USAGE;
    }
    match output.format {
        Format::Human if output.quiet => emit(&report.render_findings()),
        Format::Human => emit(&report.render_human()),
        Format::Json => match serde_json::to_string_pretty(&report) {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Junit => {
            let reports: Vec<Report> = revisions
                .iter()
                .filter_map(|revision| set.registry(*revision))
                .map(|registry| engine::validate(&registry, events))
                .collect();
            emit(&mcp_trace_validator::junit::render_all(&reports));
        }
    }
    verdict_to_code(report.verdict(), output.strict)
}

fn run_requirements(
    format: Format,
    registry_path: Option<&std::path::Path>,
    revision: Option<&str>,
) -> u8 {
    let registry = match requirements_registry(registry_path, revision) {
        Ok(registry) => registry,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    match format {
        Format::Junit => {
            eprintln!("error: --format junit applies to validate, not requirements");
            return EXIT_USAGE;
        }
        Format::Json => match serde_json::to_string_pretty(&registry) {
            Ok(json) => emit(&format!("{json}\n")),
            Err(error) => {
                eprintln!("error: cannot serialize registry: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Human => {
            let mut out = format!("requirement registry — revision {}\n", registry.revision());
            for requirement in registry.requirements() {
                let verification = match &requirement.verification {
                    Verification::Checks { checks } => format!("checks: {}", checks.join(", ")),
                    Verification::Excluded { .. } => "excluded".to_owned(),
                    // Foreign #[non_exhaustive] enum: future arms surface visibly.
                    _ => "unrecognized verification".to_owned(),
                };
                let _ = writeln!(
                    out,
                    "  {} {:<9} ({}) — {}",
                    requirement.id,
                    requirement.level.keyword(),
                    verification,
                    requirement.source.quote
                );
            }
            emit(&out);
        }
    }
    EXIT_OK
}

/// The registry `requirements` prints: a custom file, or a built-in revision (the
/// newest by default).
fn requirements_registry(
    registry_path: Option<&std::path::Path>,
    revision: Option<&str>,
) -> Result<Registry, String> {
    if let Some(path) = registry_path {
        return load_registry(path);
    }
    let set = load_registry_set(None)?;
    let revision = match revision {
        Some(text) => text
            .parse::<ProtocolRevision>()
            .map_err(|error| error.to_string())?,
        None => set
            .latest()
            .ok_or_else(|| "the built-in registry set describes no revision".to_owned())?,
    };
    set.registry(revision).ok_or_else(|| {
        format!(
            "no built-in registry for revision {revision} (supported: {})",
            set.revisions()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// Writes `text` to stdout. A reader that closed the pipe early (`… | head`) has
/// what it wanted; that ends the output, it is not an error — `print!` would panic.
fn emit(text: &str) {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    if let Err(error) = stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        eprintln!("error: cannot write output: {error}");
    }
}
