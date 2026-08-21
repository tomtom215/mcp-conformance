// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The command line: the argument struct and the value enums it parses into.
//!
//! Split from `main` so that file stays about *dispatch* — which transport,
//! which lifecycle, which phases — the same seam `render` was cut on.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, exactly as `render` does.
#![allow(clippy::redundant_pub_crate)]

use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use rmcp::model::{LoggingLevel, ProtocolVersion};
use rmcp::service::ClientLifecycleMode;

/// Reference MCP host: scripted client behavior, bounded loops, suite SUT.
///
/// The `struct_excessive_bools` lint reads four `bool` fields as a state
/// machine that wants a type. Here they are four independent command-line
/// switches, each naming one phase a recording may include, and clap's derive
/// binds a flag to a field: collapsing them into an enum would mean the flags
/// could no longer be combined, which is exactly what the capture does.
#[derive(Debug, Parser)]
#[command(name = "mcp-reference-host", version, about, long_about = None)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct Cli {
    /// Streamable HTTP URL of the server. The official runner passes this as
    /// the final positional argument; `--url` is the standalone spelling.
    #[arg(long, conflicts_with = "server_cmd")]
    pub(crate) url: Option<String>,
    /// Spawn this stdio server as a child process (split on spaces, the same
    /// convention the official runner applies to its `--command`).
    #[arg(long, conflicts_with = "url")]
    pub(crate) server_cmd: Option<String>,
    /// Record the session as a validator-ready JSON Lines trace in this
    /// directory (one file per run).
    #[arg(long, value_name = "DIR")]
    pub(crate) trace_dir: Option<PathBuf>,
    /// The URL the official runner appends (equivalent to `--url`).
    #[arg(value_name = "URL")]
    pub(crate) positional_url: Option<String>,
    /// Hard deadline for the whole run, in seconds. The host owns its own
    /// exit: a server that never answers must produce a diagnostic and exit 1
    /// here — the official runner's 30 s kill reaches only the `sh -c`
    /// wrapper it spawns, and an orphaned host holding its pipes open would
    /// wedge the runner forever (measured against suite 0.1.16).
    #[arg(long, default_value_t = 25)]
    pub(crate) deadline_secs: u64,
    /// Let the run continue past this many errors. Overrides the scenario
    /// plan's budget, which is `0` because the suite's scenarios judge a
    /// clean run; a recording sweeping every tool meets `test_error_handling`,
    /// whose whole job is to return one.
    #[arg(long, value_name = "N")]
    pub(crate) error_budget: Option<u32>,
    /// Cap the run at this many turns, overriding the scenario plan's, which
    /// is `16` for the generic plan. That bound is sized for the suite's
    /// scenarios, which publish one tool each; this workspace's own
    /// everything-server publishes more than sixteen, so a run meant to reach
    /// every tool needs a higher cap — `cargo xtask draft-capture` passes 32.
    #[arg(long, value_name = "N")]
    pub(crate) turn_limit: Option<u32>,
    /// Open a `subscriptions/listen` stream before the tool loop and drain it
    /// to its end. `2026-07-28` only — the method does not exist before it.
    #[arg(long)]
    pub(crate) subscribe: bool,
    /// Send the probe session instead of a normal run: deliberately malformed
    /// requests, one per rejection clause, so a recording carries what the
    /// server does when it must refuse. Requires `--url`; the probes are raw
    /// HTTP, which is the point — rmcp's client will not emit one.
    #[arg(long, conflicts_with_all = ["server_cmd", "sweep", "subscribe"])]
    pub(crate) probe: bool,
    /// After the tool loop, drive the rest of the server's surface: prompts,
    /// resources, templates, completion, and one read of a URI the catalog
    /// does not contain. A recording without this evidences the tool clauses
    /// and nothing else.
    #[arg(long)]
    pub(crate) sweep: bool,
    /// Cancel one answered tool call, then make another, so the recording
    /// carries a `notifications/cancelled` naming a request and a permitted
    /// server message after it. `2026-07-28`'s cancellation clauses are MUST
    /// NOTs, and a recording of nothing happening cannot evidence one.
    #[arg(long)]
    pub(crate) cancel: bool,
    /// Ask every tool call for log messages at this level or above, through
    /// `_meta.io.modelcontextprotocol/logLevel`. Omitted, the request asks for
    /// none — which `2026-07-28` requires a server to honour by staying
    /// silent, so a recording that never asks cannot judge the logging clauses.
    #[arg(long, value_name = "LEVEL")]
    pub(crate) log_level: Option<LogLevel>,
    /// Carry this W3C Trace Context `traceparent` in every tool call's
    /// `_meta`. Omitted, the requests carry none — which `2026-07-28` permits,
    /// and which leaves the trace-context clause with nothing to judge.
    #[arg(long, value_name = "TRACEPARENT")]
    pub(crate) traceparent: Option<String>,
    /// Protocol revision to speak. `2026-07-28` uses the stateless lifecycle:
    /// `server/discover` instead of `initialize`, and a `_meta` envelope on
    /// every request.
    #[arg(long, value_enum, default_value_t = Revision::default())]
    pub(crate) protocol_version: Revision,
}

/// The revision the host speaks, as a CLI value.
///
/// A separate enum from rmcp's [`ProtocolVersion`] because the choice here is
/// not "which version string" but "which lifecycle": `2026-07-28` removed the
/// handshake, so the two options differ in the messages the host sends before
/// it can send anything else.
/// The eight RFC 5424 severities, as a CLI value.
///
/// Spelled out rather than deriving on rmcp's [`LoggingLevel`]: that type is
/// `#[non_exhaustive]` and carries no `ValueEnum`, and the CLI's accepted set
/// is a contract with whoever scripts a capture.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum LogLevel {
    Debug,
    Info,
    Notice,
    Warning,
    Error,
    Critical,
    Alert,
    Emergency,
}

impl From<LogLevel> for LoggingLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => Self::Debug,
            LogLevel::Info => Self::Info,
            LogLevel::Notice => Self::Notice,
            LogLevel::Warning => Self::Warning,
            LogLevel::Error => Self::Error,
            LogLevel::Critical => Self::Critical,
            LogLevel::Alert => Self::Alert,
            LogLevel::Emergency => Self::Emergency,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub(crate) enum Revision {
    /// The `initialize` handshake.
    #[default]
    #[value(name = "2025-11-25")]
    V20251125,
    /// Stateless: `server/discover`, then a `_meta` envelope per request.
    #[value(name = "2026-07-28")]
    V20260728,
}

impl From<Revision> for ClientLifecycleMode {
    fn from(revision: Revision) -> Self {
        match revision {
            Revision::V20251125 => Self::Initialize,
            // Only this revision, not a preference list: the host is asked to
            // exercise the stateless surface, and a list that could fall back
            // would quietly record a legacy session instead when the server
            // does not serve it.
            Revision::V20260728 => Self::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        }
    }
}
