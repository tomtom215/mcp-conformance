// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-reference-host` — drive a bounded, scripted tool-use loop against an
//! MCP server, as the official suite's client SUT or standalone.
//!
//! The pinned suite's client-SUT contract (ADR-0009): the runner appends the
//! scenario server's URL as the final argument, names the scenario in
//! `MCP_CONFORMANCE_SCENARIO`, and expects a clean exit within 30 s.
//! Standalone use: `--url <http>` or `--server-cmd "<stdio server cmd>"`.
//!
//! Exit codes: `0` run completed, `1` run failed (stop reason, transport, or
//! scenario error), `2` invocation error (clap's convention).
//!
//! Diagnostics go to stderr; stdout stays silent (suite runs capture it, and
//! a future stdout report format must not have to fight old noise).

// SEP-2577 forward-deprecates Logging, and rmcp 3.x carries the attribute, so
// naming `LoggingLevel` fires it on correct code: the level a request asks for
// is how `2026-07-28` *replaced* `logging/setLevel`, and it is the only way a
// recording can carry the logging clauses at all. Scoped to this module rather
// than the crate, matching `mcp-everything-server`'s two module-level allows —
// a blanket allow would also hide a deprecation that genuinely matters.
#![allow(deprecated)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use mcp_reference_host::capture::{CaptureTransport, RecordingTransport};
use mcp_reference_host::handler::HostHandler;
use mcp_reference_host::run::{RunPlan, RunReport, StopReason, run};
use mcp_reference_host::scenario::{ScenarioPlan, plan_for};
use mcp_reference_host::script::InteractionScript;
use mcp_reference_host::{subscribe, sweep};
use rmcp::model::{LoggingLevel, ProtocolVersion};
use rmcp::service::{ClientLifecycleMode, ClientServiceExt as _};
use rmcp::transport::Transport;
use tokio_util::sync::CancellationToken;

