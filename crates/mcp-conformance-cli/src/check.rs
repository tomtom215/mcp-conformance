// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check`: from a server to its clause report in one command (ADR-0024).
//!
//! It starts the recorder in front of the server (the stdio wrapper for a
//! command, the HTTP proxy for a URL), drives the exerciser through it
//! ([`session`]), and then judges the recording exactly as
//! `mcp-trace-validator validate --party server` would — the same code, flags,
//! formats and exit codes ([`mcp_trace_validator::cli`]). The exerciser links
//! no MCP SDK: the trace describes bytes, not an SDK's reading of them.
//!
//! The run's own account — what was listed, called, left alone and why — goes
//! to stderr, so stdout is the report alone in every format.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use axum::http::{HeaderName, HeaderValue};
use mcp_trace_capture::Signals;
use mcp_trace_capture::output;
use mcp_trace_validator::cli as validator;
use mcp_trace_validator::report::Party;

use crate::EXIT_USAGE;

pub(crate) mod args;
mod arguments;
mod headers;
mod request;
mod session;
mod sse;
mod summary;
mod wire;

use args::{CheckArgs, SpecVersion};
use request::{Era, LEGACY, MODERN, Requests};
use session::{Ended, Plan, Session};
use summary::Summary;
use wire::{Failure, Limits, Wire};

/// The exit status when the run was interrupted, as a shell reports SIGINT.
const EXIT_INTERRUPTED: u8 = 130;

/// Runs `check`; returns the exit status.
pub(crate) async fn run(args: CheckArgs) -> u8 {
    match check(&args).await {
        Ok(code) => code,
        Err(message) => {
            eprintln!("mcp-conformance check: {message}");
            EXIT_USAGE
        }
    }
}

/// What is being checked.
enum Subject {
    Url(String, Vec<(HeaderName, HeaderValue)>),
    Command(OsString, Vec<OsString>),
}

