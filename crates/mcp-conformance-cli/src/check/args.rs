// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check`'s command line.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, ValueEnum};

/// `check`'s exit codes: the validator's, and one for a run that never began.
pub(crate) const EXIT_CODES: &str = "\
Exit status (mcp-trace-validator validate's, for the recorded session):
  0    judged, and no MUST-level finding is held against the server
       (warnings pass unless --strict)
  1    a clause failed
  2    the run could not be made or judged: bad arguments, the server could
       not be started or reached, no session was established, the server
       negotiated a revision this build does not judge, output not written
  3    the trace was not written whole
  130  interrupted";

/// What `check` exercises, and within which bounds.
#[derive(Debug, Args)]
pub(crate) struct CheckArgs {
    /// A Streamable HTTP server's endpoint URL, e.g. `http://localhost:3000/mcp`.
    #[arg(help_heading = "The server", long, value_name = "URL", conflicts_with_all = ["command", "server"])]
    pub(crate) url: Option<String>,
    /// A stdio server's command as one string, split at whitespace and run
    /// without a shell (as `test --command` takes it). The form after `--`
    /// needs no quoting and keeps arguments that contain spaces.
    #[arg(
        help_heading = "The server",
        long,
        value_name = "COMMAND",
        conflicts_with = "server"
    )]
    pub(crate) command: Option<String>,
    /// A stdio server's command and its arguments, after `--`.
    #[arg(
        last = true,
        value_name = "SERVER",
        required_unless_present_any = ["url", "command"]
    )]
    pub(crate) server: Vec<OsString>,
    /// A header to send with every HTTP request, as `Name: value`; repeatable.
    /// For a server that wants `Authorization`. The recorder keeps the names of
    /// the headers it knows and never an `Authorization` value.
    #[arg(help_heading = "The server", long, value_name = "HEADER")]
    pub(crate) header: Vec<String>,
    /// The protocol revision to speak. `auto` does what a client supporting
    /// both does: asks `server/discover` (2026-07-28) first and falls back to
    /// the `initialize` handshake (2025-11-25).
    #[arg(help_heading = "The server", long, value_enum, default_value_t = SpecVersion::Auto, value_name = "REVISION")]
    pub(crate) spec_version: SpecVersion,
    /// Call this tool whatever its annotations say; repeatable. The side
    /// effects are then yours.
    #[arg(help_heading = "What is called", long, value_name = "TOOL")]
    pub(crate) call: Vec<String>,
    /// Call every tool the server lists, read-only or not.
    #[arg(long, help_heading = "What is called")]
    pub(crate) call_all: bool,
    /// The most requests sent in the run.
    #[arg(help_heading = "Limits", long, value_name = "N", default_value_t = 100)]
    pub(crate) max_requests: usize,
    /// The most tools called.
    #[arg(help_heading = "Limits", long, value_name = "N", default_value_t = 25)]
    pub(crate) max_tool_calls: usize,
    /// The most resources read, and the most prompts fetched.
    #[arg(help_heading = "Limits", long, value_name = "N", default_value_t = 25)]
    pub(crate) max_reads: usize,
    /// The most pages followed for one listing.
    #[arg(help_heading = "Limits", long, value_name = "N", default_value_t = 20)]
    pub(crate) max_pages: usize,
    /// The longest wait for one answer, in seconds.
    #[arg(
        help_heading = "Limits",
        long,
        value_name = "SECONDS",
        default_value_t = 30
    )]
    pub(crate) timeout: u64,
    /// The longest the whole run may take, in seconds.
    #[arg(
        help_heading = "Limits",
        long,
        value_name = "SECONDS",
        default_value_t = 300
    )]
    pub(crate) max_time: u64,
    /// The most bytes read for one answer.
    #[arg(help_heading = "Limits", long, value_name = "BYTES", default_value_t = 2 * 1024 * 1024)]
    pub(crate) max_response_bytes: usize,
    /// Where to keep the recording (JSON Lines). By default a new file in a
    /// new private directory under the system's temporary one, with the report
    /// as a page (`report.html`) beside it; the paths are printed, and
    /// `validate`, `serve` and `library import` read the recording. With `-o`
    /// the page is written only where `--html-report` says.
    #[arg(help_heading = "The recording", short, long, value_name = "PATH")]
    pub(crate) output: Option<PathBuf>,
    /// Overwrite the recording if it exists.
    #[arg(help_heading = "The recording", long, requires = "output")]
    pub(crate) force: bool,
    /// Also write the run's account of itself — what was listed, called, left
    /// alone and why — as JSON to FILE.
    #[arg(help_heading = "The recording", long, value_name = "FILE")]
    pub(crate) summary: Option<PathBuf>,
    #[command(flatten, next_help_heading = "The report")]
    pub(crate) report: mcp_trace_validator::cli::ReportArgs,
}

/// The revision `check` speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SpecVersion {
    /// `server/discover` first, then `initialize`.
    Auto,
    /// The `initialize` handshake only.
    #[value(name = "2025-11-25")]
    Legacy,
    /// Per-request metadata only.
    #[value(name = "2026-07-28")]
    Modern,
}
