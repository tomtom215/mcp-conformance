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

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use mcp_conformance_core::requirement::{Registry, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_trace_validator::reader;

mod emit;
mod input;
mod judgeable;
mod requirements;
mod validate;

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
    #[command(
        after_help = "Exit status: 0 pass (warnings pass unless --strict), 1 a clause \
        failed, 2 bad invocation or nothing judgeable or output not written, 3 malformed trace."
    )]
    Validate {
        /// Paths to the trace documents (one or more), or `-` for stdin alone.
        /// Several traces give one report per format: a section each in human
        /// output, one `JUnit` document, one SARIF run.
        #[arg(required = true, num_args = 1..)]
        traces: Vec<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = Format::Human)]
        format: Format,
        /// Treat SHOULD-level findings (warnings) as failures.
        #[arg(long)]
        strict: bool,
        /// Human output: list every clause with its outcome, and the reason for
        /// each exclusion. By default only failing, warning and unsupported
        /// clauses are listed (the totals always count every clause). JSON,
        /// `JUnit` and SARIF always carry every clause.
        #[arg(short, long)]
        all: bool,
        /// Accepted for compatibility: the findings-only listing it selected is
        /// now the default.
        #[arg(short, long, hide = true, conflicts_with = "all")]
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
    /// SARIF 2.1.0 (validate only), for code scanning: GitHub, GitLab, IDEs.
    Sarif,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Command::Validate {
            traces,
            format,
            strict,
            all,
            quiet: _,
            registry,
            revisions,
            registry_set,
            max_line_bytes,
            max_events,
        } => validate::run(
            &traces,
            &reader::Limits::new(max_events, max_line_bytes),
            validate::Output {
                format,
                strict,
                all,
            },
            &validate::Sources {
                registry: registry.as_deref(),
                registry_set: registry_set.as_deref(),
                revisions: &revisions,
            },
        ),
        Command::Requirements {
            format,
            registry,
            revision,
        } => requirements::run(format, registry.as_deref(), revision.as_deref()),
    };
    ExitCode::from(code)
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
