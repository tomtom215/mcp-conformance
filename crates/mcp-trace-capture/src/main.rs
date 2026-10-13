// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `mcp-trace-capture` — record an MCP session as a validator-ready trace.
//!
//! Exit codes (stable interface):
//!
//! | Code | Meaning |
//! |------|---------|
//! | server's | `stdio`: the wrapped server's own exit code, passed through (128 + signal when it was killed by one, on Unix; the whole code on Windows) |
//! | 1    | `stdio`: waiting for the server failed |
//! | 0    | `http`: stopped with a complete trace |
//! | 128 + signal | `http`: a second signal stopped the proxy without waiting for open requests |
//! | 2    | Invocation problem: bad arguments, an existing output file, a misplaced `{session}` or a missing directory for it, a server that would not start, an address that would not bind (no trace file is created); also `http` serving failed after it started |
//! | 3    | The session ran but a trace is incomplete (a write failed); only when the exit code would otherwise be 0 |

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use args::{Cli, Mode};
use clap::Parser as _;
use mcp_conformance_core::trace::LifecycleEvent;
use mcp_trace_capture::traces::Traces;
use mcp_trace_capture::{Signals, http, stdio};

pub mod args;
pub mod output;

const EXIT_USAGE: i32 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let target = match output::check(&cli.output, cli.force) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            return exit(EXIT_USAGE);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("mcp-trace-capture: cannot start the async runtime: {error}");
            return exit(EXIT_USAGE);
        }
    };
    let code = match &cli.mode {
        Mode::Stdio { command } => runtime.block_on(run_stdio(&cli, &target, command)),
        Mode::Http {
            upstream,
            listen,
            preserve_host,
        } => runtime.block_on(run_http(&cli, &target, upstream, *listen, *preserve_host)),
    };
    // A client blocked on this process's stdin would otherwise hold the runtime open.
    runtime.shutdown_background();
    exit(code)
}

/// Ends the process with `code`. Windows keeps a process's whole exit code,
/// which `ExitCode` (a `u8`) cannot carry, so one that does not fit is passed
/// with `std::process::exit`; everywhere else every code here fits.
fn exit(code: i32) -> ExitCode {
    if whole_exit(code, cfg!(windows)) {
        std::process::exit(code);
    }
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Whether `code` must be passed whole, past `ExitCode`.
const fn whole_exit(code: i32, windows: bool) -> bool {
    windows && (code < 0 || code > 255)
}

/// Starts the server, then creates the trace, then relays the session.
async fn run_stdio(cli: &Cli, target: &output::Target, command: &[OsString]) -> i32 {
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
    let (recorder, path) = match output::open(target, cli.force, cli.max_message_bytes) {
        Ok(opened) => opened,
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            server.kill().await;
            return EXIT_USAGE;
        }
    };
    let (code, ended) = match stdio::run_server(
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
            (exit_code_of(outcome.status), Some(outcome.status))
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            (1, None)
        }
    };
    output::finish(&recorder, &path, code, ended)
}

