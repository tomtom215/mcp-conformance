// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The requests to stop a capture answers: `SIGINT`, `SIGTERM`, `SIGHUP` and
//! `SIGQUIT` on Unix, Ctrl-C elsewhere.
//!
//! A capture never lets one of these kill it outright: the stdio wrapper relays it
//! to the server and closes the trace once the server is gone; the HTTP proxy shuts
//! down within a bounded time and closes the trace. Every signal is delivered to
//! [`Signals::recv`], the first and every later one, so a second request can cut a
//! wait short — once installed, the handlers replace the default action for the
//! life of the process, and a capture that stopped listening would be one that
//! only `SIGKILL` could end.

use std::io;
use std::time::Duration;

/// How long the HTTP proxy's open connections get to finish after a request to
/// stop, before the capture ends them. A second request ends the wait at once.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

/// How long a stdio server's processes get to exit after a request to stop,
/// before the wrapper kills their process group. A second request ends the wait
/// at once.
///
/// Shorter than the 2 seconds the official SDKs' stdio clients (Python and
/// TypeScript) allow between `SIGTERM` and `SIGKILL`, so the wrapper has killed
/// the server and exited before that `SIGKILL` arrives. It reaches only the
/// wrapper (its pid, or its process group), never the server's own group: a
/// wrapper killed while still waiting would leave behind a server that ignores
/// `SIGTERM`. The server has already had the 2 seconds those clients wait after
/// closing its stdin.
pub const SERVER_STOP_GRACE: Duration = Duration::from_secs(1);

/// A request to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Stop {
    /// `SIGINT`, or Ctrl-C on any platform.
    Interrupt,
    /// `SIGTERM`.
    Terminate,
    /// `SIGHUP`: the terminal or the session that started the capture went away.
    Hangup,
    /// `SIGQUIT` (a terminal's Ctrl-\\). Its default action ends a process at
    /// once, which would leave the server — in a process group of its own, out
    /// of the terminal's reach — running, and the trace unclosed.
    Quit,
}

impl Stop {
    /// The signal's Unix number; a shell reports a process it ended as exiting
    /// with `128 +` this.
    #[must_use]
    pub const fn number(self) -> i32 {
        match self {
            Self::Interrupt => 2,
            Self::Terminate => 15,
            Self::Hangup => 1,
            Self::Quit => 3,
        }
    }

    /// The signal's name, for messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Interrupt => "SIGINT",
            Self::Terminate => "SIGTERM",
            Self::Hangup => "SIGHUP",
            Self::Quit => "SIGQUIT",
        }
    }
}

/// The installed handlers.
#[derive(Debug)]
pub struct Signals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
    #[cfg(unix)]
    hangup: tokio::signal::unix::Signal,
    #[cfg(unix)]
    quit: tokio::signal::unix::Signal,
}

impl Signals {
    /// Installs the handlers. A signal arriving any time after this call is kept
    /// for the next [`Signals::recv`], not lost and not fatal — the difference
    /// between a capture that finishes its trace and one the default handler kills
    /// mid-announcement.
    ///
    /// Must be called within a Tokio runtime.
    ///
    /// # Errors
    ///
    /// A handler could not be registered.
    #[cfg_attr(
        not(unix),
        allow(
            clippy::missing_const_for_fn,
            clippy::unnecessary_wraps,
            reason = "only the Unix handlers can fail to register"
        )
    )]
    pub fn install() -> io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
                hangup: signal(SignalKind::hangup())?,
                quit: signal(SignalKind::quit())?,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    /// The next request to stop.
    pub async fn recv(&mut self) -> Stop {
        #[cfg(unix)]
        {
            // A stream that ended can deliver nothing more; it must not win the
            // race against the others.
            let interrupt = async {
                if self.interrupt.recv().await.is_none() {
                    std::future::pending::<()>().await;
                }
            };
            let terminate = async {
                if self.terminate.recv().await.is_none() {
                    std::future::pending::<()>().await;
                }
            };
            let hangup = async {
                if self.hangup.recv().await.is_none() {
                    std::future::pending::<()>().await;
                }
            };
            let quit = async {
                if self.quit.recv().await.is_none() {
                    std::future::pending::<()>().await;
                }
            };
            tokio::select! {
                () = interrupt => Stop::Interrupt,
                () = terminate => Stop::Terminate,
                () = hangup => Stop::Hangup,
                () = quit => Stop::Quit,
            }
        }
        #[cfg(not(unix))]
        {
            if tokio::signal::ctrl_c().await.is_err() {
                std::future::pending::<()>().await;
            }
            Stop::Interrupt
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Stop;

    #[test]
    fn numbers_and_names_are_the_unix_ones() {
        assert_eq!(
            [Stop::Hangup, Stop::Interrupt, Stop::Quit, Stop::Terminate].map(Stop::number),
            [1, 2, 3, 15]
        );
        assert_eq!(Stop::Quit.name(), "SIGQUIT");
        assert_eq!(Stop::Terminate.name(), "SIGTERM");
        assert_eq!(Stop::Interrupt.name(), "SIGINT");
        assert_eq!(Stop::Hangup.name(), "SIGHUP");
    }
}
