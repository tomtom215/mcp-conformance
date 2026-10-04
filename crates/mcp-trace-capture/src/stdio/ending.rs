// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How a wrapped session ends, and the bound on every wait while it does.
//!
//! Three things can end a session: the server exits, the client closes its end,
//! or this process is asked to stop. In the last case the request is relayed to
//! the server's process group and the server has [`STOP_GRACE`] to exit before it
//! is killed; a second request skips the rest of the grace period. No wait after a
//! request to stop is unbounded, so a server (or a process it left behind) that
//! ignores the signal cannot keep the capture running.

use std::io;
use std::process::ExitStatus;
use std::time::Duration;

use tokio::task::JoinHandle;

use super::process::ServerProcess;
use crate::signals::{STOP_GRACE, Signals, Stop};

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
    /// The request to stop that ended the session, if any.
    pub(super) stop: Option<Stop>,
    /// A second request cut the grace period short; no further wait is wanted.
    pub(super) hurried: bool,
}

type Relay = JoinHandle<io::Result<()>>;

/// Waits for the session to end.
pub(super) async fn wait_for_end(
    process: &mut ServerProcess,
    client: &mut Relay,
    signals: &mut Signals,
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
                    let (status, hurried) = stop_server(process, signals, stop).await?;
                    (status, Some(stop), hurried)
                }
            };
            Ok(End { status, client_closed, client_finished: true, stop, hurried })
        }
        stop = signals.recv() => {
            let (status, hurried) = stop_server(process, signals, stop).await?;
            Ok(End {
                status,
                client_closed: false,
                client_finished: false,
                stop: Some(stop),
                hurried,
            })
        }
    }
}

/// Relays `stop` to the server's group and waits for the server to exit: up to
/// [`STOP_GRACE`], or until a second request, then kills the group. Returns the
/// status, and whether the wait was cut short.
async fn stop_server(
    process: &mut ServerProcess,
    signals: &mut Signals,
    stop: Stop,
) -> io::Result<(ExitStatus, bool)> {
    process.relay(stop);
    let hurried = tokio::select! {
        status = process.wait() => return Ok((status?, false)),
        () = tokio::time::sleep(STOP_GRACE) => false,
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
/// stop arriving during it is relayed, as during the session. Once one has been
/// made, the group gets [`STOP_GRACE`] more (none after a second request), is
/// killed, and the relay is abandoned shortly after if something outside the
/// group still holds the pipe. Returns a request made during this wait.
pub(super) async fn drain(
    relay: &mut Relay,
    process: &mut ServerProcess,
    signals: &mut Signals,
    end: &End,
) -> Option<Stop> {
    let mut late = None;
    if end.stop.is_none() {
        tokio::select! {
            _ = &mut *relay => return None,
            stop = signals.recv() => {
                process.relay(stop);
                late = Some(stop);
            }
        }
    }
    if !end.hurried {
        tokio::select! {
            _ = &mut *relay => return late,
            () = tokio::time::sleep(STOP_GRACE) => {}
            _ = signals.recv() => {}
        }
    }
    process.kill();
    if tokio::time::timeout(AFTER_KILL, &mut *relay).await.is_err() {
        relay.abort();
    }
    late
}
