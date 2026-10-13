// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A whole capture run, as `mcp-trace-capture stdio` and `http` make it.
//!
//! The trace is created only once the session can start, signals are relayed
//! or obeyed, what went unrecorded is said on stderr, and the exit code is the
//! one the binary's README documents.
//!
//! The binary is a command line over these two functions. `mcp-conformance
//! watch` calls the same two with an [`Observer`], so a watched session is
//! recorded exactly as a captured one is — same files, same messages, same
//! exit codes — and what it adds sees each event as it is written.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use mcp_conformance_core::trace::LifecycleEvent;

use crate::output::{self, Target};
use crate::recorder::Observer;
use crate::traces::Traces;
use crate::{Signals, http, stdio};

mod summary;
pub mod values;

/// The exit code for an invocation problem (the README's table).
pub const EXIT_USAGE: i32 = 2;

/// Where and how a run records: the binary's `-o`, `--force` and
/// `--max-message-bytes`, and who else sees each event.
#[derive(Debug)]
#[non_exhaustive]
pub struct Recording<'a> {
    /// Where traces go, as [`output::check`] accepted it.
    pub target: &'a Target,
    /// Overwrite an existing trace file.
    pub force: bool,
    /// The largest message recorded, in bytes.
    pub max_message: usize,
    /// Shown every event as it is written.
    pub observer: Option<Arc<dyn Observer>>,
}

impl<'a> Recording<'a> {
    /// Recording to `target` as the binary does by default: no overwriting,
    /// the default message limit, no observer.
    #[must_use]
    pub const fn new(target: &'a Target) -> Self {
        Self {
            target,
            force: false,
            max_message: crate::DEFAULT_MAX_MESSAGE,
            observer: None,
        }
    }
}

/// The process's exit status for `code`.
///
/// Windows keeps a process's whole exit code, which `ExitCode` (a `u8`) cannot
/// carry, so one that does not fit is passed with `std::process::exit` — the
/// process ends here; everywhere else every code a run returns fits.
#[must_use]
pub fn exit(code: i32) -> ExitCode {
    if whole_exit(code, cfg!(windows)) {
        std::process::exit(code);
    }
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Whether `code` must be passed whole, past `ExitCode`.
const fn whole_exit(code: i32, windows: bool) -> bool {
    windows && (code < 0 || code > 255)
}

/// `mcp-trace-capture stdio -- <command>`: starts the server, then creates the
/// trace, then relays the session; returns the exit code.
pub async fn stdio(recording: Recording<'_>, command: &[OsString]) -> i32 {
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
    let (recorder, path) = match output::open(
        recording.target,
        recording.force,
        recording.max_message,
        recording.observer,
    ) {
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
        recording.max_message,
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
            summary::report_stdio("client", outcome.client);
            summary::report_stdio("server", outcome.server);
            (exit_code_of(outcome.status), Some(outcome.status))
        }
        Err(error) => {
            eprintln!("mcp-trace-capture: {error}");
            (1, None)
        }
    };
    output::finish(&recorder, &path, code, ended)
}

/// `mcp-trace-capture http`: checks the upstream and binds, then creates the
/// trace, then serves until a request to stop; returns the exit code.
pub async fn http(
    recording: Recording<'_>,
    upstream: &str,
    listen: SocketAddr,
    preserve_host: bool,
) -> i32 {
    let checked = values::upstream(upstream)
        .and_then(|upstream| http::Options::new(upstream, recording.max_message));
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
    let (traces, path) = match output::traces(
        recording.target,
        recording.force,
        recording.max_message,
        recording.observer,
    ) {
        Ok(opened) => opened,
        Err(message) => {
            eprintln!("mcp-trace-capture: {message}");
            return EXIT_USAGE;
        }
    };
    summary::announce(bound, upstream);
    let served = serve_until_stopped(listener, &traces, options, signals).await;
    let code = served.map_or_else(
        |code| code,
        |served| summary::http_summary(served, bound, upstream),
    );
    output::finish_traces(&traces, path.as_deref().map(Path::new), code)
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
        crate::STOP_GRACE.as_secs()
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
const fn exit_code_of_stop(stop: crate::Stop) -> i32 {
    128 + stop.number()
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