async fn check(args: &CheckArgs) -> Result<u8, String> {
    let subject = subject(args)?;
    let trace = match &args.output {
        Some(path) => path.clone(),
        None => fresh_trace()?,
    };
    let mut summary = Summary {
        subject: shown(args),
        transport: match subject {
            Subject::Url(..) => "streamable-http",
            Subject::Command(..) => "stdio",
        },
        trace: trace.display().to_string(),
        ..Summary::default()
    };
    let plan = plan(args);
    let mut signals =
        Signals::install().map_err(|error| format!("cannot install a signal handler: {error}"))?;
    let mut wire = connect(&subject, &trace, args.force).await?;
    let driven = {
        let driving = drive(
            &mut wire,
            &subject,
            &trace,
            args.spec_version,
            &plan,
            &mut summary,
        );
        tokio::select! {
            ended = driving => Some(ended),
            _ = signals.recv() => None,
        }
    };
    let closed = wire.close().await;
    eprint!("{}", summary.render());
    if let Some(path) = &args.summary {
        let json = serde_json::to_string_pretty(&summary)
            .map_err(|error| format!("cannot serialize the summary: {error}"))?;
        std::fs::write(path, json + "\n")
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    closed?;
    conclude(args, driven, &summary, &trace)
}

/// The bounds `args` set.
fn plan(args: &CheckArgs) -> Plan {
    Plan {
        call: args.call.clone(),
        call_all: args.call_all,
        max_requests: args.max_requests,
        max_tool_calls: args.max_tool_calls,
        max_reads: args.max_reads,
        max_pages: args.max_pages,
        deadline: Instant::now() + Duration::from_secs(args.max_time),
        limits: Limits {
            timeout: Duration::from_secs(args.timeout),
            max_response: args.max_response_bytes,
        },
    }
}

/// Judges the recording, unless the run ended in a way that leaves nothing a
/// verdict could be about; returns the exit status.
fn conclude(
    args: &CheckArgs,
    driven: Option<Result<(), Ended>>,
    summary: &Summary,
    trace: &Path,
) -> Result<u8, String> {
    match driven {
        None => {
            eprintln!("mcp-conformance check: interrupted; the trace holds the session so far");
            return Ok(EXIT_INTERRUPTED);
        }
        Some(Err(Ended::Unsupported(revision))) => {
            return Err(format!(
                "the server negotiated protocol revision {revision:?}, which this build does \
                 not judge (it judges {LEGACY} and {MODERN}); the session was closed, as a \
                 client that does not speak a revision closes it"
            ));
        }
        Some(Err(Ended::Lost(failure))) if summary.revision.is_none() => {
            return Err(format!("no session was established: {failure}"));
        }
        Some(Err(Ended::Lost(failure))) => {
            eprintln!(
                "mcp-conformance check: the session ended early ({failure}); the report \
                 below judges what was recorded"
            );
        }
        Some(Ok(()) | Err(Ended::Bound)) => {}
    }
    let report = args.report.clone().viewed_for(Party::Server);
    Ok(validator::judge(
        &[trace.display().to_string()],
        &report,
        &validator::JudgingArgs::default(),
    ))
}

/// Establishes the session in the era `version` asks for and exercises it.
async fn drive(
    wire: &mut Wire,
    subject: &Subject,
    trace: &Path,
    version: SpecVersion,
    plan: &Plan,
    summary: &mut Summary,
) -> Result<(), Ended> {
    let modern = !matches!(version, SpecVersion::Legacy);
    let mut session = Session {
        wire,
        requests: Requests::new(if modern { Era::Modern } else { Era::Legacy }),
        plan,
        summary,
    };
    let capabilities = if modern {
        match session.discover().await {
            Ok(Some(capabilities)) => {
                session.summary.negotiation = "the server answered `server/discover`".into();
                capabilities
            }
            Ok(None) | Err(Ended::Lost(_)) if matches!(version, SpecVersion::Modern) => {
                return Err(Ended::Lost(Failure::Transport(format!(
                    "the server did not answer `server/discover` as a {MODERN} server does \
                     (--spec-version {MODERN} asks for that revision alone)"
                ))));
            }
            Ok(None) => {
                session.summary.negotiation = format!(
                    "`server/discover` was not answered as a {MODERN} server answers it, so \
                     the session fell back to `initialize`"
                );
                session.initialize().await?
            }
            Err(Ended::Lost(Failure::Closed)) if matches!(subject, Subject::Command(..)) => {
                // The probe ended the server: a legacy server may treat a
                // request before `initialize` that way. Start it again and
                // open with the handshake; the trace starts over with it.
                let restarted = connect(subject, trace, true).await.map_err(|error| {
                    Ended::Lost(Failure::Transport(format!(
                        "restarting the server: {error}"
                    )))
                })?;
                let ended = std::mem::replace(session.wire, restarted);
                let _ = ended.close().await;
                session.requests = Requests::new(Era::Legacy);
                session.summary.requests = 0;
                session.summary.unanswered.clear();
                session.summary.negotiation = "the server exited on the `server/discover` \
                     probe, so it was started again and opened with `initialize`"
                    .into();
                session.initialize().await?
            }
            Err(Ended::Lost(Failure::Closed)) => {
                // Over HTTP a stream that ends unanswered is one exchange
                // lost, not the server: the endpoint is still there.
                session.summary.negotiation = "the stream answering `server/discover` ended \
                     without an answer, so the session fell back to `initialize`"
                    .into();
                session.initialize().await?
            }
            Err(ended) => return Err(ended),
        }
    } else {
        session.summary.negotiation = format!("--spec-version {LEGACY}: opened with `initialize`");
        session.initialize().await?
    };
    session.exercise(&capabilities).await
}

async fn connect(subject: &Subject, trace: &Path, force: bool) -> Result<Wire, String> {
    let target = output::check(trace, force)?;
    match subject {
        Subject::Url(url, extra) => {
            let (http, _) = wire::http::Http::start(url, extra.clone(), &target, force).await?;
            Ok(Wire::Http(Box::new(http)))
        }
        Subject::Command(program, arguments) => {
            let (stdio, _) = wire::stdio::Stdio::start(program, arguments, &target, force)?;
            Ok(Wire::Stdio(Box::new(stdio)))
        }
    }
}

fn subject(args: &CheckArgs) -> Result<Subject, String> {
    if let Some(url) = &args.url {
        let mut extra = Vec::new();
        for header in &args.header {
            let (name, value) = header
                .split_once(':')
                .ok_or("--header takes `Name: value`")?;
            let name = HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| "--header: not a header name")?;
            // The value is never repeated: it is usually a credential.
            let mut value =
                HeaderValue::from_str(value.trim()).map_err(|_| "--header: not a header value")?;
            value.set_sensitive(true);
            extra.push((name, value));
        }
        return Ok(Subject::Url(url.clone(), extra));
    }
    if !args.header.is_empty() {
        return Err("--header applies to --url; a stdio server reads its environment".to_owned());
    }
    let mut words: Vec<OsString> = args.command.as_ref().map_or_else(
        || args.server.clone(),
        |command| command.split_whitespace().map(OsString::from).collect(),
    );
    if words.is_empty() {
        return Err("the server command is empty".to_owned());
    }
    let program = words.remove(0);
    Ok(Subject::Command(program, words))
}

/// The subject as the summary names it; a URL without its query, which can
/// carry a key.
fn shown(args: &CheckArgs) -> String {
    if let Some(url) = &args.url {
        return url.split(['?', '#']).next().unwrap_or_default().to_owned();
    }
    args.command.clone().unwrap_or_else(|| {
        args.server
            .iter()
            .map(|word| word.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// A trace path in a new private directory under the system's temporary one.
fn fresh_trace() -> Result<PathBuf, String> {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.subsec_nanos());
    let dir = std::env::temp_dir().join(format!(
        "mcp-conformance-check-{}-{unique}",
        std::process::id()
    ));
    private_dir()
        .create(&dir)
        .map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
    Ok(dir.join("session.jsonl"))
}

/// A builder for a directory only its owner can enter (mode `0700` on Unix;
/// elsewhere the system's temporary directory is already the user's own).
///
/// One function with the Unix step inside it, not one function per
/// platform: a body compiled on no machine that tests it is a body no test
/// can hold.
fn private_dir() -> std::fs::DirBuilder {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder
}
