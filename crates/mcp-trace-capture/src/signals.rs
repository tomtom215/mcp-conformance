// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The requests to stop a capture answers: `SIGINT`, `SIGTERM` and `SIGHUP` on Unix,
//! Ctrl-C elsewhere.
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

/// How long a server (stdio) or the open connections (HTTP) get to finish after
/// a request to stop, before the capture ends them. A second request ends the
/// wait at once.
pub const STOP_GRACE: Duration = Duration::from_secs(3);

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
        }
    }

    /// The signal's name, for messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Interrupt => "SIGINT",
            Self::Terminate => "SIGTERM",
            Self::Hangup => "SIGHUP",
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
            tokio::select! {
                () = interrupt => Stop::Interrupt,
                () = terminate => Stop::Terminate,
                () = hangup => Stop::Hangup,
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
            [Stop::Hangup, Stop::Interrupt, Stop::Terminate].map(Stop::number),
            [1, 2, 15]
        );
        assert_eq!(Stop::Terminate.name(), "SIGTERM");
        assert_eq!(Stop::Interrupt.name(), "SIGINT");
        assert_eq!(Stop::Hangup.name(), "SIGHUP");
    }
}
