// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-trace-capture` — record an MCP session as a validator-ready trace.
//!
//! Exit codes (stable interface):
//!
//! | Code | Meaning |
//! |------|---------|
//! | server's | `stdio`: the wrapped server's own exit code, passed through (128 + signal when it was killed by one, on Unix) |
//! | 0    | `http`: stopped cleanly with a complete trace |
//! | 128 + signal | `http`: a second signal stopped the proxy without waiting for open requests |
//! | 2    | Invocation problem: bad arguments, an existing output file, a server that would not start, an address that would not bind (no trace file is created) |
//! | 3    | The session ran but the trace is incomplete (a write failed); only when the exit code would otherwise be 0 |

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use mcp_conformance_core::trace::{Direction, LifecycleEvent, TransportKind};
use mcp_trace_capture::{DEFAULT_MAX_MESSAGE, Recorder, Signals, http, stdio};

pub mod output;

const EXIT_USAGE: u8 = 2;

/// Record a Model Context Protocol session as a trace for `mcp-trace-validator`.
#[derive(Debug, Parser)]
#[command(name = "mcp-trace-capture", version, about, long_about = None)]
struct Cli {
    /// Where to write the trace (JSON Lines).
    #[arg(
        short,
        long,
        value_name = "PATH",
        default_value = "mcp-trace.jsonl",
        global = true
    )]
    output: PathBuf,
    /// Overwrite the output file if it exists.
    #[arg(long, global = true)]
    force: bool,
    /// The largest message recorded, in bytes; larger ones are forwarded unrecorded.
    #[arg(long, value_name = "BYTES", default_value_t = DEFAULT_MAX_MESSAGE, global = true)]
    max_message_bytes: usize,
    #[command(subcommand)]
    mode: Mode,
}

#[derive(Debug, Subcommand)]
enum Mode {
    /// Wrap a stdio server: configure your client to launch
    /// `mcp-trace-capture stdio -- <server> [args…]` instead of the server.
    Stdio {
        /// The server command and its arguments.
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<OsString>,
    },
    /// Proxy a streamable-HTTP server: point your client at the listen address.
    Http {
        /// The server's base URL; request paths are appended to its path.
        #[arg(long, value_name = "URL")]
        upstream: axum::http::Uri,
        /// Where the proxy listens.
        #[arg(long, value_name = "ADDR", default_value = "127.0.0.1:8080")]
        listen: SocketAddr,
        /// Forward the client's `Host` header unchanged instead of naming the
        /// upstream, so the server's own `Host` validation sees the client's.
        #[arg(long)]
        preserve_host: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Err(message) = output::check(&cli.output, cli.force) {
        eprintln!("mcp-trace-capture: {message}");
        return ExitCode::from(EXIT_USAGE);
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mcp-trace-capture: cannot start the async runtime: {error}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let code = match &cli.mode {
        Mode::Stdio { command } => runtime.block_on(run_stdio(&cli, command)),
        Mode::Http {
            upstream,
            listen,
            preserve_host,
        } => runtime.block_on(run_http(&cli, upstream, *listen, *preserve_host)),
    };
    // A client blocked on this process's stdin would otherwise hold the runtime open.
    runtime.shutdown_background();
    ExitCode::from(code)
}

/// Starts the server, then creates the trace, then relays the session.
async fn run_stdio(cli: &Cli, command: &[OsString]) -> u8 {
    let Some((program, args)) = command.split_first() else {
        return EXIT_USAGE; // clap requires the command
    };
    let started = Signals::install()
        .and_then(|signals| stdio::spawn(program, args).map(|server| (signals, server)));
    let (signals, server) = match started {
        Ok(started) => started,
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            return EXIT_USAGE;
        }
    };
    let recorder = match output::open(&cli.output, cli.force, cli.max_message_bytes) {
        Ok(recorder) => recorder,
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            server.kill().await;
            return EXIT_USAGE;
        }
    };
    let code = match stdio::run_server(
        Arc::clone(&recorder),
        server,
        signals,
        cli.max_message_bytes,
    )
    .await
    {
        Ok(outcome) => {
            if let Some(stop) = outcome.stop {
                eprintln!(
                    "mcp-trace-capture: {} relayed to the server's process group; the \
                     server exited with {}",
                    stop.name(),
                    outcome.status
                );
            }
            report_stdio("client", outcome.client);
            report_stdio("server", outcome.server);
            exit_code_of(outcome.status)
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            1
        }
    };
    output::finish(&recorder, &cli.output, code)
}

