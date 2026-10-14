// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which trace an exchange is recorded in, and the place a request whose trace
//! is not yet known keeps in the one it would otherwise join.

use std::sync::Arc;

use mcp_conformance_core::trace::{Direction, EventBody};

use super::writer::Command;

/// The trace an exchange belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum TraceKey {
    /// A session the server assigned, by its `Mcp-Session-Id`.
    Session(String),
    /// Exchanges naming no session ([`STATELESS_TRACE`](super::STATELESS_TRACE)).
    Stateless,
    /// Exchanges naming a session the server never assigned
    /// ([`UNASSIGNED_TRACE`](super::UNASSIGNED_TRACE)).
    Unassigned,
}

impl TraceKey {
    /// The key's part of the trace file name.
    pub(super) fn label(&self) -> String {
        match self {
            Self::Session(id) => file_label(id),
            Self::Stateless => super::STATELESS_TRACE.to_owned(),
            Self::Unassigned => super::UNASSIGNED_TRACE.to_owned(),
        }
    }
}

/// Identity of one trace file. The writer names the file when it first writes
/// to it, so a trace that only ever had a place reserved in it and given up
/// leaves no file and spends no ordinal.
#[derive(Debug)]
pub(super) struct SessionFile {
    /// Unique for the run; the writer keys the file's state by it.
    pub(super) id: u64,
    /// The file name's descriptive part, safe for a file name.
    pub(super) label: String,
}

/// A place reserved in a trace for a request observed before the trace it
/// belongs in is known — one naming no session, which joins the `stateless`
/// trace unless its response assigns a session.
///
/// Settle it with [`Reservation::settle`]. Dropped unsettled — the exchange
/// was abandoned mid-flight — it gives the place up, so the records queued
/// behind it in that trace are written rather than held for the rest of the
/// run.
pub(super) struct Reservation {
    pub(super) sender: tokio::sync::mpsc::Sender<Command>,
    pub(super) file: Arc<SessionFile>,
    pub(super) place: u64,
    pub(super) settled: bool,
}

impl Reservation {
    /// Writes `events` in the reserved place; none gives the place up.
    pub(super) async fn settle(mut self, events: Vec<(Direction, EventBody)>) {
        let place = self.place;
        if self
            .sender
            .send(Command::Fill { place, events })
            .await
            .is_err()
        {
            eprintln!("mcp-everything-server: tap writer gone; event dropped");
        }
        self.settled = true;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let release = Command::Fill {
            place: self.place,
            events: Vec::new(),
        };
        // Drop cannot await: a full channel hands the release to a task, which
        // still delivers it after the reservation (one channel, one order).
        if let Err(tokio::sync::mpsc::error::TrySendError::Full(release)) =
            self.sender.try_send(release)
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let sender = self.sender.clone();
            runtime.spawn(async move {
                let _ = sender.send(release).await;
            });
        }
    }
}

/// `session_id` as part of a file name: ASCII letters, digits, `-`, `_` and
/// `.` kept, anything else `_`, at most 64 characters.
///
/// The id comes from a header, and a client can name any session: written
/// verbatim, `../x` would leave the trace directory and `a:b` would open an
/// NTFS alternate data stream. The ordinal before it keeps names unique, so
/// two ids that sanitize alike still get two files, and a name that is never
/// `.` or `..`.
pub(super) fn file_label(session_id: &str) -> String {
    session_id
        .chars()
        .take(64)
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '_'
            }
        })
        .collect()
}