/// Reference MCP host: scripted client behavior, bounded loops, suite SUT.
#[derive(Debug, Parser)]
#[command(name = "mcp-reference-host", version, about, long_about = None)]
struct Cli {
    /// Streamable HTTP URL of the server. The official runner passes this as
    /// the final positional argument; `--url` is the standalone spelling.
    #[arg(long, conflicts_with = "server_cmd")]
    url: Option<String>,
    /// Spawn this stdio server as a child process (split on spaces, the same
    /// convention the official runner applies to its `--command`).
    #[arg(long, conflicts_with = "url")]
    server_cmd: Option<String>,
    /// Record the session as a validator-ready JSON Lines trace in this
    /// directory (one file per run).
    #[arg(long, value_name = "DIR")]
    trace_dir: Option<PathBuf>,
    /// The URL the official runner appends (equivalent to `--url`).
    #[arg(value_name = "URL")]
    positional_url: Option<String>,
    /// Hard deadline for the whole run, in seconds. The host owns its own
    /// exit: a server that never answers must produce a diagnostic and exit 1
    /// here — the official runner's 30 s kill reaches only the `sh -c`
    /// wrapper it spawns, and an orphaned host holding its pipes open would
    /// wedge the runner forever (measured against suite 0.1.16).
    #[arg(long, default_value_t = 25)]
    deadline_secs: u64,
    /// Let the run continue past this many errors. Overrides the scenario
    /// plan's budget, which is `0` because the suite's scenarios judge a
    /// clean run; a recording sweeping every tool meets `test_error_handling`,
    /// whose whole job is to return one.
    #[arg(long, value_name = "N")]
    error_budget: Option<u32>,
    /// Cap the run at this many turns, overriding the scenario plan's.
    #[arg(long, value_name = "N")]
    turn_limit: Option<u32>,
    /// Open a `subscriptions/listen` stream before the tool loop and drain it
    /// to its end. `2026-07-28` only — the method does not exist before it.
    #[arg(long)]
    subscribe: bool,
    /// After the tool loop, drive the rest of the server's surface: prompts,
    /// resources, templates, completion, and one read of a URI the catalog
    /// does not contain. A recording without this evidences the tool clauses
    /// and nothing else.
    #[arg(long)]
    sweep: bool,
    /// Ask every tool call for log messages at this level or above, through
    /// `_meta.io.modelcontextprotocol/logLevel`. Omitted, the request asks for
    /// none — which `2026-07-28` requires a server to honour by staying
    /// silent, so a recording that never asks cannot judge the logging clauses.
    #[arg(long, value_name = "LEVEL")]
    log_level: Option<LogLevel>,
    /// Protocol revision to speak. `2026-07-28` uses the stateless lifecycle:
    /// `server/discover` instead of `initialize`, and a `_meta` envelope on
    /// every request.
    #[arg(long, value_enum, default_value_t = Revision::default())]
    protocol_version: Revision,
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
enum LogLevel {
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
enum Revision {
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

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let deadline = std::time::Duration::from_secs(cli.deadline_secs);
    (tokio::time::timeout(deadline, dispatch(cli)).await).unwrap_or_else(|_| {
        eprintln!(
            "mcp-reference-host: run exceeded its {}s deadline — the server \
             stopped answering; raise --deadline-secs only if the server is \
             legitimately that slow",
            deadline.as_secs()
        );
        ExitCode::FAILURE
    })
}

/// Scenario dispatch: everything between argument parsing and the exit code.
async fn dispatch(cli: Cli) -> ExitCode {
    let scenario = std::env::var("MCP_CONFORMANCE_SCENARIO").ok();
    let plan = plan_for(scenario.as_deref());
    eprintln!(
        "mcp-reference-host: scenario {:?}",
        scenario.as_deref().unwrap_or("(none: generic plan)")
    );

    let url = cli.url.or(cli.positional_url);
    let lifecycle = ClientLifecycleMode::from(cli.protocol_version);
    let plan = overridden(plan, cli.error_budget, cli.turn_limit, cli.log_level);
    let extras = Extras {
        subscribe: cli.subscribe,
        sweep: cli.sweep,
    };
    match (plan, url, cli.server_cmd) {
        (ScenarioPlan::SseRetry, Some(url), _) => sse_retry(&url).await,
        (ScenarioPlan::SseRetry, None, _) => {
            eprintln!("mcp-reference-host: the sse-retry scenario needs a server URL");
            ExitCode::from(2)
        }
        (ScenarioPlan::Agent { script, plan }, url, server_cmd) => {
            let session = Session {
                script,
                plan,
                lifecycle,
                extras,
            };
            agent_over(url, server_cmd, cli.trace_dir, session).await
        }
    }
}

/// Connects the agent session over whichever transport the invocation named.
///
/// The two real transports differ only in how they are built and what the
/// trace calls them; keeping the arms adjacent is what stops one of them
/// silently missing an option the other gained.
async fn agent_over(
    url: Option<String>,
    server_cmd: Option<String>,
    trace_dir: Option<PathBuf>,
    session: Session,
) -> ExitCode {
    match (url, server_cmd) {
        (Some(url), None) => {
            let transport = mcp_reference_host::connect::streamable_http(&url);
            recorded(
                transport,
                CaptureTransport::StreamableHttp,
                trace_dir,
                session,
            )
            .await
        }
        (None, Some(command)) => match mcp_reference_host::connect::child_process(&command) {
            Ok(transport) => recorded(transport, CaptureTransport::Stdio, trace_dir, session).await,
            Err(error) => {
                eprintln!("mcp-reference-host: cannot spawn {command:?}: {error}");
                ExitCode::FAILURE
            }
        },
        (None, None) => {
            eprintln!(
                "mcp-reference-host: pass a server URL (positional or --url) or --server-cmd"
            );
            ExitCode::from(2)
        }
        (Some(_), Some(_)) => {
            // clap's conflicts_with already rejects this; kept as defense.
            eprintln!("mcp-reference-host: --url and --server-cmd are mutually exclusive");
            ExitCode::from(2)
        }
    }
}

/// Everything a run needs beyond its transport.
///
/// One struct because the four always travel together and neither transport
/// arm varies them; splitting them across an argument list only gave the two
/// arms more to keep in step.
struct Session {
    script: InteractionScript,
    plan: RunPlan,
    lifecycle: ClientLifecycleMode,
    extras: Extras,
}

/// Runs `session` over `transport`, wrapping it in the recorder when the
/// operator asked for a trace.
async fn recorded<T: Transport<rmcp::service::RoleClient> + 'static>(
    transport: T,
    kind: CaptureTransport,
    trace_dir: Option<PathBuf>,
    session: Session,
) -> ExitCode {
    match trace_dir {
        Some(dir) => match recording(transport, kind, &dir) {
            Ok(transport) => agent_run(transport, session).await,
            Err(code) => code,
        },
        None => agent_run(transport, session).await,
    }
}

/// `plan` with the operator's bounds applied, when they gave any.
///
/// The scenario table's bounds are a *contract* with the official suite, so
/// they are the default and never rewritten in place; an override is for a run
/// the suite does not define — a recording sweeping every tool, where meeting
/// `test_error_handling` is the point rather than a failure.
fn overridden(
    plan: ScenarioPlan,
    error_budget: Option<u32>,
    turn_limit: Option<u32>,
    log_level: Option<LogLevel>,
) -> ScenarioPlan {
    let ScenarioPlan::Agent { script, mut plan } = plan else {
        // `sse-retry` runs its own dance with no plan to bound.
        return plan;
    };
    if let Some(error_budget) = error_budget {
        plan.error_budget = error_budget;
    }
    if let Some(turn_limit) = turn_limit {
        plan.turn_limit = turn_limit;
    }
    // Left `None` unless asked: the suite's scenarios judge a session that
    // requested no logs, and a host that asked anyway would change what the
    // server under test emits during a scenario nobody wrote for it.
    plan.log_level = log_level.map(Into::into);
    ScenarioPlan::Agent { script, plan }
}

/// What a run does either side of the tool loop.
///
/// Bundled rather than passed as two `bool`s: at the call sites they are
/// adjacent and identically typed, which is exactly where an argument swap
/// stops being a compile error.
#[derive(Debug, Clone, Copy)]
struct Extras {
    /// Drain one `subscriptions/listen` stream before the loop.
    subscribe: bool,
    /// Drive the non-tool surface after it.
    sweep: bool,
}

/// The sse-retry scenario: the host's own compliant resumption dance
/// (rmcp 1.7's transport cannot pass it — ADR-0009 §Amendment).
async fn sse_retry(url: &str) -> ExitCode {
    match mcp_reference_host::resume::run_sse_retry(url).await {
        Ok(report) => {
            eprintln!(
                "mcp-reference-host: sse-retry dance completed (waited {:?}, \
                 Last-Event-ID {:?}): {}",
                report.waited, report.last_event_id, report.tool_result_text
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("mcp-reference-host: sse-retry dance failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Wraps `transport` in the trace recorder, creating the directory and a
/// timestamped file name; failures are invocation errors (exit 2) because
/// the operator asked for a recording that cannot happen.
fn recording<T>(
    transport: T,
    kind: CaptureTransport,
    dir: &std::path::Path,
) -> Result<RecordingTransport<T>, ExitCode> {
    if let Err(error) = std::fs::create_dir_all(dir) {
        eprintln!(
            "mcp-reference-host: cannot create trace dir {}: {error}",
            dir.display()
        );
        return Err(ExitCode::from(2));
    }
    // One file per run: scenario name (when set) + pid keeps concurrent
    // suite scenarios from colliding in a shared directory.
    let scenario = std::env::var("MCP_CONFORMANCE_SCENARIO").unwrap_or_else(|_| "run".to_owned());
    let path = dir.join(format!(
        "{}-{}.jsonl",
        scenario.replace(['/', '\\'], "-"),
        std::process::id()
    ));
    eprintln!("mcp-reference-host: recording trace to {}", path.display());
    RecordingTransport::create(transport, kind, &path).map_err(|error| {
        eprintln!("mcp-reference-host: cannot create trace file: {error}");
        ExitCode::from(2)
    })
}

/// Connects over `transport`, runs the bounded loop, and reports.
async fn agent_run(
    transport: impl Transport<rmcp::service::RoleClient> + 'static,
    session: Session,
) -> ExitCode {
    let Session {
        script,
        plan,
        lifecycle,
        extras,
    } = session;
    let handler = HostHandler::new(script);
    let client = match handler
        .clone()
        .serve_with_lifecycle(transport, lifecycle)
        .await
    {
        Ok(client) => client,
        Err(error) => {
            eprintln!("mcp-reference-host: initialization failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if extras.subscribe && !drained(&client).await {
        let _ = client.cancel().await;
        return ExitCode::FAILURE;
    }
    let report = run(&client, &plan, &CancellationToken::new()).await;
    render(&report);
    if extras.sweep {
        // After the loop, not before: the sweep reads resources the tools may
        // have changed, and a recording is easier to follow when the
        // discovery-driven half sits on one side of the tool calls.
        render_sweep(&sweep::run(client.peer()).await);
    }
    let clean_shutdown = client.cancel().await.is_ok();
    if report.stop == StopReason::Completed && clean_shutdown {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Drives one subscription to its end, reporting what it carried.
///
/// Before the tool loop rather than after: the loop ends by disconnecting, and
/// a subscription still open at that moment would end abruptly — which is a
/// different clause from the graceful closure this is here to exercise.
async fn drained<S: rmcp::service::Service<rmcp::service::RoleClient>>(
    client: &rmcp::service::RunningService<rmcp::service::RoleClient, S>,
) -> bool {
    // The URIs the reference server publishes and one it does not, so the
    // acknowledgment in a recording shows the server narrowing the filter
    // rather than leaving that to be assumed.
    let filter = subscribe::everything("test://static-text", "test://not-a-resource");
    match subscribe::drain(client, filter).await {
        Ok(report) => {
            eprintln!(
                "mcp-reference-host: subscription acknowledged [{}], received [{}], ended {}",
                report.acknowledged.join(", "),
                report.notifications.join(", "),
                report.ended
            );
            true
        }
        Err(error) => {
            eprintln!("mcp-reference-host: subscription failed: {error}");
            false
        }
    }
}

/// The sweep record, one line per step, on stderr.
///
/// Errors are printed but not counted against the exit code: the sweep ends
/// with a read that is *meant* to fail, and a host that exited non-zero for it
/// would make the capture harness fail on its own design.
fn render_sweep(report: &sweep::SweepReport) {
    eprintln!(
        "mcp-reference-host: swept {} step(s), {} drew errors",
        report.steps.len(),
        report.errors()
    );
    for step in &report.steps {
        match &step.outcome {
            Ok(summary) => eprintln!("  ok   {}: {summary}", step.what),
            Err(error) => eprintln!("  err  {}: {error}", step.what),
        }
    }
}

/// The run record, one line per call, on stderr.
fn render(report: &RunReport) {
    eprintln!(
        "mcp-reference-host: {:?} after {} turn(s), {} error(s)",
        report.stop, report.turns, report.errors
    );
    for outcome in &report.outcomes {
        match &outcome.result {
            Ok(text) => eprintln!("  ok   {}: {text}", outcome.tool),
            Err(error) => eprintln!("  err  {}: {error}", outcome.tool),
        }
    }
}
