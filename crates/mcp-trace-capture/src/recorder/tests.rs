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
    assert_eq!(summary.messages, 0, "lifecycle events are not messages");
    assert_eq!(
        summary.after_close, 1,
        "the event refused after the close is counted; the second close is not"
    );
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
    let message = EventBody::Message {
        payload: serde_json::json!({"jsonrpc": "2.0", "method": "x"}),
    };
    let _ = recorder.record(Direction::ClientToServer, TransportKind::Stdio, message);
    assert_eq!(
        recorder.finish().messages,
        1,
        "the message written, not the one refused"
    );
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert_eq!(
        text.lines().count(),
        3,
        "two lifecycle events and the message"
    );
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

/// Keeps every line it is shown, with its trace number.
#[derive(Debug, Default)]
struct Seen(Mutex<Vec<(u64, Vec<u8>)>>);

impl Observer for Seen {
    fn observe(&self, trace: u64, line: &[u8]) {
        self.0.lock().unwrap().push((trace, line.to_vec()));
    }
}

#[test]
fn an_observer_sees_each_written_line_in_seq_order_and_nothing_else() {
    let sink = Shared::default();
    let seen = Arc::new(Seen::default());
    let recorder = Recorder::with_max_line(sink.clone(), 200)
        .observed(Arc::clone(&seen) as Arc<dyn Observer>, 7);
    recorder
        .record(Direction::ClientToServer, TransportKind::Stdio, open())
        .unwrap();
    let long = EventBody::Message {
        payload: serde_json::json!("x".repeat(300)),
    };
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, long),
        Err(NotRecorded::TooLong)
    );
    recorder
        .close(
            Direction::ClientToServer,
            TransportKind::Stdio,
            LifecycleEvent::TransportClose,
        )
        .unwrap();
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
        Err(NotRecorded::Closed)
    );
    let seen = seen.0.lock().unwrap().clone();
    let written = sink.0.lock().unwrap().clone();
    let lines: Vec<&[u8]> = written
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(seen.len(), 2, "the refused events are not shown");
    assert_eq!(lines.len(), 2);
    for ((trace, line), written) in seen.iter().zip(lines) {
        assert_eq!(*trace, 7);
        assert_eq!(line.as_slice(), written, "the line as written, no newline");
    }
}

#[test]
fn a_failed_write_is_not_shown_to_the_observer() {
    let seen = Arc::new(Seen::default());
    let recorder = Recorder::new(Failing { budget: 0 }).observed(Arc::clone(&seen) as _, 1);
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, open()),
        Err(NotRecorded::SinkFailed)
    );
    assert!(seen.0.lock().unwrap().is_empty());
}

/// The trace's lines, parsed.
fn lines_of(sink: &Shared) -> Vec<serde_json::Value> {
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    text.lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn ping() -> EventBody {
    EventBody::Message {
        payload: serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
    }
}

fn close_noting(recorder: &Recorder) -> Result<u64, NotRecorded> {
    recorder.close_with_note(
        Direction::ServerToClient,
        TransportKind::Stdio,
        LifecycleEvent::TransportClose,
    )
}

#[test]
fn the_closing_event_says_what_the_recorder_was_told_it_left_out() {
    let sink = Shared::default();
    let recorder = Recorder::new(sink.clone());
    recorder
        .record(Direction::ClientToServer, TransportKind::Stdio, ping())
        .unwrap();
    recorder.note_unrecorded(Loss::Oversized, 2);
    recorder.note_unrecorded(Loss::OverBudget, 1);
    recorder.note_unrecorded(Loss::Oversized, 1);
    assert_eq!(close_noting(&recorder), Ok(1));
    let lines = lines_of(&sink);
    assert_eq!(
        lines[1],
        serde_json::json!({
            "seq": 1, "direction": "server-to-client", "transport": "stdio",
            "kind": "lifecycle", "event": "transport-close",
            "capture": {"unrecorded": {"over_budget": 1, "oversized": 3}}
        })
    );
    assert!(
        lines[0].get("capture").is_none(),
        "only the closing event carries it"
    );
}

#[test]
fn a_recorder_told_nothing_was_left_out_says_so() {
    // `{}` is a statement (nothing was left out); no member would be silence.
    let sink = Shared::default();
    let recorder = Recorder::new(sink.clone());
    recorder
        .record(Direction::ClientToServer, TransportKind::Stdio, ping())
        .unwrap();
    close_noting(&recorder).unwrap();
    assert_eq!(
        lines_of(&sink)[1]["capture"],
        serde_json::json!({"unrecorded": {}})
    );
}

#[test]
fn a_plain_close_makes_no_claim_about_completeness() {
    // `close` is every caller's that was never told to report what it left
    // out; writing `{}` for them would assert a completeness nobody checked.
    let sink = Shared::default();
    let recorder = Recorder::new(sink.clone());
    recorder.note_unrecorded(Loss::Oversized, 5);
    recorder
        .close(
            Direction::ServerToClient,
            TransportKind::Stdio,
            LifecycleEvent::TransportClose,
        )
        .unwrap();
    assert!(lines_of(&sink)[0].get("capture").is_none());
}

#[test]
fn a_line_the_recorder_refuses_for_length_is_counted_as_oversized_by_the_recorder() {
    let sink = Shared::default();
    let recorder = Recorder::with_max_line(sink.clone(), 200);
    let big = EventBody::Message {
        payload: serde_json::Value::String("x".repeat(300)),
    };
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, big),
        Err(NotRecorded::TooLong)
    );
    assert_eq!(
        recorder.record(Direction::ClientToServer, TransportKind::Stdio, ping()),
        Ok(0)
    );
    close_noting(&recorder).unwrap();
    assert_eq!(
        lines_of(&sink)[1]["capture"]["unrecorded"],
        serde_json::json!({"oversized": 1})
    );
}

#[test]
fn what_is_left_out_after_the_close_is_in_the_summary_and_not_in_the_note() {
    let sink = Shared::default();
    let recorder = Recorder::new(sink.clone());
    recorder
        .record(Direction::ClientToServer, TransportKind::Stdio, ping())
        .unwrap();
    recorder.note_unrecorded(Loss::OtherEvents, 1);
    close_noting(&recorder).unwrap();
    recorder.note_unrecorded(Loss::OtherEvents, 2);
    recorder.note_unrecorded(Loss::Oversized, 4);
    assert_eq!(
        lines_of(&sink)[1]["capture"]["unrecorded"],
        serde_json::json!({"other_events": 1}),
        "the close is the last event and says what had happened by then"
    );
    let summary = recorder.finish();
    assert_eq!(summary.unrecorded.get(&Loss::OtherEvents).copied(), Some(3));
    assert_eq!(summary.unrecorded.get(&Loss::Oversized).copied(), Some(4));
    assert!(!summary.unrecorded.contains_key(&Loss::OverBudget));
}

#[test]
fn a_loss_counted_at_zero_is_not_a_loss() {
    let recorder = Recorder::new(Shared::default());
    recorder.note_unrecorded(Loss::Oversized, 0);
    assert!(recorder.finish().unrecorded.is_empty());
}

#[test]
fn the_names_a_trace_carries_are_the_counters_names() {
    assert_eq!(Loss::Oversized.name(), "oversized");
    assert_eq!(Loss::OverBudget.name(), "over_budget");
    assert_eq!(Loss::OtherEvents.name(), "other_events");
}
