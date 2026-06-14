// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-trace-validator` — validate recorded MCP protocol traces offline.
//!
//! Exit codes (stable interface, relied on by CI integrations):
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Validation ran; no MUST-level violations (warnings allowed unless `--strict`) |
//! | 1    | MUST-level violations — or SHOULD-level ones under `--strict` |
//! | 2    | Invocation, registry, or check-inventory problem (including `unsupported` outcomes) |
//! | 3    | The trace document itself was malformed |

use std::fs;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mcp_conformance_core::requirement::{Registry, RegistrySet, Verification};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_trace_validator::report::Verdict;
use mcp_trace_validator::{engine, multi, reader};

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
    /// Validate a JSON Lines trace against a requirement registry.
    ///
    /// With one or more `--revision` flags, judgment runs in multi-revision mode against
    /// a registry *set* (the built-in set, or `--registry-set`), emitting a report whose
    /// every clause carries its outcome under each revision.
    Validate {
        /// Path to the trace document, or `-` for stdin.
        trace: String,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
        /// Treat SHOULD-level findings (warnings) as failures.
        #[arg(long)]
        strict: bool,
        /// Path to a single-revision registry JSON document; defaults to the built-in
        /// `2025-11-25` registry. Mutually exclusive with `--revision`.
        #[arg(long)]
        registry: Option<PathBuf>,
        /// A protocol revision (`YYYY-MM-DD`) to judge against; repeatable. Each flag adds
        /// a revision to a single multi-revision run.
        #[arg(long = "revision", value_name = "YYYY-MM-DD")]
        revisions: Vec<String>,
        /// Path to a multi-revision registry *set* JSON document; defaults to the built-in
        /// set. Only meaningful with `--revision`.
        #[arg(long)]
        registry_set: Option<PathBuf>,
    },
    /// Print the requirement registry this build validates against.
    Requirements {
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
        /// Path to a registry JSON document; defaults to the built-in `2025-11-25`
        /// registry.
        #[arg(long)]
        registry: Option<PathBuf>,
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
            registry,
            revisions,
            registry_set,
        } => run_validate_command(
            &trace,
            format,
            strict,
            registry.as_deref(),
            registry_set.as_deref(),
            &revisions,
        ),
        Command::Requirements { format, registry } => run_requirements(format, registry.as_deref()),
    };
    ExitCode::from(code)
}

/// Dispatches the `validate` command to single- or multi-revision judgment by whether any
/// `--revision` was given, rejecting the nonsensical flag combinations up front.
fn run_validate_command(
    trace: &str,
    format: Format,
    strict: bool,
    registry: Option<&std::path::Path>,
    registry_set: Option<&std::path::Path>,
    revisions: &[String],
) -> u8 {
    if revisions.is_empty() {
        if registry_set.is_some() {
            eprintln!("error: --registry-set requires --revision (multi-revision mode)");
            return EXIT_USAGE;
        }
        return run_validate(trace, format, strict, registry);
    }
    if registry.is_some() {
        eprintln!(
            "error: --registry is single-revision; use --registry-set (or the built-in set) \
             with --revision"
        );
        return EXIT_USAGE;
    }
    run_validate_multi(trace, format, strict, registry_set, revisions)
}

fn load_registry(path: Option<&std::path::Path>) -> Result<Registry, String> {
    match path {
        None => Registry::builtin_2025_11_25().map_err(|error| error.to_string()),
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            Registry::from_json(&text).map_err(|error| format!("{}: {error}", path.display()))
        }
    }
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

fn read_trace_document(source: &str) -> Result<String, String> {
    if source == "-" {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .map_err(|error| format!("cannot read stdin: {error}"))?;
        Ok(text)
    } else {
        fs::read_to_string(source).map_err(|error| format!("cannot read {source}: {error}"))
    }
}

fn run_validate(
    trace_source: &str,
    format: Format,
    strict: bool,
    registry_path: Option<&std::path::Path>,
) -> u8 {
    let registry = match load_registry(registry_path) {
        Ok(registry) => registry,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let document = match read_trace_document(trace_source) {
        Ok(document) => document,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let events = match reader::parse_trace(&document, &reader::Limits::default()) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("error: malformed trace: {error}");
            return EXIT_MALFORMED_TRACE;
        }
    };

    let report = engine::validate(&registry, &events);
    match format {
        Format::Human => print!("{}", report.render_human()),
        Format::Json => match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Junit => print!("{}", mcp_trace_validator::junit::render(&report)),
    }

    verdict_to_code(report.verdict(), strict)
}

/// Multi-revision judgment: one trace against several revisions of a registry set, with
/// per-clause applicability differences in the report (roadmap M2.5).
fn run_validate_multi(
    trace_source: &str,
    format: Format,
    strict: bool,
    registry_set_path: Option<&std::path::Path>,
    revisions: &[String],
) -> u8 {
    if matches!(format, Format::Junit) {
        eprintln!(
            "error: --format junit applies to single-revision validate, not multi-revision \
             (use json or human)"
        );
        return EXIT_USAGE;
    }
    let set = match load_registry_set(registry_set_path) {
        Ok(set) => set,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let revisions = match parse_revisions(revisions) {
        Ok(revisions) => revisions,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let document = match read_trace_document(trace_source) {
        Ok(document) => document,
        Err(message) => {
            eprintln!("error: {message}");
            return EXIT_USAGE;
        }
    };
    let events = match reader::parse_trace(&document, &reader::Limits::default()) {
        Ok(events) => events,
        Err(error) => {
            eprintln!("error: malformed trace: {error}");
            return EXIT_MALFORMED_TRACE;
        }
    };
    let report = match multi::validate_revisions(&set, &revisions, &events) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("error: {error}");
            return EXIT_USAGE;
        }
    };
    match format {
        Format::Human => print!("{}", report.render_human()),
        Format::Json => match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("error: cannot serialize report: {error}");
                return EXIT_USAGE;
            }
        },
        // Guarded above; defensive rather than a reachable panic.
        Format::Junit => return EXIT_USAGE,
    }
    verdict_to_code(report.verdict(), strict)
}

fn run_requirements(format: Format, registry_path: Option<&std::path::Path>) -> u8 {
    let registry = match load_registry(registry_path) {
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
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("error: cannot serialize registry: {error}");
                return EXIT_USAGE;
            }
        },
        Format::Human => {
            println!("requirement registry — revision {}", registry.revision());
            for requirement in registry.requirements() {
                let verification = match &requirement.verification {
                    Verification::Checks { checks } => format!("checks: {}", checks.join(", ")),
                    Verification::Excluded { .. } => "excluded".to_owned(),
                    // Foreign #[non_exhaustive] enum: future arms surface visibly.
                    _ => "unrecognized verification".to_owned(),
                };
                println!(
                    "  {} {:<9} ({}) — {}",
                    requirement.id,
                    requirement.level.keyword(),
                    verification,
                    requirement.source.quote
                );
            }
        }
    }
    EXIT_OK
}
