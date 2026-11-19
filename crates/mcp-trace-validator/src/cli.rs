// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The command line, as a library (feature `cli`): the argument definitions and
//! the code behind `validate` and `requirements`.
//!
//! `mcp-trace-validator` is this module and a `main`. It is public so that
//! `mcp-conformance` can offer the same two subcommands, and end `check` with
//! the same report, from the same definitions: one set of flags, one renderer,
//! one table of exit codes, which therefore cannot drift between the binaries.
//!
//! **Not a stable library interface.** The flags and exit codes are (see
//! `STABILITY.md`); the Rust items here exist for this workspace's binaries
//! and may change in any release. A program that wants verdicts should call
//! [`crate::engine`] and render a [`crate::report::Report`].
//!
//! Exit codes:
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0    | Validation ran; no MUST-level violations (warnings allowed unless `--strict`) |
//! | 1    | MUST-level violations — or SHOULD-level ones under `--strict`, which says so on stderr |
//! | 2    | Invocation, registry, or check-inventory problem (including `unsupported` outcomes), a trace that judges nothing, or output not written |
//! | 3    | The trace document itself was malformed |

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in private modules; this follows the rustc lint
// and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::path::PathBuf;

use clap::ValueEnum;
use mcp_conformance_core::requirement::{BUILTIN_REVISIONS, Registry, RegistrySet};
use mcp_conformance_core::revision::ProtocolRevision;

use crate::reader;
use crate::report::Party;

mod emit;
mod input;
mod judgeable;
mod requirements;
mod selection;
mod validate;

pub use requirements::Args as RequirementsArgs;

pub(crate) const EXIT_OK: u8 = 0;
pub(crate) const EXIT_FINDINGS: u8 = 1;
pub(crate) const EXIT_USAGE: u8 = 2;
pub(crate) const EXIT_MALFORMED_TRACE: u8 = 3;

/// `validate`'s arguments: the traces, how the report is presented, and what
/// they are judged against.
#[derive(Debug, clap::Args)]
pub struct ValidateArgs {
    /// Paths to the trace documents (one or more), or `-` for stdin alone.
    /// Several traces give one report per format: a section each in human
    /// output, one `JUnit` document, one SARIF run.
    #[arg(required = true, num_args = 1..)]
    traces: Vec<String>,
    #[command(flatten)]
    report: ReportArgs,
    #[command(flatten)]
    judging: JudgingArgs,
}

/// How a report is presented and what decides its verdict: the flags
/// `validate` and `mcp-conformance check` share.
#[derive(Debug, Clone, clap::Args)]
// Each bool is one independent command-line flag; there is no state machine
// between them for an enum to name.
#[allow(clippy::struct_excessive_bools)]
pub struct ReportArgs {
    /// Output format.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    format: Format,
    /// Treat SHOULD-level findings (warnings) as failures.
    #[arg(long)]
    strict: bool,
    /// Count only this party's findings — and findings held against
    /// nobody, which are labelled — toward the verdict and the exit
    /// status. The report still lists every finding with the party it is
    /// held against, and its totals still count them all; the verdict
    /// line says the view it was reached under. For judging one side of
    /// a session the other side drove, such as a server the official
    /// suite's client tested.
    #[arg(long, value_enum, value_name = "PARTY")]
    party: Option<PartyArg>,
    /// Accept the findings a baseline file lists: they stay in the report,
    /// marked and counted apart, and only the findings it does not list
    /// decide the verdict and the exit status. An entry that matches no
    /// finding is stale and fails the run (`--allow-stale-baseline`), and
    /// one whose `expires` day has passed accepts nothing. Write one with
    /// `--write-baseline`.
    #[arg(long, value_name = "FILE")]
    baseline: Option<PathBuf>,
    /// Write the findings of this run, those that count toward its verdict,
    /// as a baseline to FILE, and report as if `--baseline FILE` had been
    /// given. Each entry's reason says it was recorded and not reviewed.
    /// FILE may exist only if it is also the `--baseline` in use, in which
    /// case the entries that still match keep their reasons and expiry.
    #[arg(long, value_name = "FILE")]
    write_baseline: Option<PathBuf>,
    /// List the baseline's stale entries without failing the run for them.
    #[arg(long, requires = "baseline")]
    allow_stale_baseline: bool,
    #[command(flatten)]
    selection: selection::Selection,
    /// Human output: list every clause with its outcome, every finding,
    /// and the reason for each exclusion. By default only failing, warning
    /// and unsupported clauses are listed, each with its first 20 findings
    /// (the totals always count every clause). JSON, `JUnit`, SARIF and
    /// HTML always carry every clause and finding.
    #[arg(short, long)]
    all: bool,
    /// Accepted for compatibility: the findings-only listing it selected is
    /// now the default.
    #[arg(short, long, hide = true, conflicts_with = "all")]
    quiet: bool,
}

impl ReportArgs {
    /// These arguments with the report viewed for `party` unless `--party`
    /// named one: how a command whose subject is one end of the session
    /// (`check`'s server) holds the verdict to that end by default.
    #[must_use]
    pub fn viewed_for(mut self, party: Party) -> Self {
        self.party.get_or_insert(match party {
            Party::Client => PartyArg::Client,
            Party::Server => PartyArg::Server,
        });
        self
    }

