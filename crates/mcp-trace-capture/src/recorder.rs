// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The trace writer: assigns `seq` in observation order and appends one event per
//! line.
//!
//! One lock covers both the counter and the write, so the order of `seq` values is
//! the order of lines in the file — the property the validator treats as the only
//! ordering authority. Every event is flushed before the lock is released: a capture
//! killed mid-session keeps everything it recorded, with at most one torn final line.
//!
//! A failed write does not stop the session. Recording is the tool's purpose, but a
//! proxy that tears down the client's session because a disk filled up has made its
//! failure the user's; instead the first error is reported on stderr, further events
//! are dropped, and [`Recorder::finish`] reports the trace as incomplete so the
//! binary can exit non-zero.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::sync::{Mutex, PoisonError};

use mcp_conformance_core::trace::{
    DEFAULT_MAX_LINE_BYTES, Direction, EventBody, LifecycleEvent, TraceEvent, TransportKind,
};

/// Appends trace events to a sink, one JSON object per line.
#[derive(Debug)]
pub struct Recorder {
    inner: Mutex<Inner>,
    max_line: usize,
}

/// Why [`Recorder::record`] wrote nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotRecorded {
    /// The event's line would exceed the recorder's line limit. Nothing was
    /// written and no `seq` was used; later events are unaffected.
    TooLong,
    /// The sink has failed; nothing more is written.
    SinkFailed,
    /// The trace has been closed ([`Recorder::close`]); its last event is written.
    Closed,
}

struct Inner {
    sessions: Sessions,
    next_seq: u64,
    sink: Box<dyn Write + Send>,
    failed: Option<io::Error>,
    dropped: u64,
    closed: bool,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("next_seq", &self.next_seq)
            .field("failed", &self.failed)
            .field("dropped", &self.dropped)
            .finish_non_exhaustive()
    }
}

/// How a capture ended, for the caller's exit code.
#[derive(Debug)]
#[non_exhaustive]
pub struct Summary {
    /// Events written.
    pub recorded: u64,
    /// Events lost after a write failed; zero for a complete trace.
    pub dropped: u64,
    /// The first write error, if any.
    pub error: Option<io::Error>,
    /// The sessions the recorded traffic showed.
    pub sessions: Sessions,
}

/// Evidence of how many sessions a trace holds. The validator judges a trace as
/// one session, so more than one of either count means its findings about ids
/// and ordering mix sessions together.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sessions {
    /// Client `initialize` requests seen (2025-11-25 and earlier start every
    /// session with one).
    pub initialize_requests: u64,
    /// Distinct `Mcp-Session-Id` values seen on HTTP requests or responses.
    pub session_ids: BTreeSet<String>,
}

impl Sessions {
    /// The number of sessions the evidence shows: the larger of the two counts.
    #[must_use]
    pub fn count(&self) -> u64 {
        let ids = u64::try_from(self.session_ids.len()).unwrap_or(u64::MAX);
        self.initialize_requests.max(ids)
    }

    fn observe(&mut self, direction: Direction, body: &EventBody) {
        match body {
            EventBody::Message { payload }
                if direction == Direction::ClientToServer
                    && payload.get("method").and_then(serde_json::Value::as_str)
                        == Some("initialize")
                    && payload.get("id").is_some() =>
            {
                self.initialize_requests += 1;
            }
            EventBody::Http { headers, .. } => {
                if let Some(id) = headers.get("mcp-session-id") {
                    self.session_ids.insert(id.clone());
                }
            }
            _ => {}
        }
    }
}

impl Summary {
    /// Whether every observed event reached the trace.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.error.is_none()
    }
}

impl Recorder {
    /// A recorder writing to `sink`, starting at `seq` 0, whose lines fit a
    /// reader's default limit
    /// ([`DEFAULT_MAX_LINE_BYTES`]).
    pub fn new(sink: impl Write + Send + 'static) -> Self {
        Self::with_max_line(sink, DEFAULT_MAX_LINE_BYTES)
    }

