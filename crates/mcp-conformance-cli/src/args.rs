// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The command line.
//!
//! `test` and `watch`; the workbench's later subcommands (ADR-0021) join them
//! as siblings in [`Command`], sharing the report options in [`ReportArgs`].
//!
//! `--url` is taken as text and checked in `run`, not by clap: clap's error
//! repeats the value, and a server URL can carry a key in its query.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use mcp_trace_capture::run::values::listen_address;

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
    Test(TestArgs),
    /// Record a session exactly as `mcp-trace-capture` does — stdio wrapper or
    /// HTTP proxy, same flags, same trace — and judge it while it runs: each
    /// new finding printed (stderr) as the exchange that shows it completes,
    /// a live tally, the validator's report at the end, and with `--otlp` one
    /// OpenTelemetry span per exchange.
    #[command(after_help = WATCH_EXIT_CODES)]
    Watch(WatchArgs),
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

/// `watch`'s exit codes: the recorder's, which a client launching the stdio
/// wrapper relies on.
pub(crate) const WATCH_EXIT_CODES: &str = "\
Exit status (mcp-trace-capture's; findings do not change it — judge the
trace with `mcp-trace-validator validate` for a verdict's exit code):
  stdio  the server's exit code (128 + signal if a signal ended it, on Unix;
         the whole code on Windows); 1 if waiting for the server failed
  http   0 after a stop; 128 + signal when a second signal cut the stop short;
         2 if serving failed
  2      bad arguments, or a run that could not start (no trace file is created)
  3      a trace is incomplete (a write failed), in place of a 0";

/// What `watch` records, and where its spans go. `-o`, `--force` and
/// `--max-message-bytes` are `mcp-trace-capture`'s, word for word.
#[derive(Debug, Args)]
pub(crate) struct WatchArgs {
    #[command(subcommand)]
    pub(crate) mode: WatchMode,
    /// Where to write the trace (JSON Lines); `{session}` in the file name numbers a
    /// trace per session.
    ///
    /// With `{session}` in the file name, each session gets its own numbered file
    /// (`traces/{session}.jsonl` writes `traces/001.jsonl`, `002`, …), and no
    /// existing file is ever overwritten.
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "mcp-trace.jsonl",
        global = true
    )]
    pub(crate) output: PathBuf,
    /// Overwrite the output file if it exists (not needed with `{session}`).
    #[arg(long, global = true)]
    pub(crate) force: bool,
    /// The largest message recorded, in bytes; larger ones are forwarded unrecorded.
    #[arg(long, value_name = "BYTES", default_value_t = mcp_trace_capture::DEFAULT_MAX_MESSAGE, global = true)]
    pub(crate) max_message_bytes: usize,
    /// Also export one span per exchange, with each finding as a span event, to
    /// an OpenTelemetry collector over OTLP/HTTP (JSON): its base URL, e.g.
    /// `http://localhost:4318` (`/v1/traces` is appended), or a full URL ending
    /// in `/v1/traces`.
    #[arg(long, value_name = "URL", global = true)]
    pub(crate) otlp: Option<String>,
}

/// What `watch` sits in front of: `mcp-trace-capture`'s two modes.
#[derive(Debug, Subcommand)]
pub(crate) enum WatchMode {
    /// Wrap a stdio server: configure your client to launch
    /// `mcp-conformance watch stdio -- <server> [args…]` instead of the server.
    #[command(after_help = WATCH_EXIT_CODES)]
    Stdio {
        /// The server command and its arguments.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    /// Proxy a streamable-HTTP server: point your client at the listen address.
    #[command(after_help = WATCH_EXIT_CODES)]
    Http {
        /// The server's URL. The proxy's `/` is this URL itself; any other request
        /// path is appended to its path.
        #[arg(long, value_name = "URL")]
        upstream: String,
        /// Where the proxy listens: an IP address and a port.
        #[arg(long, value_name = "IP:PORT", default_value = "127.0.0.1:8080", value_parser = listen_address)]
        listen: SocketAddr,
        /// Forward the client's `Host` header unchanged instead of naming the
        /// upstream, so the server's own `Host` validation sees the client's.
        #[arg(long)]
        preserve_host: bool,
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
