// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;
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
        Ok(0)
    );
    let message = EventBody::Message {
        payload: serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
    };
    assert_eq!(
        recorder.record(Direction::ServerToClient, TransportKind::Stdio, message),
        Ok(1)
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
        Ok(0)
    );
    let big = EventBody::Message {
        payload: serde_json::json!({"text": "x".repeat(500)}),
    };
    assert_eq!(
        recorder.record(Direction::ServerToClient, TransportKind::Stdio, big),
        Err(NotRecorded::SinkFailed)
    );
    // Even an event that would fit is dropped once the sink has failed: a
    // trace with a hole in it is worse than a truncated one.
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
        Err(NotRecorded::SinkFailed)
    );
    let summary = recorder.finish();
    assert!(!summary.is_complete());
    assert_eq!((summary.recorded, summary.dropped), (1, 2));
}

#[test]
fn a_line_over_the_limit_is_refused_without_using_a_seq() {
    let sink = Shared::default();
    let message = |text: &str| EventBody::Message {
        payload: serde_json::json!({ "t": text }),
    };
    // The line for {"t":"ab"} at seq 0, measured rather than assumed.
    let fits = serde_json::to_vec(&TraceEvent::new(
        0,
        Direction::ClientToServer,
        TransportKind::Stdio,
        message("ab"),
    ))
    .unwrap()
    .len();
    let recorder = Recorder::with_max_line(sink.clone(), fits);
    let record = |text: &str| {
        recorder.record(
            Direction::ClientToServer,
            TransportKind::Stdio,
            message(text),
        )
    };
    assert_eq!(record("abc"), Err(NotRecorded::TooLong));
    assert_eq!(record("ab"), Ok(0), "exactly the limit is kept, at seq 0");
    assert_eq!(record("abc"), Err(NotRecorded::TooLong));
    let summary = recorder.finish();
    assert!(
        summary.is_complete(),
        "a refused line is not a sink failure"
    );
    assert_eq!((summary.recorded, summary.dropped), (1, 0));
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert_eq!(text.lines().next().unwrap().len(), fits);
}

#[test]
fn a_number_that_grows_when_rewritten_is_measured_as_written() {
    // `9e15` is 4 bytes read and 18 written: the limit applies to the line
    // as the trace holds it.
    let payload: serde_json::Value = serde_json::from_str("[9e15]").unwrap();
    assert_eq!(
        serde_json::to_string(&payload).unwrap(),
        "[9000000000000000.0]"
    );
    let body = || EventBody::Message {
        payload: payload.clone(),
    };
    let written = serde_json::to_vec(&TraceEvent::new(
        0,
        Direction::ClientToServer,
        TransportKind::Stdio,
        body(),
    ))
    .unwrap()
    .len();
    let recorder = Recorder::with_max_line(Shared::default(), written - 1);
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, body()),
        Err(NotRecorded::TooLong)
    );
}

#[test]
fn debug_shows_the_counters_not_the_sink() {
    let recorder = Recorder::new(Shared::default());
    let _ = recorder.record(Direction::ClientToServer, TransportKind::Stdio, open());
    let debug = format!("{recorder:?}");
    assert!(debug.contains("next_seq: 1"), "{debug}");
    assert!(debug.contains("dropped: 0"), "{debug}");
    assert!(debug.contains(".."), "the sink is elided: {debug}");
}

#[test]
fn a_closed_trace_keeps_its_close_as_the_last_event() {
    let sink = Shared::default();
    let recorder = Recorder::new(sink.clone());
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
        Ok(0)
    );
    assert!(!recorder.is_closed());
    assert_eq!(
        recorder.close(
            Direction::ServerToClient,
            TransportKind::Stdio,
            LifecycleEvent::TransportClose
        ),
        Ok(1)
    );
    assert!(recorder.is_closed());
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
        Err(NotRecorded::Closed)
    );
    assert_eq!(
        recorder.close(
            Direction::ClientToServer,
            TransportKind::Stdio,
            LifecycleEvent::TransportAbort
        ),
        Err(NotRecorded::Closed),
        "the first close wins"
    );
    let summary = recorder.finish();
    assert!(
        summary.is_complete(),
        "a refusal after the close is not a loss"
    );
    assert_eq!((summary.recorded, summary.dropped), (2, 0));
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(text.lines().last().unwrap().contains("transport-close"));
}

#[test]
fn events_recorded_together_are_adjacent_and_a_refused_one_takes_no_seq() {
    let sink = Shared::default();
    let recorder = Recorder::with_max_line(sink.clone(), 200);
    let big = EventBody::Message {
        payload: serde_json::json!({"text": "x".repeat(500)}),
    };
    let results = recorder.record_all([
        (Direction::ClientToServer, TransportKind::Stdio, open()),
        (Direction::ClientToServer, TransportKind::Stdio, big),
        (Direction::ClientToServer, TransportKind::Stdio, open()),
    ]);
    assert_eq!(results, [Ok(0), Err(NotRecorded::TooLong), Ok(1)]);
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert_eq!(text.lines().count(), 2);
}

#[test]
fn sessions_are_counted_from_initialize_requests_and_session_ids() {
    let recorder = Recorder::new(Shared::default());
    let initialize = |id: u64| EventBody::Message {
        payload: serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "initialize"}),
    };
    let http = |id: &str| EventBody::Http {
        method: None,
        status: Some(200),
        headers: [("mcp-session-id".to_owned(), id.to_owned())].into(),
    };
    let _ = recorder.record(
        Direction::ClientToServer,
        TransportKind::Stdio,
        initialize(1),
    );
    let _ = recorder.record(
        Direction::ClientToServer,
        TransportKind::Stdio,
        initialize(2),
    );
    // A server echoing the method name is not a client starting a session.
    let _ = recorder.record(
        Direction::ServerToClient,
        TransportKind::Stdio,
        initialize(1),
    );
    for id in ["a", "a", "b"] {
        let _ = recorder.record(
            Direction::ServerToClient,
            TransportKind::StreamableHttp,
            http(id),
        );
    }
    let sessions = recorder.finish().sessions;
    assert_eq!(sessions.initialize_requests, 2);
    assert_eq!(sessions.session_ids.len(), 2);
    assert_eq!(sessions.count(), 2);
}
