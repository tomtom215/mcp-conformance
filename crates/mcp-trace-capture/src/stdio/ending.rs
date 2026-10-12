// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How a wrapped session ends, and the bound on every wait while it does.
//!
//! Three things can end a session: the server exits, the client closes its end,
//! or this process is asked to stop. In the last case the trace is closed at
//! once — before any wait, so a wrapper killed during the grace period still
//! leaves a closed trace — the request is relayed to the server's process group,
//! and the group has [`SERVER_STOP_GRACE`] from the request to be gone before it
//! is killed; a second request skips the rest of that time. One deadline covers
//! the server and anything still holding its stdout, so no wait after a request
//! to stop is unbounded, and the whole stop ends inside the window the official
//! SDKs' stdio clients allow between `SIGTERM` and `SIGKILL`.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use mcp_conformance_core::trace::{Direction, LifecycleEvent, TransportKind};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::process::ServerProcess;
use crate::recorder::Recorder;
use crate::signals::{SERVER_STOP_GRACE, Signals, Stop};

/// After the server's group has been killed, how long its stdout may take to
/// reach end of stream before the relay is abandoned. Only a process that left the
/// group (a daemon that called `setsid`) can still hold the pipe by then.
const AFTER_KILL: Duration = Duration::from_millis(500);

/// How the session ended.
#[derive(Debug)]
pub(super) struct End {
    /// The server's exit status.
    pub(super) status: ExitStatus,
    /// The client closed its end (end of stream on stdin) before the server exited.
    pub(super) client_closed: bool,
    /// The client's relay has finished, so it need not be cancelled.
    pub(super) client_finished: bool,
    /// The request to stop that ended the session, if any, and when the
    /// server's processes must be gone by.
    pub(super) stop: Option<(Stop, Instant)>,
    /// A second request cut the grace period short; no further wait is wanted.
    pub(super) hurried: bool,
}

type Relay = JoinHandle<io::Result<()>>;

/// Waits for the session to end.
pub(super) async fn wait_for_end(
    process: &mut ServerProcess,
    client: &mut Relay,
    signals: &mut Signals,
    recorder: &Recorder,
) -> io::Result<End> {
    tokio::select! {
        status = process.wait() => Ok(End {
            status: status?,
            client_closed: false,
            client_finished: false,
            stop: None,
            hurried: false,
        }),
        joined = &mut *client => {
            // Only a clean end of stream is the client closing. An error is almost
            // always a write to a server that has gone (its stdin closed with it),
            // and reading it as the client closing made the trace's closing
            // direction depend on which of the two the runtime noticed first.
            let client_closed = matches!(joined, Ok(Ok(())));
            let (status, stop, hurried) = tokio::select! {
                status = process.wait() => (status?, None, false),
                stop = signals.recv() => {
                    let deadline = begin_stop(process, recorder, stop);
                    let (status, hurried) = stop_server(process, signals, deadline).await?;
                    (status, Some((stop, deadline)), hurried)
                }
            };
            Ok(End { status, client_closed, client_finished: true, stop, hurried })
        }
        stop = signals.recv() => {
            let deadline = begin_stop(process, recorder, stop);
            let (status, hurried) = stop_server(process, signals, deadline).await?;
            Ok(End {
                status,
                client_closed: false,
                client_finished: false,
                stop: Some((stop, deadline)),
                hurried,
            })
        }
    }
}

/// Closes the trace, then relays `stop` to the server's group; returns when the
/// group must be gone by.
///
/// The close comes first: the client that asked for the stop may kill this
/// process at any moment from here on (the official SDKs do, 2 seconds after
/// `SIGTERM`), and an event written later might never be written. What the
/// session carries after it is still relayed, and counted as not recorded.
fn begin_stop(process: &mut ServerProcess, recorder: &Recorder, stop: Stop) -> Instant {
    let _ = recorder.close(
        Direction::ClientToServer,
        TransportKind::Stdio,
        LifecycleEvent::TransportClose,
    );
    process.relay(stop);
    Instant::now() + SERVER_STOP_GRACE
}

/// Waits for the server to exit: until `deadline`, or until a second request,
/// then kills the group. Returns the status, and whether the wait was cut short.
async fn stop_server(
    process: &mut ServerProcess,
    signals: &mut Signals,
    deadline: Instant,
) -> io::Result<(ExitStatus, bool)> {
    let hurried = tokio::select! {
        status = process.wait() => return Ok((status?, false)),
        () = tokio::time::sleep_until(deadline) => false,
        _ = signals.recv() => true,
    };
    process.kill();
    Ok((process.wait().await?, hurried))
}

/// Waits for the server's output relay to reach end of stream, which happens when
/// the last process holding the server's stdout exits.
///
/// With no request to stop, that wait is as long as the server's processes run —
/// a process the server left behind is part of the session — but a request to
/// stop arriving during it closes the trace and is relayed, as during the
/// session. Once one has been made, the group has until the same deadline the
/// server had (none after a second request), is killed, and the relay is
/// abandoned shortly after if something outside the group still holds the pipe.
/// Returns a request made during this wait.
pub(super) async fn drain(
    relay: &mut Relay,
    process: &mut ServerProcess,
    signals: &mut Signals,
    end: &End,
    recorder: &Recorder,
) -> Option<Stop> {
    let mut late = None;
    let deadline = if let Some((_, deadline)) = end.stop {
        deadline
    } else {
        tokio::select! {
            _ = &mut *relay => return None,
            stop = signals.recv() => {
                late = Some(stop);
                begin_stop(process, recorder, stop)
            }
        }
    };
    if !end.hurried {
        tokio::select! {
            _ = &mut *relay => return late,
            () = tokio::time::sleep_until(deadline) => {}
            _ = signals.recv() => {}
        }
    }
    process.kill();
    if tokio::time::timeout(AFTER_KILL, &mut *relay).await.is_err() {
        relay.abort();
    }
    late
}
