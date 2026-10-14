// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `watch`: a session recorded as `mcp-trace-capture` records it, judged as it
//! runs (ADR-0021, decision 3).
//!
//! The recording is `mcp-trace-capture`'s own code path
//! ([`mcp_trace_capture::run`]) — the same files, flags, stderr and exit
//! codes — with an [`Observer`](mcp_trace_capture::Observer) added. Like the
//! tap it is read-only: bytes are forwarded unchanged and never wait for
//! judging or export (ADR-0019). What `watch` adds happens on the far side of
//! a channel: the judging thread ([`judging`]) and, with `--otlp`, the export
//! task ([`export`]).

use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use mcp_conformance_cli::live::Live;
use mcp_conformance_cli::otlp::Peer;
use mcp_trace_capture::output::{self, Target};
use mcp_trace_capture::run::{self, EXIT_USAGE, Recording};
use mcp_trace_validator::report::printable;
use tokio::runtime::Runtime;

use crate::args::{WatchArgs, WatchMode};

mod export;
mod ids;
mod judging;
mod print;

/// How long the end of a run waits for the last spans to reach the collector.
const EXPORT_DEADLINE: Duration = Duration::from_secs(10);

/// Runs `watch`; the exit code is the recorder's.
pub(crate) fn main(runtime: &Runtime, args: &WatchArgs) -> ExitCode {
    let target = match output::check(&args.output, args.force) {
        Ok(target) => target,
        Err(message) => {
            eprintln!("{} {message}", print::PREFIX);
            return run::exit(EXIT_USAGE);
        }
    };
    let endpoint = match args
        .otlp
        .as_deref()
        .map(export::Endpoint::parse)
        .transpose()
    {
        Ok(endpoint) => endpoint,
        Err(message) => {
            eprintln!("{} --otlp: {message}", print::PREFIX);
            return run::exit(EXIT_USAGE);
        }
    };
    if let Err(error) = Live::new() {
        eprintln!("{} built-in registry: {error}", print::PREFIX);
        return run::exit(EXIT_USAGE);
    }
    let (sink, exporting) = endpoint
        .as_ref()
        .map(|endpoint| start_export(runtime, endpoint))
        .unzip();
    let peer = match &args.mode {
        WatchMode::Http { upstream, .. } => peer(upstream),
        WatchMode::Stdio { .. } => None,
    };
    let (tap, thread) = judging::start(judging::Setup {
        out: Box::new(std::io::stderr()),
        terminal: std::io::stderr().is_terminal(),
        numbered: matches!(target, Target::Numbered(_)),
        name: names(&target),
        spans: sink,
        peer,
        judge: Box::new(Live::judge),
    });
    let notes = tap.0.clone();
    let mut recording = Recording::new(&target);
    recording.force = args.force;
    recording.max_message = args.max_message_bytes;
    recording.observer = Some(Arc::new(tap));
    let code = match &args.mode {
        WatchMode::Stdio { command } => runtime.block_on(run::stdio(recording, command)),
        WatchMode::Http {
            upstream,
            listen,
            preserve_host,
        } => runtime.block_on(run::http(recording, upstream, *listen, *preserve_host)),
    };
    let _ = notes.send(judging::Note::End);
    let judged = thread.join().unwrap_or_default();
    if let (Some(task), Some(endpoint)) = (exporting, &endpoint) {
        exported(runtime, task, endpoint, judged.unattached);
    }
    run::exit(code)
}

/// Announces the export and starts it on `runtime`.
fn start_export(
    runtime: &Runtime,
    endpoint: &export::Endpoint,
) -> (export::SpanSink, tokio::task::JoinHandle<export::Exported>) {
    if endpoint.is_plaintext_remote() {
        eprintln!(
            "{} warning: spans go to {} unencrypted (http:// to a host that is not this \
             machine): method, tool and prompt names, resource URIs, session IDs and findings",
            print::PREFIX,
            endpoint.shown()
        );
    }
    eprintln!(
        "{} exporting one span per exchange to {} (OTLP/HTTP, JSON)",
        print::PREFIX,
        endpoint.shown()
    );
    let _entered = runtime.enter();
    export::start(endpoint.clone())
}

/// The trace's name for the final report, by trace number.
fn names(target: &Target) -> Box<dyn Fn(u64) -> String + Send> {
    match target {
        Target::File(path) => {
            let path = path.display().to_string();
            Box::new(move |_| path.clone())
        }
        Target::Numbered(numbered) => {
            let numbered = numbered.clone();
            Box::new(move |number| numbered.path(number).display().to_string())
        }
    }
}

/// `server.address` and `server.port` for an HTTP session: the upstream's.
fn peer(upstream: &str) -> Option<Peer> {
    let uri = run::values::upstream(upstream).ok()?;
    let host = uri.host()?.trim_start_matches('[').trim_end_matches(']');
    let port = uri.port_u16().or_else(|| match uri.scheme_str() {
        Some("https") => Some(443),
        Some("http") => Some(80),
        _ => None,
    })?;
    Some(Peer::new(host, port))
}

/// Waits (boundedly) for the export to finish and says how it went.
fn exported(
    runtime: &Runtime,
    task: tokio::task::JoinHandle<export::Exported>,
    endpoint: &export::Endpoint,
    unattached: u64,
) {
    let shown = endpoint.shown();
    let done = runtime.block_on(async { tokio::time::timeout(EXPORT_DEADLINE, task).await });
    let Ok(Ok(exported)) = done else {
        eprintln!(
            "{} the export to {shown} did not finish within {}s; spans still queued are \
             not sent",
            print::PREFIX,
            EXPORT_DEADLINE.as_secs()
        );
        return;
    };
    eprintln!("{}", export_summary(&exported, &shown, unattached));
}

/// The line saying how the export went.
fn export_summary(exported: &export::Exported, shown: &str, unattached: u64) -> String {
    let mut line = format!(
        "{} exported {} span(s) to {shown}",
        print::PREFIX,
        exported.sent
    );
    if exported.failed > 0 {
        let _ = write!(
            line,
            "; {} not accepted ({})",
            exported.failed,
            printable(exported.error.as_deref().unwrap_or("unknown error"))
        );
    }
    if exported.dropped > 0 {
        let _ = write!(line, "; {} dropped, the queue being full", exported.dropped);
    }
    if unattached > 0 {
        let _ = write!(
            line,
            "; {unattached} finding(s) came before any exchange and are in no span"
        );
    }
    line
}

#[cfg(test)]
mod tests;