    /// Whether the report is the terminal-oriented text, which a command may
    /// precede with lines of its own; every other format is a document.
    #[must_use]
    pub const fn is_human(&self) -> bool {
        matches!(self.format, Format::Human)
    }
}

/// What traces are judged against, and the limits they are read under.
#[derive(Debug, Clone, clap::Args)]
pub struct JudgingArgs {
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
    #[arg(
        long,
        value_name = "BYTES",
        default_value_t = reader::Limits::default().max_line_bytes,
        value_parser = at_least_one
    )]
    max_line_bytes: usize,
    /// The most events accepted in one trace.
    #[arg(
        long,
        value_name = "N",
        default_value_t = reader::Limits::default().max_events,
        value_parser = at_least_one
    )]
    max_events: usize,
}

impl Default for JudgingArgs {
    /// The built-in registries, the revision each trace declares, and the
    /// default limits: `validate` with none of these flags given.
    fn default() -> Self {
        let limits = reader::Limits::default();
        Self {
            registry: None,
            revisions: Vec::new(),
            registry_set: None,
            max_line_bytes: limits.max_line_bytes,
            max_events: limits.max_events,
        }
    }
}

/// The party `validate --party` views a report for.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum PartyArg {
    /// The client's findings, and those held against nobody.
    Client,
    /// The server's findings, and those held against nobody.
    Server,
}

impl PartyArg {
    const fn party(self) -> Party {
        match self {
            Self::Client => Party::Client,
            Self::Server => Party::Server,
        }
    }
}

/// `validate` output formats.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum Format {
    /// Terminal-oriented text.
    Human,
    /// Pretty-printed JSON.
    Json,
    /// `JUnit` XML, for CI test-report ingestion.
    Junit,
    /// SARIF 2.1.0, for code scanning: GitHub, GitLab, IDEs.
    Sarif,
    /// One self-contained HTML page, to read in a browser or attach to a bug
    /// report: the verdict, each finding at its event in the message timeline
    /// beside the clause it breaks, and the accounting of every other clause.
    /// It loads nothing — no script, font or image.
    Html,
}

/// Runs `validate`: judges the traces, writes the report to stdout and
/// diagnostics to stderr, and returns the exit status.
#[must_use]
pub fn validate(args: &ValidateArgs) -> u8 {
    judge(&args.traces, &args.report, &args.judging)
}

/// [`validate`] for `traces` named by the caller instead of the command line:
/// how `mcp-conformance check` reports on the session it has just recorded.
#[must_use]
pub fn judge(traces: &[String], report: &ReportArgs, judging: &JudgingArgs) -> u8 {
    let filter = crate::report::Filter::new(
        report.selection.areas(),
        report.selection.level_keywords(),
    );
    validate::run(
        traces,
        &reader::Limits::new(judging.max_events, judging.max_line_bytes),
        validate::Output {
            format: report.format,
            strict: report.strict,
            all: report.all,
            party: report.party.map(PartyArg::party),
            filter: &filter,
        },
        &validate::Sources {
            registry: judging.registry.as_deref(),
            registry_set: judging.registry_set.as_deref(),
            revisions: &judging.revisions,
        },
        &validate::baseline::Settings {
            read: report.baseline.as_deref(),
            write: report.write_baseline.as_deref(),
            allow_stale: report.allow_stale_baseline,
        },
    )
}

/// Runs `requirements`: prints the registry, part of it, or single
/// requirements in full, and returns the exit status.
#[must_use]
pub fn requirements(args: &RequirementsArgs) -> u8 {
    requirements::run(args)
}

/// Writes `shell`'s completion script for `command`, installed as `name`.
///
/// It goes to stdout, generated from the definition that parses the command
/// line, so it cannot drift from the flags. Returns the exit status.
#[must_use]
pub fn completions(shell: clap_complete::Shell, command: &mut clap::Command, name: &str) -> u8 {
    let mut script = Vec::new();
    clap_complete::generate(shell, command, name, &mut script);
    if emit::emit(&String::from_utf8_lossy(&script)) {
        EXIT_OK
    } else {
        EXIT_USAGE
    }
}

/// `--version`'s text for a binary at `version`: `-V`'s line, unchanged for
/// the scripts that read it, then the protocol revisions this build judges.
///
/// Built at run time from the registry set the build ships, so it cannot
/// drift from it; clap takes a `'static` string, and this one is made once
/// per process.
#[must_use]
pub fn long_version(version: &str) -> &'static str {
    let revisions: Vec<String> = BUILTIN_REVISIONS.iter().map(ToString::to_string).collect();
    let text = format!(
        "{version}\njudges protocol revisions: {}",
        revisions.join(", ")
    );
    Box::leak(text.into_boxed_str())
}

/// A limit flag's value: a count of at least one. A limit of 0 accepts no
/// trace at all, so every run would fail on the flag rather than the trace.
fn at_least_one(text: &str) -> Result<usize, String> {
    match text.parse::<usize>() {
        Ok(0) => Err("must be at least 1: a limit of 0 accepts no trace at all".to_owned()),
        Ok(value) => Ok(value),
        Err(error) => Err(error.to_string()),
    }
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
