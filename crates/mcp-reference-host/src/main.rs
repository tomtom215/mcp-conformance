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

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use mcp_reference_host::capture::{CaptureTransport, RecordingTransport};
use mcp_reference_host::handler::HostHandler;
use mcp_reference_host::run::{RunPlan, RunReport, StopReason, run};
use mcp_reference_host::scenario::{ScenarioPlan, plan_for};
use mcp_reference_host::script::InteractionScript;
use rmcp::ServiceExt as _;
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
    match (plan, url, cli.server_cmd) {
        (ScenarioPlan::SseRetry, Some(url), _) => sse_retry(&url).await,
        (ScenarioPlan::SseRetry, None, _) => {
            eprintln!("mcp-reference-host: the sse-retry scenario needs a server URL");
            ExitCode::from(2)
        }
        (ScenarioPlan::Agent { script, plan }, Some(url), None) => {
            let transport = mcp_reference_host::connect::streamable_http(&url);
            match cli.trace_dir {
                Some(dir) => match recording(transport, CaptureTransport::StreamableHttp, &dir) {
                    Ok(transport) => agent_run(transport, script, plan).await,
                    Err(code) => code,
                },
                None => agent_run(transport, script, plan).await,
            }
        }
        (ScenarioPlan::Agent { script, plan }, None, Some(command)) => {
            let transport = match mcp_reference_host::connect::child_process(&command) {
                Ok(transport) => transport,
                Err(error) => {
                    eprintln!("mcp-reference-host: cannot spawn {command:?}: {error}");
                    return ExitCode::FAILURE;
                }
            };
            match cli.trace_dir {
                Some(dir) => match recording(transport, CaptureTransport::Stdio, &dir) {
                    Ok(transport) => agent_run(transport, script, plan).await,
                    Err(code) => code,
                },
                None => agent_run(transport, script, plan).await,
            }
        }
        (ScenarioPlan::Agent { .. }, None, None) => {
            eprintln!(
                "mcp-reference-host: pass a server URL (positional or --url) or --server-cmd"
            );
            ExitCode::from(2)
        }
        (ScenarioPlan::Agent { .. }, Some(_), Some(_)) => {
            // clap's conflicts_with already rejects this; kept as defense.
            eprintln!("mcp-reference-host: --url and --server-cmd are mutually exclusive");
            ExitCode::from(2)
        }
    }
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
    script: InteractionScript,
    plan: RunPlan,
) -> ExitCode {
    let handler = HostHandler::new(script);
    let client = match handler.clone().serve(transport).await {
        Ok(client) => client,
        Err(error) => {
            eprintln!("mcp-reference-host: initialization failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let report = run(&client, &plan, &CancellationToken::new()).await;
    render(&report);
    let clean_shutdown = client.cancel().await.is_ok();
    if report.stop == StopReason::Completed && clean_shutdown {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
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