    /// A recorder that writes no line longer than `max_line` bytes (newline
    /// excluded, as a reader counts it).
    pub fn with_max_line(sink: impl Write + Send + 'static, max_line: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                sessions: Sessions::default(),
                next_seq: 0,
                sink: Box::new(sink),
                failed: None,
                dropped: 0,
                closed: false,
            }),
            max_line,
        }
    }

    /// Records one event and returns its `seq`.
    ///
    /// # Errors
    ///
    /// [`NotRecorded::TooLong`] when the event's line would exceed the line
    /// limit, [`NotRecorded::SinkFailed`] once a write has failed, and
    /// [`NotRecorded::Closed`] after [`Recorder::close`].
    pub fn record(
        &self,
        direction: Direction,
        transport: TransportKind,
        body: EventBody,
    ) -> Result<u64, NotRecorded> {
        let mut inner = self.lock();
        self.write(&mut inner, direction, transport, body)
    }

    /// Records several events as adjacent lines — no other event is written
    /// between them — and returns each one's outcome, in order. An event refused
    /// (too long, say) takes no `seq`, and the others are still written.
    pub fn record_all(
        &self,
        events: impl IntoIterator<Item = (Direction, TransportKind, EventBody)>,
    ) -> Vec<Result<u64, NotRecorded>> {
        let mut inner = self.lock();
        events
            .into_iter()
            .map(|(direction, transport, body)| self.write(&mut inner, direction, transport, body))
            .collect()
    }

    /// Records the trace's closing lifecycle event and seals it: every later
    /// [`Recorder::record`] is refused with [`NotRecorded::Closed`], so the close
    /// stays the last event even if a task still relaying records after it. A
    /// second close is refused the same way, so the first one wins.
    ///
    /// # Errors
    ///
    /// As [`Recorder::record`].
    pub fn close(
        &self,
        direction: Direction,
        transport: TransportKind,
        event: LifecycleEvent,
    ) -> Result<u64, NotRecorded> {
        let mut inner = self.lock();
        let written = self.write(
            &mut inner,
            direction,
            transport,
            EventBody::Lifecycle { event },
        );
        inner.closed = true;
        written
    }

    /// Whether [`Recorder::close`] has been called.
    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A panic while holding the lock can only come from the sink itself; the
        // counter and flag stay consistent either way, so the data is usable.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn write(
        &self,
        inner: &mut Inner,
        direction: Direction,
        transport: TransportKind,
        body: EventBody,
    ) -> Result<u64, NotRecorded> {
        if inner.closed {
            return Err(NotRecorded::Closed);
        }
        // Observed whether or not the event is then written: a session the trace
        // could not hold in full is still a session.
        inner.sessions.observe(direction, &body);
        if inner.failed.is_some() {
            inner.dropped += 1;
            return Err(NotRecorded::SinkFailed);
        }
        let seq = inner.next_seq;
        let event = TraceEvent::new(seq, direction, transport, body);
        let line = serde_json::to_vec(&event).map_err(io::Error::other);
        if line.as_ref().is_ok_and(|line| line.len() > self.max_line) {
            return Err(NotRecorded::TooLong);
        }
        match line.and_then(|line| write_line(&mut inner.sink, line)) {
            Ok(()) => {
                inner.next_seq += 1;
                Ok(seq)
            }
            Err(error) => {
                let message = error.to_string();
                inner.failed = Some(error);
                inner.dropped += 1;
                eprintln!(
                    "mcp-trace-capture: cannot write the trace ({message}); the session \
                     continues, but events from seq {seq} on are not recorded"
                );
                Err(NotRecorded::SinkFailed)
            }
        }
    }

    /// Flushes the sink and reports whether the trace is complete.
    pub fn finish(&self) -> Summary {
        let mut inner = self.lock();
        if inner.failed.is_none()
            && let Err(error) = inner.sink.flush()
        {
            inner.failed = Some(error);
        }
        Summary {
            recorded: inner.next_seq,
            dropped: inner.dropped,
            error: inner.failed.take(),
            sessions: inner.sessions.clone(),
        }
    }
}

fn write_line(sink: &mut Box<dyn Write + Send>, mut line: Vec<u8>) -> io::Result<()> {
    line.push(b'\n');
    sink.write_all(&line)?;
    sink.flush()
}

#[cfg(test)]
mod tests;
