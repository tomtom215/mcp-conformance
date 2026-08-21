// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Bounded reads for the binary tests.
//!
//! Every read these tests perform waits on output the server under test is
//! supposed to produce, which makes every one of them a hang the moment it
//! stops producing it. `cargo test` has no per-test timeout, so the failure
//! mode is not a red test — it is a job that spends its whole budget blocked on
//! a line that is never coming, and a maintainer reading "still running" with
//! nothing to go on.
//!
//! The mutation gate made it concrete. Flipping
//! `HttpSecurityPolicy::validates_nothing` to `false` should have failed
//! `disabling_host_validation_warns_after_the_readiness_line` in milliseconds —
//! the warning is simply absent. Instead the test blocked on `read_line`
//! forever and cargo-mutants could only report it as a 183-second timeout,
//! which is the one outcome that says nothing about whether a test noticed.
//!
//! `xtask::conformance::await_readiness_line` reads a child's readiness line on
//! a worker thread bounded by a timeout, for exactly this reason. This is that
//! shape, shared across the test binaries that need it.

// Each test binary includes this module and uses the part of it that it needs;
// the unused remainder is not dead code in the workspace, only in that binary.
#![allow(dead_code)]
// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::io::{BufRead as _, BufReader};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

/// How long to wait for a line the server owes. Generous, and matching
/// `xtask::conformance::READINESS_TIMEOUT`: the binary is already built, so
/// startup is socket-bind plus runtime init.
pub(crate) const LINE_TIMEOUT: Duration = Duration::from_secs(30);

/// Lines from a child's pipe, read on a worker thread so a line that never
/// arrives **fails** the test instead of hanging it.
///
/// Draining in the background is a second benefit: a chatty server can never
/// block on a full pipe while the test is busy elsewhere.
pub(crate) struct Lines(Receiver<String>);

impl Lines {
    pub(crate) fn from(source: impl std::io::Read + Send + 'static) -> Self {
        let (sender, receiver) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(source);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    // The receiver is gone once the test ends: stop rather than
                    // spin reading into a closed channel.
                    Ok(_) => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        Self(receiver)
    }

    /// The next line, or a panic naming what was expected and never came.
    ///
    /// Both failure shapes land here: a silent server times out, and one that
    /// closed its pipe (crashed, or exited) disconnects. The message says
    /// which, because they call for different investigations.
    #[track_caller]
    pub(crate) fn next(&self, what: &str) -> String {
        self.0
            .recv_timeout(LINE_TIMEOUT)
            .unwrap_or_else(|error| panic!("no {what} within {LINE_TIMEOUT:?}: {error}"))
    }

    /// Everything still to come, until the pipe closes.
    ///
    /// Call it once the child is dead, so "until the pipe closes" is a bounded
    /// wait rather than a promise.
    pub(crate) fn rest(&self) -> String {
        let mut out = String::new();
        while let Ok(line) = self.0.recv_timeout(LINE_TIMEOUT) {
            out.push_str(&line);
        }
        out
    }
}

/// A TCP read timeout, so an HTTP exchange with a wedged server fails instead
/// of blocking. Same rule as [`Lines`], for the transport these tests speak
/// when they are not reading pipes.
pub(crate) fn bound_reads(stream: &std::net::TcpStream) {
    stream
        .set_read_timeout(Some(LINE_TIMEOUT))
        .expect("a loopback stream accepts a read timeout");
}
