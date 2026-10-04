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

use std::ffi::{OsStr, OsString};
use std::io;
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};
use tokio::process::{ChildStdin, ChildStdout};

use crate::framing::{Line, LineSplitter, parse_json};
use crate::recorder::{NotRecorded, Recorder};
use crate::signals::{Signals, Stop};
use process::ServerProcess;

mod ending;
mod process;

/// What one direction carried that was not a JSON message.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unrecorded {
    /// Lines that were not JSON (including blank lines). Each is recorded as a
    /// message whose payload is the line as a JSON string, so the validator judges
    /// it as what it is — something on the stream that is not a valid MCP message
    /// (`TRAN-004`/`TRAN-005`, `TRAN-117`) — rather than the trace hiding it.
    pub not_json: u64,
    /// Lines longer than the message limit, forwarded but not recorded.
    pub oversized: u64,
}

/// A pump's running [`Unrecorded`] counts, readable while it runs — so what it left
/// out is still known when the session ends by cancelling it.
#[derive(Debug, Default)]
struct Tally {
    not_json: AtomicU64,
    oversized: AtomicU64,
}

impl Tally {
    fn snapshot(&self) -> Unrecorded {
        Unrecorded {
            not_json: self.not_json.load(Ordering::Relaxed),
            oversized: self.oversized.load(Ordering::Relaxed),
        }
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
    from: R,
    to: W,
    max_message: usize,
) -> io::Result<Unrecorded>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let tally = Tally::default();
    pump_counted(recorder, direction, from, to, max_message, &tally).await?;
    Ok(tally.snapshot())
}

/// [`pump`], counting into `tally` as it goes.
async fn pump_counted<R, W>(
    recorder: &Recorder,
    direction: Direction,
    mut from: R,
    mut to: W,
    max_message: usize,
    tally: &Tally,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut splitter = LineSplitter::new(max_message);
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let read = from.read(&mut buffer).await?;
        if read == 0 {
            if let Some(line) = splitter.finish() {
                record_line(recorder, direction, line, tally);
            }
            to.shutdown().await.ok();
            return Ok(());
        }
        let chunk = &buffer[..read];
        for line in splitter.push(chunk) {
            record_line(recorder, direction, line, tally);
        }
        to.write_all(chunk).await?;
        to.flush().await?;
    }
}

