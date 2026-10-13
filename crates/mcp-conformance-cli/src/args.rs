// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The command line.
//!
//! One subcommand today, `test`; the workbench's later ones (ADR-0021) join it
//! as siblings in [`Command`], sharing the report options in [`ReportArgs`].
//!
//! `--url` is taken as text and checked in `run`, not by clap: clap's error
//! repeats the value, and a server URL can carry a key in its query.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

/// The exit codes, for `--help`; the README's table says the same.
pub(crate) const EXIT_CODES: &str = "\
Exit status:
  0  the suite passed, and the validator agrees: no failing clause the
     divergence baseline does not explain, and no stale baseline entry
  1  findings: a suite check failed or warned, or the two disagree
  2  the run could not be made or judged: bad arguments, Node.js or npx
     missing, the suite did not run, no session judgeable, the report not
     written";

/// Run the official MCP conformance suite through a recording tap and judge
/// every session it drove, clause by clause.
#[derive(Debug, Parser)]
#[command(name = "mcp-conformance", version, about, long_about = None, after_help = EXIT_CODES)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Run the pinned official suite against a server or a client, recording
    /// its traffic, and report the suite's checks, the clause report of the
    /// recorded sessions, and whether the two agree.
    ///
    /// Needs Node.js 18 or newer with `npx` on PATH: the suite is a Node
    /// package, fetched by npx at exactly the version this build pins.
    #[command(after_help = EXIT_CODES)]
    Test(Box<TestArgs>),
    /// Carries a client under test to the suite's server through the recorder;
    /// `test --client-command` runs it as the client command it gives the
    /// suite. Not for direct use.
    #[command(name = "__client-relay", hide = true)]
    ClientRelay {
        /// The client's command and arguments, then the suite's server URL.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
}

/// What `test` runs, and how it reports.
#[derive(Debug, Args)]
pub(crate) struct TestArgs {
    #[command(flatten)]
    pub(crate) subject: Subject,
    /// A scenario to run instead of the default set; repeatable. Server: the
    /// suite's active scenarios by default. Client: its four protocol
    /// scenarios (`initialize`, `tools_call`,
    /// `elicitation-sep1034-client-defaults`, `sse-retry`).
    #[arg(long = "scenario", value_name = "NAME")]
    pub(crate) scenarios: Vec<String>,
    /// The protocol revision the suite selects scenarios for.
    #[arg(long, value_name = "YYYY-MM-DD", default_value = mcp_conformance_cli::suite::SPEC_VERSION)]
    pub(crate) spec_version: String,
    /// The suite's own baseline of scenarios expected to fail (its YAML
    /// format: `server:` and `client:` lists), passed to it unchanged.
    #[arg(long, value_name = "YAML")]
    pub(crate) expected_failures: Option<PathBuf>,
    /// Explained divergences between the suite and the validator (the
    /// `agreement-divergences.json` format). Without one, every failing
    /// clause is an unexplained divergence.
    #[arg(long, value_name = "JSON")]
    pub(crate) divergences: Option<PathBuf>,
    /// Where the suite's results and the recorded traces are kept: a directory
    /// that does not exist yet, or is empty. By default, a new one under the
    /// system's temporary directory; either way its path is printed.
    #[arg(long, value_name = "DIR")]
    pub(crate) work_dir: Option<PathBuf>,
    #[command(flatten)]
    pub(crate) report: ReportArgs,
}

/// The subject under test: exactly one of three.
#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub(crate) struct Subject {
    /// A streamable-HTTP server's endpoint URL, e.g. `http://localhost:3000/mcp`.
    /// The suite reaches it through a recording proxy.
    #[arg(long, value_name = "URL")]
    pub(crate) url: Option<String>,
    /// A stdio server's command, split at whitespace and run without a shell,
    /// e.g. `"python my_server.py"`. One server process per suite session,
    /// each under the recorder's stdio wrapper; the suite reaches them through
    /// a loopback bridge, and its HTTP-transport scenarios are not run.
    #[arg(long, value_name = "COMMAND")]
    pub(crate) command: Option<String>,
    /// A client's command, e.g. `"node my_client.js"`. The suite runs it
    /// through a shell, once per scenario, with its server's URL appended;
    /// the client reaches that server through a recording proxy.
    #[arg(long, value_name = "COMMAND")]
    pub(crate) client_command: Option<String>,
}

/// How a report is written: shared by every subcommand that writes one.
#[derive(Debug, Clone, Args)]
pub(crate) struct ReportArgs {
    /// Output format. `junit` and `sarif` carry the clause report; the suite's
    /// checks and the agreement are in `human` and `json`, and on stderr.
    #[arg(long, value_enum, default_value_t = Format::Human)]
    pub(crate) format: Format,
    /// Write the report to this file instead of stdout.
    #[arg(short, long, value_name = "FILE")]
    pub(crate) output: Option<PathBuf>,
    /// Human output: every clause of every session, not only those needing
    /// attention.
    #[arg(short, long)]
    pub(crate) all: bool,
}

/// Report formats: the validator's four.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Terminal-oriented text.
    Human,
    /// Pretty-printed JSON.
    Json,
    /// `JUnit` XML: the clause report, for CI test-report ingestion.
    Junit,
    /// SARIF 2.1.0: the clause report, for code scanning.
    Sarif,
}