/// Checks the upstream and binds, then creates the trace, then serves.
async fn run_http(
    cli: &Cli,
    upstream: &axum::http::Uri,
    listen: SocketAddr,
    preserve_host: bool,
) -> u8 {
    let options = match http::Options::new(upstream.clone(), cli.max_message_bytes) {
        Ok(mut options) => {
            options.preserve_host = preserve_host;
            options
        }
        Err(message) => {
            eprintln!("mcp-trace-capture: --upstream: {message}");
            return EXIT_USAGE;
        }
    };
    let listener = match tokio::net::TcpListener::bind(listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("mcp-trace-capture: cannot listen on {listen}: {error}");
            return EXIT_USAGE;
        }
    };
    let bound = listener.local_addr().unwrap_or(listen);
    // Installed before the address is announced: a client (or a test) may signal
    // the moment it reads that line.
    let signals = match Signals::install() {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("mcp-trace-capture: cannot install a signal handler: {error}");
            return EXIT_USAGE;
        }
    };
    let recorder = match output::open(&cli.output, cli.force, cli.max_message_bytes) {
        Ok(recorder) => recorder,
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            return EXIT_USAGE;
        }
    };
    if !bound.ip().is_loopback() {
        eprintln!(
            "mcp-trace-capture: warning: {bound} is not a loopback address; anyone who can \
             reach it can send requests through this proxy, credentials and all, and have \
             them recorded"
        );
    }
    eprintln!(
        "mcp-trace-capture: listening on http://{bound}, forwarding to {} (Ctrl-C to stop)",
        shown(upstream)
    );
    let code = serve_until_stopped(listener, &recorder, options, signals).await;
    output::finish(&recorder, &cli.output, code)
}

/// The upstream as announced: without its query, which can carry credentials.
fn shown(upstream: &axum::http::Uri) -> String {
    let path = upstream.path();
    let authority = upstream
        .authority()
        .map_or("", |authority| authority.as_str());
    let scheme = upstream.scheme_str().unwrap_or("http");
    let query = if upstream.query().is_some() {
        "?…"
    } else {
        ""
    };
    format!("{scheme}://{authority}{path}{query}")
}

/// Serves until a request to stop, then lets the proxy stop within its grace
/// period — or at once on a second request.
async fn serve_until_stopped(
    listener: tokio::net::TcpListener,
    recorder: &Arc<Recorder>,
    options: http::Options,
    mut signals: Signals,
) -> u8 {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = http::serve(listener, Arc::clone(recorder), options, async {
        let _ = stopped.await;
    });
    tokio::pin!(serving);
    let first = tokio::select! {
        served = &mut serving => return http_summary(served),
        first = signals.recv() => first,
    };
    eprintln!(
        "mcp-trace-capture: {}: stopping; open event streams are ended, other requests get \
         {}s (signal again to stop now)",
        first.name(),
        mcp_trace_capture::STOP_GRACE.as_secs()
    );
    let _ = stop.send(());
    tokio::select! {
        served = &mut serving => http_summary(served),
        again = signals.recv() => {
            let _ = recorder.close(
                Direction::ClientToServer,
                TransportKind::StreamableHttp,
                LifecycleEvent::TransportClose,
            );
            eprintln!(
                "mcp-trace-capture: {} again: stopped without waiting for open requests",
                again.name()
            );
            exit_code_of_stop(again)
        }
    }
}

/// The exit code for a capture a second signal stopped: as a shell reports a
/// process that signal ended.
fn exit_code_of_stop(stop: mcp_trace_capture::Stop) -> u8 {
    u8::try_from(128 + stop.number()).unwrap_or(1)
}

fn http_summary(served: std::io::Result<http::Unrecorded>) -> u8 {
    match served {
        Ok(unrecorded) => {
            report_unrecorded("session", unrecorded.not_json, unrecorded.oversized);
            if unrecorded.upstream_failures > 0 {
                eprintln!(
                    "mcp-trace-capture: {} request(s) could not reach the upstream and were \
                     answered 502 by the proxy (recorded as transport-abort)",
                    unrecorded.upstream_failures
                );
            }
            if unrecorded.upstream_cut > 0 {
                eprintln!(
                    "mcp-trace-capture: {} response(s) were cut off by the upstream mid-body; \
                     the truncation was relayed (recorded as transport-abort)",
                    unrecorded.upstream_cut
                );
            }
            0
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            EXIT_USAGE
        }
    }
}

/// The stdio summary: non-JSON lines are in the trace (as string payloads), so
/// they are reported as findings to look for rather than as gaps in it.
fn report_stdio(side: &str, unrecorded: stdio::Unrecorded) {
    if unrecorded.not_json > 0 {
        eprintln!(
            "mcp-trace-capture: {} {side} line(s) were not JSON; each is recorded as a \
             string payload, which the validator reports as not a valid MCP message",
            unrecorded.not_json
        );
    }
    report_unrecorded(side, 0, unrecorded.oversized);
}

fn report_unrecorded(side: &str, not_json: u64, oversized: u64) {
    if not_json > 0 {
        eprintln!(
            "mcp-trace-capture: {not_json} {side} message(s) were not JSON and are not in \
             the trace (forwarded unchanged)"
        );
    }
    if oversized > 0 {
        eprintln!(
            "mcp-trace-capture: {oversized} {side} message(s) were over the size limit \
             (--max-message-bytes) and are not in the trace (forwarded unchanged)"
        );
    }
}

/// The wrapped server's exit code, as a shell would report it.
fn exit_code_of(status: std::process::ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code & 0xFF).unwrap_or(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return u8::try_from(128 + (signal & 0x7F)).unwrap_or(1);
        }
    }
    1
}
