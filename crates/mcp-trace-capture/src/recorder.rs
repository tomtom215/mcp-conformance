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

use std::io::{self, Write};
use std::sync::{Mutex, PoisonError};

use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent, TransportKind};

/// Appends trace events to a sink, one JSON object per line.
#[derive(Debug)]
pub struct Recorder {
    inner: Mutex<Inner>,
}

struct Inner {
    next_seq: u64,
    sink: Box<dyn Write + Send>,
    failed: Option<io::Error>,
    dropped: u64,
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
}

impl Summary {
    /// Whether every observed event reached the trace.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.error.is_none()
    }
}

impl Recorder {
    /// A recorder writing to `sink`, starting at `seq` 0.
    pub fn new(sink: impl Write + Send + 'static) -> Self {
        Self {
            inner: Mutex::new(Inner {
                next_seq: 0,
                sink: Box::new(sink),
                failed: None,
                dropped: 0,
            }),
        }
    }

    /// Records one event and returns its `seq`, or `None` once the sink has failed.
    pub fn record(
        &self,
        direction: Direction,
        transport: TransportKind,
        body: EventBody,
    ) -> Option<u64> {
        // A panic while holding the lock can only come from the sink itself; the
        // counter and flag stay consistent either way, so the data is usable.
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.failed.is_some() {
            inner.dropped += 1;
            return None;
        }
        let seq = inner.next_seq;
        let event = TraceEvent::new(seq, direction, transport, body);
        match write_line(&mut inner.sink, &event) {
            Ok(()) => {
                inner.next_seq += 1;
                Some(seq)
            }
            Err(error) => {
                eprintln!(
                    "mcp-trace-capture: cannot write the trace ({error}); the session \
                     continues, but events from seq {seq} on are not recorded"
                );
                inner.failed = Some(error);
                inner.dropped += 1;
                None
            }
        }
    }

    /// Flushes the sink and reports whether the trace is complete.
    pub fn finish(&self) -> Summary {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if inner.failed.is_none()
            && let Err(error) = inner.sink.flush()
        {
            inner.failed = Some(error);
        }
        Summary {
            recorded: inner.next_seq,
            dropped: inner.dropped,
            error: inner.failed.take(),
        }
    }
}

fn write_line(sink: &mut Box<dyn Write + Send>, event: &TraceEvent) -> io::Result<()> {
    let mut line = serde_json::to_vec(event).map_err(io::Error::other)?;
    line.push(b'\n');
    sink.write_all(&line)?;
    sink.flush()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use mcp_conformance_core::trace::LifecycleEvent;
    use std::sync::Arc;

    /// A sink the test can read back after the recorder owns it.
    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Accepts `budget` bytes, then fails every write.
    struct Failing {
        budget: usize,
    }

    impl Write for Failing {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.budget {
                return Err(io::Error::other("disk full"));
            }
            self.budget -= bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn open() -> EventBody {
        EventBody::Lifecycle {
            event: LifecycleEvent::TransportOpen,
        }
    }

    #[test]
    fn events_are_numbered_in_order_and_parse_back() {
        let sink = Shared::default();
        let recorder = Recorder::new(sink.clone());
        assert_eq!(
            recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
            Some(0)
        );
        let message = EventBody::Message {
            payload: serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        };
        assert_eq!(
            recorder.record(Direction::ServerToClient, TransportKind::Stdio, message),
            Some(1)
        );
        let summary = recorder.finish();
        assert!(summary.is_complete());
        assert_eq!(summary.recorded, 2);

        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let events: Vec<TraceEvent> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[1].seq, 1);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn a_failed_write_stops_recording_without_panicking_and_is_reported() {
        let recorder = Recorder::new(Failing { budget: 200 });
        assert_eq!(
            recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
            Some(0)
        );
        let big = EventBody::Message {
            payload: serde_json::json!({"text": "x".repeat(500)}),
        };
        assert_eq!(
            recorder.record(Direction::ServerToClient, TransportKind::Stdio, big),
            None
        );
        // Even an event that would fit is dropped once the sink has failed: a
        // trace with a hole in it is worse than a truncated one.
        assert_eq!(
            recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
            None
        );
        let summary = recorder.finish();
        assert!(!summary.is_complete());
        assert_eq!((summary.recorded, summary.dropped), (1, 2));
    }
}
