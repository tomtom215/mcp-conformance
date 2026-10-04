// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mcp_conformance_core::trace::{Direction, EventBody, LifecycleEvent, TransportKind};

use super::*;

const INITIALIZE: &[u8] = br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
const PING: &[u8] = br#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#;

/// A fresh directory of this test's own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcp-trace-capture-traces-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn per_session(dir: &Path) -> Traces {
    let numbered = Numbered::parse(&dir.join("{session}.jsonl"))
        .unwrap()
        .unwrap();
    Traces::per_session(numbered, 1 << 20)
}

/// Records one event through `route`, so its file has a line to count.
fn record(route: &Route) {
    route
        .recorder()
        .record(
            Direction::ClientToServer,
            TransportKind::StreamableHttp,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportOpen,
            },
        )
        .unwrap();
}

fn seqs(path: &Path) -> Vec<u64> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["seq"]
                .as_u64()
                .unwrap()
        })
        .collect()
}

#[test]
fn initialize_is_recognised_alone_and_in_a_batch_and_only_as_a_request() {
    assert!(is_initialize(INITIALIZE));
    assert!(is_initialize(
        br#"[{"jsonrpc":"2.0","method":"x"},{"jsonrpc":"2.0","id":"a","method":"initialize"}]"#
    ));
    assert!(!is_initialize(PING));
    assert!(!is_initialize(
        br#"{"jsonrpc":"2.0","method":"initialize"}"#
    ));
    assert!(!is_initialize(b"not json"));
    assert!(!is_initialize(b""));
}

#[test]
fn each_session_gets_its_own_file_with_seq_from_zero() {
    let dir = scratch("own-file");
    let traces = per_session(&dir);
    // Two clients interleaved: each initializes, is assigned an id, and uses it.
    let first = traces.route(None, Some(INITIALIZE));
    record(&first);
    traces.respond(&first, Some("a"));
    let second = traces.route(None, Some(INITIALIZE));
    record(&second);
    traces.respond(&second, Some("b"));
    for id in ["a", "b", "a"] {
        record(&traces.route(Some(id), Some(PING)));
    }
    // No id, no initialize: the session begun most recently.
    record(&traces.route(None, Some(PING)));
    record(&traces.route(None, None));

    let finished = traces.finish();
    let paths: Vec<PathBuf> = finished.iter().map(|f| f.path.clone().unwrap()).collect();
    assert_eq!(paths, [dir.join("001.jsonl"), dir.join("002.jsonl")]);
    assert!(finished.iter().all(|f| f.summary.is_complete()));
    assert_eq!(seqs(&paths[0]), [0, 1, 2]);
    assert_eq!(seqs(&paths[1]), [0, 1, 2, 3]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_id_first_seen_on_a_request_begins_a_session_and_keeps_it() {
    let dir = scratch("unknown-id");
    let traces = per_session(&dir);
    let first = traces.route(Some("x"), Some(PING));
    // A response naming another id does not rename a session that has one.
    traces.respond(&first, Some("y"));
    let again = traces.route(Some("x"), None);
    assert_eq!(again.index, first.index);
    let other = traces.route(Some("y"), None);
    assert_ne!(other.index, first.index);
    assert_eq!(traces.finish().len(), 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_id_already_bound_is_not_rebound_by_a_later_response() {
    let dir = scratch("rebind");
    let traces = per_session(&dir);
    let first = traces.route(None, Some(INITIALIZE));
    traces.respond(&first, Some("a"));
    let second = traces.route(None, Some(INITIALIZE));
    traces.respond(&second, Some("a"));
    assert_eq!(traces.route(Some("a"), None).index, first.index);
    // `second` is still unnamed, so a fresh id may name it.
    traces.respond(&second, Some("b"));
    assert_eq!(traces.route(Some("b"), None).index, second.index);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn sessionless_traffic_is_one_session() {
    let dir = scratch("stateless");
    let traces = per_session(&dir);
    for _ in 0..3 {
        let route = traces.route(None, Some(PING));
        record(&route);
        traces.respond(&route, None);
    }
    let finished = traces.finish();
    assert_eq!(finished.len(), 1);
    assert_eq!(seqs(finished[0].path.as_ref().unwrap()), [0, 1, 2]);
    std::fs::remove_dir_all(&dir).ok();
}

/// A sink the test reads back after the recorder owns it.
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

#[test]
fn one_trace_holds_every_session_in_one_sequence() {
    let sink = Arc::new(Mutex::new(Vec::new()));
    let traces = Traces::one(Arc::new(Recorder::new(Shared(Arc::clone(&sink)))));
    record(&traces.route(None, Some(INITIALIZE)));
    record(&traces.route(None, Some(PING)));
    record(&traces.route(None, Some(INITIALIZE)));
    let finished = traces.finish();
    assert_eq!(finished.len(), 1);
    assert!(finished[0].path.is_none());
    assert_eq!(finished[0].summary.recorded, 3);
    assert_eq!(
        String::from_utf8(sink.lock().unwrap().clone())
            .unwrap()
            .lines()
            .count(),
        3
    );
}

#[test]
fn closing_writes_the_close_event_to_every_trace() {
    let dir = scratch("close-all");
    let traces = per_session(&dir);
    let first = traces.route(None, Some(INITIALIZE));
    record(&first);
    let second = traces.route(None, Some(INITIALIZE));
    record(&second);
    traces.close_all(LifecycleEvent::TransportClose);
    for finished in traces.finish() {
        let text = std::fs::read_to_string(finished.path.unwrap()).unwrap();
        let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!(last["event"], "transport-close", "{text}");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_file_that_cannot_be_created_makes_that_trace_incomplete_not_the_proxy_fail() {
    let dir = scratch("uncreatable");
    let numbered = Numbered::parse(&dir.join("absent/{session}.jsonl"))
        .unwrap()
        .unwrap();
    let traces = Traces::per_session(numbered, 1 << 20);
    let route = traces.route(None, Some(INITIALIZE));
    assert!(
        route
            .recorder()
            .record(
                Direction::ClientToServer,
                TransportKind::StreamableHttp,
                EventBody::Lifecycle {
                    event: LifecycleEvent::TransportOpen,
                },
            )
            .is_err()
    );
    let finished = traces.finish();
    assert_eq!(finished.len(), 1);
    assert!(finished[0].path.is_none());
    let error = finished[0].summary.error.as_ref().unwrap().to_string();
    assert!(error.contains("absent"), "{error}");
    std::fs::remove_dir_all(&dir).ok();
}
