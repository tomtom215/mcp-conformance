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
//! | 2    | Invocation problem: bad arguments, an existing output file, a server that would not start, an address that would not bind |
//! | 3    | The session ran but the trace is incomplete (a write failed); only when the exit code would otherwise be 0 |

use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use mcp_trace_capture::{DEFAULT_MAX_MESSAGE, Recorder, http, stdio};

const EXIT_USAGE: u8 = 2;
const EXIT_INCOMPLETE: u8 = 3;

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
    let recorder = match open_output(&cli) {
        Ok(recorder) => Arc::new(recorder),
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            return ExitCode::from(EXIT_USAGE);
        }
    };
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
    let code = match cli.mode {
        Mode::Stdio { mut command } => {
            let program = command.remove(0);
            runtime.block_on(run_stdio(
                &recorder,
                program,
                command,
                cli.max_message_bytes,
            ))
        }
        Mode::Http {
            upstream,
            listen,
            preserve_host,
        } => runtime.block_on(run_http(
            &recorder,
            upstream,
            listen,
            cli.max_message_bytes,
            preserve_host,
        )),
    };
    // A client blocked on this process's stdin would otherwise hold the runtime open.
    runtime.shutdown_background();
    finish(&recorder, &cli.output, code)
}

fn open_output(cli: &Cli) -> Result<Recorder, String> {
    let mut options = OpenOptions::new();
    options.write(true);
    if cli.force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let file = options.open(&cli.output).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            format!(
                "{} exists; appending would break the trace's sequence numbers \
                 (use --force to overwrite, or -o for another path)",
                cli.output.display()
            )
        } else {
            format!("cannot create {}: {error}", cli.output.display())
        }
    })?;
    eprintln!("mcp-trace-capture: recording to {}", cli.output.display());
    Ok(Recorder::new(BufWriter::new(file)))
}

async fn run_stdio(
    recorder: &Arc<Recorder>,
    program: OsString,
    args: Vec<OsString>,
    max_message: usize,
) -> u8 {
    match stdio::run(Arc::clone(recorder), program, args, max_message).await {
        Ok(outcome) => {
            report_unrecorded("client", outcome.client.not_json, outcome.client.oversized);
            report_unrecorded("server", outcome.server.not_json, outcome.server.oversized);
            exit_code_of(outcome.status)
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            EXIT_USAGE
        }
    }
}

async fn run_http(
    recorder: &Arc<Recorder>,
    upstream: axum::http::Uri,
    listen: SocketAddr,
    max_message: usize,
    preserve_host: bool,
) -> u8 {
    let options = match http::Options::new(upstream.clone(), max_message) {
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
    eprintln!(
        "mcp-trace-capture: listening on http://{bound}, forwarding to {upstream} (Ctrl-C to stop)"
    );
    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    match http::serve(listener, Arc::clone(recorder), options, shutdown).await {
        Ok(unrecorded) => {
            report_unrecorded("session", unrecorded.not_json, unrecorded.oversized);
            if unrecorded.upstream_failures > 0 {
                eprintln!(
                    "mcp-trace-capture: {} request(s) could not reach the upstream and were \
                     answered 502 by the proxy (recorded as transport-abort)",
                    unrecorded.upstream_failures
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

fn report_unrecorded(side: &str, not_json: u64, oversized: u64) {
    if not_json > 0 {
        eprintln!(
            "mcp-trace-capture: {not_json} {side} message(s) were not JSON and are not in \
             the trace (forwarded unchanged)"
        );
    }
    if oversized > 0 {
        eprintln!(
            "mcp-trace-capture: {oversized} {side} message(s) exceeded --max-message-bytes \
             and are not in the trace (forwarded unchanged)"
        );
    }
}

fn finish(recorder: &Recorder, output: &std::path::Path, code: u8) -> ExitCode {
    let summary = recorder.finish();
    if let Some(error) = &summary.error {
        eprintln!(
            "mcp-trace-capture: the trace at {} is incomplete: {} event(s) recorded, {} lost \
             after a write failed ({error})",
            output.display(),
            summary.recorded,
            summary.dropped
        );
        return ExitCode::from(if code == 0 { EXIT_INCOMPLETE } else { code });
    }
    eprintln!(
        "mcp-trace-capture: recorded {} event(s) to {}; validate with \
         `mcp-trace-validator validate {}`",
        summary.recorded,
        output.display(),
        output.display()
    );
    ExitCode::from(code)
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
