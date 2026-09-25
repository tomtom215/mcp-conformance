// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The stdio wrapper: run the server as a child, sit between it and the client, and
//! record every message.
//!
//! An MCP client that launches `mcp-trace-capture stdio -- <server>` in place of
//! `<server>` sees the server's bytes, unchanged; the server sees the client's. The
//! server's stderr is inherited, untouched.
//!
//! **Ordering.** Each message is recorded before the newline that completes it is
//! forwarded. The peer cannot act on a message it has not received in full, so a
//! response can never be recorded ahead of the request it answers — the `seq` order
//! is causal by construction, not by the luck of task scheduling.

use std::ffi::OsString;
use std::io;
use std::process::ExitStatus;
use std::sync::Arc;

use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::process::Command;

use crate::framing::{Line, LineSplitter, parse_json};
use crate::recorder::Recorder;

/// What one direction carried that the trace could not hold.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unrecorded {
    /// Lines that were not JSON (including blank lines). The stdio transport allows
    /// only messages on stdout, so on the server side each is itself a finding the
    /// trace format cannot yet represent.
    pub not_json: u64,
    /// Lines longer than the message limit.
    pub oversized: u64,
}

impl Unrecorded {
    /// Whether nothing was left out.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.not_json == 0 && self.oversized == 0
    }
}

/// Copies `from` to `to` until end of stream, recording each complete line as a
/// message event before forwarding the bytes that complete it.
///
/// # Errors
///
/// A read or write error on either side; everything recorded up to it stays recorded.
pub async fn pump<R, W>(
    recorder: &Recorder,
    direction: Direction,
    mut from: R,
    mut to: W,
    max_message: usize,
) -> io::Result<Unrecorded>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut splitter = LineSplitter::new(max_message);
    let mut unrecorded = Unrecorded::default();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = from.read(&mut buffer).await?;
        if read == 0 {
            if let Some(line) = splitter.finish() {
                record_line(recorder, direction, line, &mut unrecorded);
            }
            to.shutdown().await.ok();
            return Ok(unrecorded);
        }
        let chunk = &buffer[..read];
        for line in splitter.push(chunk) {
            record_line(recorder, direction, line, &mut unrecorded);
        }
        to.write_all(chunk).await?;
        to.flush().await?;
    }
}

fn record_line(recorder: &Recorder, direction: Direction, line: Line, unrecorded: &mut Unrecorded) {
    match line {
        Line::Complete(bytes) => match parse_json(&bytes) {
            Some(payload) => {
                recorder.record(
                    direction,
                    TransportKind::Stdio,
                    EventBody::Message { payload },
                );
            }
            None => unrecorded.not_json += 1,
        },
        Line::Oversized => unrecorded.oversized += 1,
    }
}

/// How a wrapped session ended.
#[derive(Debug)]
#[non_exhaustive]
pub struct Outcome {
    /// The server's exit status.
    pub status: ExitStatus,
    /// What the client sent that was not recorded.
    pub client: Unrecorded,
    /// What the server sent that was not recorded.
    pub server: Unrecorded,
}

/// Runs `program` with `args` as the server, relaying this process's stdin and
/// stdout to it and recording the session.
///
/// The session ends when the server exits. If the client closes its end first, the
/// server's stdin is closed and the wrapper waits for the server to exit, as the
/// stdio transport's shutdown sequence expects. `SIGINT`/`SIGTERM` (Ctrl-C on
/// Windows) are relayed by terminating the server, so it is never orphaned.
///
/// # Errors
///
/// The server could not be spawned, or waiting for it failed.
pub async fn run(
    recorder: Arc<Recorder>,
    program: OsString,
    args: Vec<OsString>,
    max_message: usize,
) -> io::Result<Outcome> {
    let (mut child, child_stdin, child_stdout) = spawn_server(&program, &args)?;
    lifecycle(
        &recorder,
        Direction::ClientToServer,
        LifecycleEvent::TransportOpen,
    );

    let mut client = tokio::spawn({
        let recorder = Arc::clone(&recorder);
        async move {
            pump(
                &recorder,
                Direction::ClientToServer,
                tokio::io::stdin(),
                child_stdin,
                max_message,
            )
            .await
        }
    });
    let server = tokio::spawn({
        let recorder = Arc::clone(&recorder);
        async move {
            pump(
                &recorder,
                Direction::ServerToClient,
                child_stdout,
                tokio::io::stdout(),
                max_message,
            )
            .await
        }
    });

    let (status, closed_by_client, client_unrecorded) =
        wait_for_end(&mut child, &mut client).await?;
    if !closed_by_client {
        // Blocked on this process's stdin, which the client may hold open forever.
        client.abort();
    }
    // The server's stdout closes at exit, so this drains and returns.
    let server_unrecorded = server.await.map_err(io::Error::other)?.unwrap_or_default();
    let closer = if closed_by_client {
        Direction::ClientToServer
    } else {
        Direction::ServerToClient
    };
    let ending = if status.success() {
        LifecycleEvent::TransportClose
    } else {
        LifecycleEvent::TransportAbort
    };
    lifecycle(&recorder, closer, ending);
    Ok(Outcome {
        status,
        client: client_unrecorded,
        server: server_unrecorded,
    })
}

fn lifecycle(recorder: &Recorder, direction: Direction, event: LifecycleEvent) {
    recorder.record(
        direction,
        TransportKind::Stdio,
        EventBody::Lifecycle { event },
    );
}

/// Starts the server with piped stdin/stdout and inherited stderr.
fn spawn_server(
    program: &OsString,
    args: &[OsString],
) -> io::Result<(
    tokio::process::Child,
    tokio::process::ChildStdin,
    tokio::process::ChildStdout,
)> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("cannot start {}: {error}", program.to_string_lossy()),
            )
        })?;
    match (child.stdin.take(), child.stdout.take()) {
        (Some(stdin), Some(stdout)) => Ok((child, stdin, stdout)),
        _ => Err(io::Error::other(
            "the server's stdio pipes were not created",
        )),
    }
}

/// Waits for the session to end: the server exits, the client closes its end (then
/// the server is given until it exits), or this process is asked to stop (then the
/// server is terminated). Returns the server's status, whether the client closed
/// first, and what the client sent that was not recorded.
async fn wait_for_end(
    child: &mut tokio::process::Child,
    client: &mut tokio::task::JoinHandle<io::Result<Unrecorded>>,
) -> io::Result<(ExitStatus, bool, Unrecorded)> {
    tokio::select! {
        status = child.wait() => Ok((status?, false, Unrecorded::default())),
        joined = client => {
            let unrecorded = joined.map_err(io::Error::other)?.unwrap_or_default();
            let status = tokio::select! {
                status = child.wait() => status?,
                () = terminated() => {
                    child.start_kill().ok();
                    child.wait().await?
                }
            };
            Ok((status, true, unrecorded))
        }
        () = terminated() => {
            child.start_kill().ok();
            Ok((child.wait().await?, false, Unrecorded::default()))
        }
    }
}

/// Resolves when this process is asked to stop.
async fn terminated() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut interrupt), Ok(mut terminate)) = (
            signal(SignalKind::interrupt()),
            signal(SignalKind::terminate()),
        ) else {
            return std::future::pending().await;
        };
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        if tokio::signal::ctrl_c().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests;