/// Checks the upstream and binds, then creates the trace, then serves.
async fn run_http(
    cli: &Cli,
    target: &output::Target,
    upstream: &str,
    listen: SocketAddr,
    preserve_host: bool,
) -> i32 {
    let checked = args::upstream(upstream)
        .and_then(|upstream| http::Options::new(upstream, cli.max_message_bytes));
    let options = match checked {
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
    let upstream = &options.upstream.clone();
    // Installed before the address is announced: a client (or a test) may signal
    // the moment it reads that line.
    let signals = match Signals::install() {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("mcp-trace-capture: cannot install a signal handler: {error}");
            return EXIT_USAGE;
        }
    };
    let (traces, path) = match output::traces(target, cli.force, cli.max_message_bytes) {
        Ok(opened) => opened,
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
    eprintln!("mcp-trace-capture: {}", client_url(bound, upstream));
    let served = serve_until_stopped(listener, &traces, options, signals).await;
    let code = served.map_or_else(|code| code, |served| http_summary(served, bound, upstream));
    output::finish_traces(&traces, path.as_deref().map(Path::new), code)
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

/// Where to point the client: the proxy's root is the upstream URL itself.
fn client_url(bound: SocketAddr, upstream: &axum::http::Uri) -> String {
    if upstream.path() == "/" {
        format!(
            "client URL: http://{bound}/ plus the server's endpoint path, e.g. \
             http://{bound}/mcp (each request's path is appended to the upstream's)"
        )
    } else {
        format!(
            "client URL: http://{bound}/ (it reaches {}; http://{bound}/<path> reaches \
             <upstream>/<path>)",
            shown(upstream)
        )
    }
}

/// Serves until a request to stop, then lets the proxy stop within its grace
/// period — or at once on a second request, whose exit code is the `Err`.
async fn serve_until_stopped(
    listener: tokio::net::TcpListener,
    traces: &Arc<Traces>,
    options: http::Options,
    mut signals: Signals,
) -> Result<std::io::Result<http::Unrecorded>, i32> {
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = http::serve_traces(listener, Arc::clone(traces), options, async {
        let _ = stopped.await;
    });
    tokio::pin!(serving);
    let first = tokio::select! {
        served = &mut serving => return Ok(served),
        first = signals.recv() => first,
    };
    eprintln!(
        "mcp-trace-capture: {}: stopping; standalone event streams (GET) are cut, other \
         requests get {}s (signal again to stop now)",
        first.name(),
        mcp_trace_capture::STOP_GRACE.as_secs()
    );
    let _ = stop.send(());
    tokio::select! {
        served = &mut serving => Ok(served),
        again = signals.recv() => {
            // Requests still in flight are cut off: the transport did not close in
            // an orderly way.
            traces.close_all(LifecycleEvent::TransportAbort);
            eprintln!(
                "mcp-trace-capture: {} again: stopped without waiting for open requests",
                again.name()
            );
            Err(exit_code_of_stop(again))
        }
    }
}

/// The exit code for a capture a second signal stopped: as a shell reports a
/// process that signal ended.
const fn exit_code_of_stop(stop: mcp_trace_capture::Stop) -> i32 {
    128 + stop.number()
}

fn http_summary(
    served: std::io::Result<http::Unrecorded>,
    bound: SocketAddr,
    upstream: &axum::http::Uri,
) -> i32 {
    match served {
        Ok(unrecorded) => {
            report_unrecorded("session", unrecorded.not_json, unrecorded.oversized);
            if unrecorded.altered > 0 {
                eprintln!(
                    "mcp-trace-capture: {} session message(s) were JSON the trace cannot hold \
                     as sent ({ALTERED}) and are not in the trace (forwarded unchanged)",
                    unrecorded.altered
                );
            }
            if unrecorded.other_events > 0 {
                eprintln!(
                    "mcp-trace-capture: {} SSE event(s) had a type other than `message`, which \
                     MCP clients ignore, and are not in the trace (forwarded unchanged)",
                    unrecorded.other_events
                );
            }
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
            report_statuses(unrecorded, bound, upstream);
            0
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            EXIT_USAGE
        }
    }
}

/// Says how many responses were not 2xx, and asks about the path when the
/// server answered 404: a client pointed at the wrong path gets 404s, recorded
/// faithfully and otherwise unremarked.
fn report_statuses(unrecorded: http::Unrecorded, bound: SocketAddr, upstream: &axum::http::Uri) {
    if unrecorded.not_2xx > 0 {
        eprintln!(
            "mcp-trace-capture: {} response(s) had a status outside 2xx (recorded as sent)",
            unrecorded.not_2xx
        );
    }
    if unrecorded.not_found > 0 {
        eprintln!(
            "mcp-trace-capture: the server answered 404 to {} request(s) — is the endpoint \
             path right? The proxy maps http://{bound}/ to {} and http://{bound}/<path> to \
             <upstream>/<path>",
            unrecorded.not_found,
            shown(upstream)
        );
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
    if unrecorded.altered > 0 {
        eprintln!(
            "mcp-trace-capture: {} {side} line(s) were JSON the trace cannot hold as sent \
             ({ALTERED}); each is recorded as a string payload, which the validator reports as \
             not a valid MCP message",
            unrecorded.altered
        );
    }
    if unrecorded.unterminated > 0 {
        eprintln!(
            "mcp-trace-capture: the {side}'s output ended mid-line, without the newline that \
             frames a stdio message; the remainder is recorded as a string payload, which the \
             validator reports as not a valid MCP message"
        );
    }
    report_unrecorded(side, 0, unrecorded.oversized);
}

/// Why a JSON message cannot be recorded as parsed JSON.
const ALTERED: &str = "a number a double cannot hold exactly, such as an integer past 64 bits, or a repeated \
     member name";

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
fn exit_code_of(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return passed_through(code, cfg!(windows));
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return 128 + (signal & 0x7F);
        }
    }
    1
}

/// A server's exit code as this process passes it on: whole where the platform
/// keeps a whole code (Windows: `exit(256)` is 256, which the low eight bits
/// would make a success), else the eight bits a Unix wait status holds.
const fn passed_through(code: i32, whole: bool) -> i32 {
    if whole { code } else { code & 0xFF }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exit_code_is_passed_whole_where_the_platform_keeps_it_whole() {
        assert_eq!(passed_through(7, false), 7);
        assert_eq!(passed_through(256, false), 0);
        assert_eq!(passed_through(256, true), 256);
        assert_eq!(passed_through(-1_073_741_819, true), -1_073_741_819);
        assert!(whole_exit(256, true));
        assert!(whole_exit(-1, true));
        assert!(!whole_exit(255, true));
        assert!(!whole_exit(0, true));
        assert!(!whole_exit(256, false));
        assert!(!whole_exit(-1, false));
    }
}