fn record_line(recorder: &Recorder, direction: Direction, line: Line, tally: &Tally) {
    match line {
        Line::Complete(bytes) => {
            let parsed = parse_json(&bytes);
            let is_json = parsed.is_some();
            // A line that is not JSON is still what crossed the wire: record it as a
            // string (lossy UTF-8) so the validator can judge it.
            let payload = parsed.unwrap_or_else(|| {
                serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
            });
            let outcome = recorder.record(
                direction,
                TransportKind::Stdio,
                EventBody::Message { payload },
            );
            if outcome == Err(NotRecorded::TooLong) {
                tally.oversized.fetch_add(1, Ordering::Relaxed);
            } else if !is_json {
                tally.not_json.fetch_add(1, Ordering::Relaxed);
            }
        }
        Line::Oversized => {
            tally.oversized.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// [`pump_counted`] as a task of its own.
fn spawn_pump<R, W>(
    recorder: &Arc<Recorder>,
    direction: Direction,
    from: R,
    to: W,
    max_message: usize,
    tally: &Arc<Tally>,
) -> tokio::task::JoinHandle<io::Result<()>>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (recorder, tally) = (Arc::clone(recorder), Arc::clone(tally));
    tokio::spawn(
        async move { pump_counted(&recorder, direction, from, to, max_message, &tally).await },
    )
}

/// How a wrapped session ended.
#[derive(Debug)]
#[non_exhaustive]
pub struct Outcome {
    /// The server's exit status.
    pub status: ExitStatus,
    /// What the client sent that was not a JSON message.
    pub client: Unrecorded,
    /// What the server sent that was not a JSON message.
    pub server: Unrecorded,
    /// The request to stop that ended the session, if one did; it was relayed to
    /// the server's process group.
    pub stop: Option<Stop>,
}

/// A started server, not yet relayed to: [`spawn`] it before creating anything
/// that depends on its having started (the trace file), then [`run_server`].
#[derive(Debug)]
pub struct Server {
    process: ServerProcess,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl Server {
    /// Ends the server and every process in its group, without a session — for a
    /// caller that cannot go on after starting it.
    pub async fn kill(mut self) {
        self.process.kill();
        let _ = self.process.wait().await;
    }
}

/// Starts `program` with `args` as the server: piped stdin and stdout, inherited
/// stderr, and (on Unix) a process group of its own. Must be called within a
/// Tokio runtime.
///
/// # Errors
///
/// The server could not be started.
pub fn spawn(program: &OsStr, args: &[OsString]) -> io::Result<Server> {
    let (process, stdin, stdout) = process::spawn(program, args)?;
    Ok(Server {
        process,
        stdin,
        stdout,
    })
}

/// Runs `program` with `args` as the server, relaying this process's stdin and
/// stdout to it and recording the session: [`Signals::install`], [`spawn`] and
/// [`run_server`] in one call.
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
    // Installed before the server starts, so a signal that arrives at any point
    // after this is relayed rather than killing this process outright.
    let signals = Signals::install()?;
    let server = spawn(&program, &args)?;
    run_server(recorder, server, signals, max_message).await
}

/// Relays this process's stdin and stdout to `server` and records the session.
///
/// The session ends when the server exits. If the client closes its end first, the
/// server's stdin is closed and the wrapper waits for the server to exit, as the
/// stdio transport's shutdown sequence expects. A request to stop (`signals`) is
/// relayed to the server's whole process group; a server still running
/// [`STOP_GRACE`](crate::STOP_GRACE) later, or at a second request, is killed. The
/// trace's last event closes it either way.
///
/// # Errors
///
/// Waiting for the server failed.
pub async fn run_server(
    recorder: Arc<Recorder>,
    server: Server,
    mut signals: Signals,
    max_message: usize,
) -> io::Result<Outcome> {
    let Server {
        mut process,
        stdin: child_stdin,
        stdout: child_stdout,
    } = server;
    let _ = recorder.record(
        Direction::ClientToServer,
        TransportKind::Stdio,
        EventBody::Lifecycle {
            event: LifecycleEvent::TransportOpen,
        },
    );

    let (client_tally, server_tally) = (Arc::new(Tally::default()), Arc::new(Tally::default()));
    let mut client = spawn_pump(
        &recorder,
        Direction::ClientToServer,
        tokio::io::stdin(),
        child_stdin,
        max_message,
        &client_tally,
    );
    let mut server_pump = spawn_pump(
        &recorder,
        Direction::ServerToClient,
        child_stdout,
        tokio::io::stdout(),
        max_message,
        &server_tally,
    );

    let end = ending::wait_for_end(&mut process, &mut client, &mut signals).await?;
    if !end.client_finished {
        // The client's pump is blocked on this process's stdin, which the client
        // may hold open forever. Stop it, and wait until it has stopped, so nothing
        // it reads after the server has gone is recorded behind the closing event
        // below — and a library caller is not left with a task reading its stdin.
        client.abort();
        let _ = client.await;
    }
    // The server's stdout closes when the last process holding it exits; a
    // request to stop bounds that wait.
    let late_stop = ending::drain(&mut server_pump, &mut process, &mut signals, &end).await;
    let stop = end.stop.or(late_stop);
    let closer = if end.client_closed || stop.is_some() {
        Direction::ClientToServer
    } else {
        Direction::ServerToClient
    };
    let event = if end.status.success() {
        LifecycleEvent::TransportClose
    } else {
        LifecycleEvent::TransportAbort
    };
    // The last word: nothing a straggling task records after this reaches the trace.
    let _ = recorder.close(closer, TransportKind::Stdio, event);
    Ok(Outcome {
        status: end.status,
        client: client_tally.snapshot(),
        server: server_tally.snapshot(),
        stop,
    })
}

#[cfg(test)]
mod tests;
