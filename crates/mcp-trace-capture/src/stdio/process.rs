// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The wrapped server as a process: started in a process group of its own (Unix),
//! so a request to stop reaches every process the server command started — a
//! launcher such as `npx`, `uv run` or `sh -c` and the real server behind it — and
//! a kill leaves none of them holding the session's pipes.

use std::ffi::{OsStr, OsString};
use std::io;
use std::process::ExitStatus;

use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::signals::Stop;

/// A running server and its end of the pipes.
#[derive(Debug)]
pub(super) struct ServerProcess {
    child: Child,
    /// The process group, named by the leader's pid (it was started as leader).
    #[cfg(unix)]
    group: Option<nix::unistd::Pid>,
}

/// Starts the server with piped stdin/stdout and inherited stderr.
pub(super) fn spawn(
    program: &OsStr,
    args: &[OsString],
) -> io::Result<(ServerProcess, ChildStdin, ChildStdout)> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true);
    // Its own group: the terminal's Ctrl-C now reaches only the capture, which
    // relays it — one delivery, through one path, whoever sends the signal.
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("cannot start {}: {error}", program.to_string_lossy()),
        )
    })?;
    #[cfg(unix)]
    let group = child
        .id()
        .and_then(|id| i32::try_from(id).ok())
        .map(nix::unistd::Pid::from_raw);
    match (child.stdin.take(), child.stdout.take()) {
        (Some(stdin), Some(stdout)) => Ok((
            ServerProcess {
                child,
                #[cfg(unix)]
                group,
            },
            stdin,
            stdout,
        )),
        _ => Err(io::Error::other(
            "the server's stdio pipes were not created",
        )),
    }
}

impl ServerProcess {
    /// Waits for the server (the process the command started) to exit.
    pub(super) async fn wait(&mut self) -> io::Result<ExitStatus> {
        self.child.wait().await
    }

    /// Passes a request to stop on to every process in the server's group.
    /// Elsewhere there is no signal to relay, so the server is ended.
    #[cfg_attr(
        unix,
        allow(
            clippy::needless_pass_by_ref_mut,
            reason = "elsewhere the relay is a kill through the child handle"
        )
    )]
    pub(super) fn relay(&mut self, stop: Stop) {
        #[cfg(unix)]
        {
            let signal = match stop {
                Stop::Interrupt => nix::sys::signal::Signal::SIGINT,
                Stop::Terminate => nix::sys::signal::Signal::SIGTERM,
                Stop::Hangup => nix::sys::signal::Signal::SIGHUP,
            };
            self.signal_group(signal);
        }
        #[cfg(not(unix))]
        {
            let _ = stop;
            self.child.start_kill().ok();
        }
    }

    /// Ends every process in the server's group now.
    pub(super) fn kill(&mut self) {
        #[cfg(unix)]
        self.signal_group(nix::sys::signal::Signal::SIGKILL);
        // The leader too, through the handle — on Unix already covered by the
        // group, unless it has been reaped (then this is a harmless error).
        self.child.start_kill().ok();
    }

    #[cfg(unix)]
    fn signal_group(&self, signal: nix::sys::signal::Signal) {
        if let Some(group) = self.group {
            // ESRCH — the group is already empty — is the outcome wanted.
            let _ = nix::sys::signal::killpg(group, signal);
        }
    }
}
