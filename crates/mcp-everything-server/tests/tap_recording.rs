// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The session trace tap, exercised through the real HTTP app — policy
//! middleware, tap layer, and rmcp service — via `tower::ServiceExt::oneshot`
//! (no sockets, no network). Every assertion reads the JSON Lines trace the
//! tap wrote and checks it against the recording contract in
//! `src/tap.rs`: allowlisted headers only, exact event sequences, strictly
//! increasing `seq`, streamed SSE frames captured without disturbing the
//! bytes on the wire.

#![cfg(feature = "tap")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio_stream::StreamExt as _;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use mcp_everything_server::http::router_tapped;
use mcp_everything_server::policy::HttpSecurityPolicy;
use mcp_everything_server::tap::Tap;
use tower::ServiceExt as _;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"tap-test","version":"0.0.0"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const CALL_LOGGING_TOOL: &str = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"test_tool_with_logging","arguments":{}}}"#;

/// A unique, empty tap directory for one test.
fn tap_dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tap-recording-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The tapped app plus its trace directory.
fn tapped_app(test: &str) -> (Router, PathBuf) {
    let dir = tap_dir(test);
    let tap = Tap::new(dir.clone()).expect("tap directory");
    (router_tapped(HttpSecurityPolicy::default(), tap), dir)
}

/// A loopback `/mcp` POST carrying `body`, plus any extra headers.
fn mcp_post(body: &'static str, extra: &[(&str, &str)]) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost:8080")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream");
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder.body(Body::from(body)).unwrap()
}

/// Initializes a session through `app`, returning the session ID and the
/// response body text (the result travels as JSON or as an SSE frame
/// depending on the transport's choice; assertions stay agnostic).
async fn initialize(app: &Router, extra: &[(&str, &str)]) -> (String, String) {
    let response = app
        .clone()
        .oneshot(mcp_post(INITIALIZE, extra))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "initialize succeeds");
    let session_id = response
        .headers()
        .get("mcp-session-id")
        .expect("initialize response carries the session id")
        .to_str()
        .unwrap()
        .to_owned();
    let body = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    (session_id, String::from_utf8_lossy(&body).into_owned())
}

/// Polls (5 ms interval, 10 s deadline) until the session's trace file holds
/// at least `lines` events, then returns them parsed. The tap's writer is
/// asynchronous by design; the deadline only bounds failure detection.
async fn read_trace(dir: &Path, session_id: &str, lines: usize) -> Vec<serde_json::Value> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let events = trace_events(dir, session_id);
        if events.len() >= lines {
            return events;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "trace for {session_id} reached only {} of {lines} events within 10s",
            events.len()
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// The session's currently written events (empty when the file is absent).
fn trace_events(dir: &Path, session_id: &str) -> Vec<serde_json::Value> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.contains(session_id) {
            let text = std::fs::read_to_string(entry.path()).unwrap();
            let lines: Vec<&str> = text.lines().collect();
            let mut events = Vec::new();
            for (index, line) in lines.iter().enumerate() {
                match serde_json::from_str(line) {
                    Ok(event) => events.push(event),
                    // The writer appends the line and its newline separately;
                    // a final line caught mid-write is in-progress, not
                    // corrupt — exactly the truncation-survivable property
                    // the JSON Lines format is chosen for. Anything earlier
                    // failing to parse IS corruption.
                    Err(error) => {
                        assert_eq!(
                            index,
                            lines.len() - 1,
                            "only the final line may be mid-write: line {index}: {error}"
                        );
                        break;
                    }
                }
            }
            return events;
        }
    }
    Vec::new()
}

/// An initialized session (initialize + notifications/initialized exchanged,
/// seven events on disk), ready for feature traffic.
async fn initialized_session(app: &Router, dir: &Path) -> String {
    let (session_id, _) = initialize(app, &[]).await;
    let _ = read_trace(dir, &session_id, 4).await;
    let response = app
        .clone()
        .oneshot(mcp_post(
            INITIALIZED,
            &[
                ("mcp-session-id", session_id.as_str()),
                ("mcp-protocol-version", "2025-11-25"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    let _ = read_trace(dir, &session_id, 7).await;
    session_id
}

/// Opens the session's standalone GET stream and spawns a reader that
/// captures the raw SSE text as it flows through the tap.
async fn spawn_get_reader(
    app: &Router,
    session_id: &str,
) -> (tokio::task::JoinHandle<()>, Arc<std::sync::Mutex<String>>) {
    let request = Request::builder()
        .method("GET")
        .uri("/mcp")
        .header("host", "localhost:8080")
        .header("accept", "text/event-stream")
        .header("mcp-session-id", session_id)
        .header("mcp-protocol-version", "2025-11-25")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let wire = Arc::new(std::sync::Mutex::new(String::new()));
    let sink = Arc::clone(&wire);
    let reader = tokio::spawn(async move {
        let mut stream = response.into_body().into_data_stream();
        while let Some(Ok(chunk)) = stream.next().await {
            sink.lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&chunk));
        }
    });
    (reader, wire)
}

#[tokio::test]
async fn initialize_exchange_is_recorded_with_allowlisted_headers_only() {
    let (app, dir) = tapped_app("init");
    // Credential-bearing headers ride along; the trace must never see them.
    let (session_id, _) = initialize(
        &app,
        &[("authorization", "Bearer secret"), ("cookie", "id=42")],
    )
    .await;

    let events = read_trace(&dir, &session_id, 4).await;
    assert_eq!(events.len(), 4, "req http + req msg + resp http + resp msg");

    // seq is strictly increasing from zero — the schema's ordering authority.
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], index as u64, "seq order at {index}");
        assert_eq!(event["transport"], "streamable-http");
    }

    assert_eq!(events[0]["kind"], "http");
    assert_eq!(events[0]["direction"], "client-to-server");
    let request_headers = events[0]["headers"].as_object().unwrap();
    assert_eq!(
        request_headers.get("host").and_then(|v| v.as_str()),
        Some("localhost:8080")
    );
    assert!(
        !request_headers.contains_key("authorization") && !request_headers.contains_key("cookie"),
        "credential-bearing headers must never be recorded: {request_headers:?}"
    );

    assert_eq!(events[1]["kind"], "message");
    assert_eq!(events[1]["payload"]["method"], "initialize");

    assert_eq!(events[2]["kind"], "http");
    assert_eq!(events[2]["direction"], "server-to-client");
    assert_eq!(events[2]["status"], 200);
    assert_eq!(
        events[2]["headers"]["mcp-session-id"].as_str(),
        Some(session_id.as_str())
    );

    assert_eq!(events[3]["kind"], "message");
    assert_eq!(events[3]["direction"], "server-to-client");
    assert_eq!(
        events[3]["payload"]["result"]["protocolVersion"], "2025-11-25",
        "the initialize result is captured whichever body framing served it"
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn notification_202_records_the_request_but_no_response_message() {
    let (app, dir) = tapped_app("notify");
    let session_id = initialized_session(&app, &dir).await;

    let events = read_trace(&dir, &session_id, 7).await;
    assert_eq!(events.len(), 7, "exactly three new events for a 202");
    assert_eq!(events[4]["kind"], "http");
    assert_eq!(
        events[5]["payload"]["method"], "notifications/initialized",
        "the notification body is a recorded message"
    );
    assert_eq!(events[6]["kind"], "http");
    assert_eq!(events[6]["status"], 202);

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sse_frames_are_recorded_as_messages_and_delivered_intact() {
    let (app, dir) = tapped_app("sse");
    let session_id = initialized_session(&app, &dir).await;
    // Server-to-client notifications travel on the session's standalone GET
    // stream, exactly as a real client would receive them.
    let (reader, get_wire) = spawn_get_reader(&app, &session_id).await;

    let response = app
        .clone()
        .oneshot(mcp_post(
            CALL_LOGGING_TOOL,
            &[
                ("mcp-session-id", session_id.as_str()),
                ("mcp-protocol-version", "2025-11-25"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Draining the POST body is what drives its recording pass-through.
    let body = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let post_wire = String::from_utf8_lossy(&body).into_owned();

    // 7 prior events + GET req/resp http + POST req http/msg/resp http +
    // result + three staged notifications = 16.
    let events = read_trace(&dir, &session_id, 16).await;
    reader.abort();

    assert_recorded_tool_call_messages(&events);

    // Pass-through fidelity: the same frames reached the real client.
    assert!(
        post_wire.contains("executed successfully"),
        "result delivered on the POST wire: {post_wire}"
    );
    assert_eq!(
        get_wire
            .lock()
            .unwrap()
            .matches("notifications/message")
            .count(),
        3,
        "all three notifications were delivered, untouched, on the GET wire"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// The message-level recording contract after the logging tool call: exactly
/// the session's eight messages, with the notifications from the GET stream
/// and the result from the POST stream — strictly seq-ordered, nothing
/// invented.
fn assert_recorded_tool_call_messages(events: &[serde_json::Value]) {
    for window in events.windows(2) {
        // Unwrap before comparing: `Option<u64>`'s ordering would let a
        // missing `seq` on the left slip through (`None < Some(_)` is true).
        let earlier = window[0]["seq"].as_u64().expect("event carries seq");
        let later = window[1]["seq"].as_u64().expect("event carries seq");
        assert!(earlier < later, "seq must strictly increase: {window:?}");
    }
    let messages: Vec<_> = events
        .iter()
        .filter(|event| event["kind"] == "message")
        .collect();
    assert_eq!(
        messages.len(),
        8,
        "initialize req+result, initialized, tools/call req+result, three \
         staged notifications — and nothing invented"
    );
    assert_eq!(
        messages
            .iter()
            .filter(|m| m["payload"]["method"] == "notifications/message")
            .count(),
        3,
        "the three staged log notifications are recorded from the GET stream"
    );
    assert!(
        messages.iter().any(|m| m["payload"]["id"] == 2
            && m["payload"]["result"]["content"][0]["text"]
                .as_str()
                .is_some_and(|text| text.contains("executed successfully"))),
        "the tool result frame is recorded from the POST stream"
    );
}

#[tokio::test]
async fn delete_records_transport_close() {
    let (app, dir) = tapped_app("delete");
    let session_id = initialized_session(&app, &dir).await;

    let request = Request::builder()
        .method("DELETE")
        .uri("/mcp")
        .header("host", "localhost:8080")
        .header("mcp-session-id", session_id.as_str())
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert!(
        response.status().is_success(),
        "session delete succeeds: {}",
        response.status()
    );

    let events = read_trace(&dir, &session_id, 10).await;
    let last = events.last().unwrap();
    assert_eq!(last["kind"], "lifecycle");
    assert_eq!(last["event"], "transport-close");

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sessions_get_ordinal_files_in_creation_order() {
    let (app, dir) = tapped_app("ordinals");
    let (first, _) = initialize(&app, &[]).await;
    let _ = read_trace(&dir, &first, 4).await;
    let (second, _) = initialize(&app, &[]).await;
    let _ = read_trace(&dir, &second, 4).await;

    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names.len(), 2);
    assert!(
        names[0].starts_with("001-") && names[0].contains(&first),
        "first session leads the listing: {names:?}"
    );
    assert!(
        names[1].starts_with("002-") && names[1].contains(&second),
        "second session follows: {names:?}"
    );

    let _ = std::fs::remove_dir_all(dir);
}

/// The raw bytes of the session's trace file, exactly as the tap wrote them.
fn raw_trace_bytes(dir: &Path, session_id: &str) -> String {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        if entry.file_name().to_string_lossy().contains(session_id) {
            return std::fs::read_to_string(entry.path()).unwrap();
        }
    }
    panic!("no trace file for session {session_id}");
}

#[tokio::test]
async fn tap_output_parses_and_validates_through_the_real_validator() {
    // The agreement check proves the tap→validator contract end to end, but
    // only in the npx-gated conformance job. This runs the same contract on
    // every platform in the test matrix, with no network: the tap's actual
    // output bytes must parse through `mcp_trace_validator`'s real reader and
    // validate against the builtin registry — so a serialization change in the
    // tap that the reader could not read would fail here, not silently in CI.
    let (app, dir) = tapped_app("validator-roundtrip");
    let session_id = initialized_session(&app, &dir).await;
    // Add feature traffic so the trace exercises more than the handshake.
    let response = app
        .clone()
        .oneshot(mcp_post(
            CALL_LOGGING_TOOL,
            &[
                ("mcp-session-id", session_id.as_str()),
                ("mcp-protocol-version", "2025-11-25"),
            ],
        ))
        .await
        .unwrap();
    let _ = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let _ = read_trace(&dir, &session_id, 8).await;

    let bytes = raw_trace_bytes(&dir, &session_id);
    // 1. The reader accepts the tap's bytes verbatim — no field-name or shape
    //    mismatch between what the tap serializes and what the reader expects.
    let events = mcp_trace_validator::reader::parse_trace(
        &bytes,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .expect("tap output must parse through the validator's reader");
    assert!(
        events.len() >= 7,
        "the handshake plus the tool call is at least seven events: {}",
        events.len()
    );
    // 2. The validator judges the parsed session: the initialize handshake the
    //    tap recorded validates with no MUST-level lifecycle failure.
    let registry = mcp_conformance_core::requirement::Registry::builtin_2025_11_25().unwrap();
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    let failures: Vec<&str> = report
        .requirements
        .iter()
        .filter(|row| row.outcome == mcp_trace_validator::report::Outcome::Fail)
        .map(|row| row.id.as_str())
        .collect();
    assert!(
        failures.is_empty(),
        "a faithfully tapped clean session must produce zero MUST-level \
         failures in any area — wrong header recording, broken seq, or \
         mangled payloads all surface exactly here, on every platform, \
         not only in the npx-gated conformance job. Failed: {failures:?}\n{}",
        report.render_human()
    );

    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn sessionless_exchanges_are_not_recorded() {
    let (app, dir) = tapped_app("sessionless");
    // A GET without a session id never forms a session; whatever the MCP
    // service answers, the tap must record nothing.
    let request = Request::builder()
        .method("GET")
        .uri("/mcp")
        .header("host", "localhost:8080")
        .header("accept", "text/event-stream")
        .body(Body::empty())
        .unwrap();
    let _ = app.clone().oneshot(request).await.unwrap();

    let recorded = std::fs::read_dir(&dir).map_or(0, Iterator::count);
    assert_eq!(recorded, 0, "no session, no trace file");

    let _ = std::fs::remove_dir_all(dir);
}

/// A `/mcp` POST on `session_id` carrying an arbitrary (possibly huge) body.
fn session_post(session_id: &str, body: Vec<u8>) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("host", "localhost:8080")
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", session_id)
        .header("mcp-protocol-version", "2025-11-25")
        .body(Body::from(body))
        .unwrap()
}

#[tokio::test]
async fn oversized_request_bodies_pass_through_unrecorded() {
    let (app, dir) = tapped_app("oversized");
    let session_id = initialized_session(&app, &dir).await;

    // One byte past the recording cap: the tap steps aside entirely — the
    // exchange is served (with an empty body, the documented trade), and the
    // trace gains nothing.
    let huge = vec![b'x'; 4 * 1024 * 1024 + 1];
    let response = app
        .clone()
        .oneshot(session_post(&session_id, huge))
        .await
        .unwrap();
    assert!(
        response.status().is_client_error(),
        "an empty body is the service's problem to reject, not a tap crash: {}",
        response.status()
    );

    // The other side of the boundary: a large body still under the cap must
    // be recorded in full — this pins the cap's arithmetic from below.
    let padding = "x".repeat(3 * 1024 * 1024);
    let under_cap = format!(
        r#"{{"jsonrpc":"2.0","method":"notifications/tap-cap-probe","params":{{"pad":"{padding}"}}}}"#
    );
    let _ = app
        .clone()
        .oneshot(session_post(&session_id, under_cap.into_bytes()))
        .await
        .unwrap();
    let events = read_trace(&dir, &session_id, 9).await;
    assert!(
        events.iter().any(|event| {
            event["payload"]["method"] == "notifications/tap-cap-probe"
                && event["payload"]["params"]["pad"]
                    .as_str()
                    .is_some_and(|pad| pad.len() == 3 * 1024 * 1024)
        }),
        "an under-cap body is recorded byte-complete"
    );

    // Settle the writer via an exchange that *is* recorded (a session
    // delete), then check that nothing from the oversized one landed.
    let request = Request::builder()
        .method("DELETE")
        .uri("/mcp")
        .header("host", "localhost:8080")
        .header("mcp-session-id", session_id.as_str())
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert!(response.status().is_success());
    let events = read_trace(&dir, &session_id, 13).await;
    assert!(
        !events.iter().any(|event| event["payload"]["params"]["pad"]
            .as_str()
            .is_some_and(|pad| pad.len() > 4 * 1024 * 1024)),
        "the oversized exchange contributed no events"
    );
    assert_eq!(events.len(), 13, "seven prior + cap probe (3) + delete (3)");

    let _ = std::fs::remove_dir_all(dir);
}

/// One stress-test session: initialize, call `echo` with a session-distinct
/// marker, and return the session's recorded events once eleven are on disk
/// (seven handshake + call request headers/message + 200 + result).
async fn stress_session(
    app: Router,
    dir: PathBuf,
    index: usize,
) -> (String, String, Vec<serde_json::Value>) {
    let session_id = initialized_session(&app, &dir).await;
    // Fixed width so no marker is a substring of another (stress-1 would
    // match inside stress-12 and fake a bleed).
    let marker = format!("stress-{index:02}");
    let call = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"echo","arguments":{{"message":"{marker}"}}}}}}"#
    );
    let response = app
        .clone()
        .oneshot(mcp_post(
            Box::leak(call.into_boxed_str()),
            &[
                ("mcp-session-id", session_id.as_str()),
                ("mcp-protocol-version", "2025-11-25"),
            ],
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Drain the SSE body so the tap observes the full exchange.
    let _ = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
        .await
        .unwrap();
    let events = read_trace(&dir, &session_id, 11).await;
    (session_id, marker, events)
}

/// L3 concurrency proof: many sessions recording through ONE tap (one writer
/// task, one channel, one session map) at real parallelism. Every per-file
/// invariant the writer claims "by construction" is asserted over the result:
/// contiguous `seq` from 0, every line a parseable event, and no
/// cross-session bleed — each session's echo argument appears in its own
/// trace and in no other.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sessions_record_isolated_contiguous_traces() {
    const SESSIONS: usize = 16;
    let (app, dir) = tapped_app("stress");

    let workers: Vec<_> = (0..SESSIONS)
        .map(|index| tokio::spawn(stress_session(app.clone(), dir.clone(), index)))
        .collect();
    let mut traces = Vec::new();
    for worker in workers {
        traces.push(worker.await.expect("worker completes"));
    }
    assert_eq!(traces.len(), SESSIONS);

    for (session_id, marker, events) in &traces {
        // Contiguity, not just monotonicity: the writer assigns 0,1,2,… per
        // file, so any gap means an event was lost or misrouted.
        for (index, event) in events.iter().enumerate() {
            assert_eq!(
                event["seq"].as_u64(),
                Some(index as u64),
                "{session_id}: seq must be contiguous from 0"
            );
        }
        let text: Vec<String> = events.iter().map(ToString::to_string).collect();
        let own = text
            .iter()
            .filter(|line| line.contains(marker.as_str()))
            .count();
        assert!(
            own >= 2,
            "{session_id}: its own echo call and result must be recorded, found {own}"
        );
        for (other_id, other_marker, _) in &traces {
            if other_id != session_id {
                assert!(
                    !text.iter().any(|line| line.contains(other_marker.as_str())),
                    "{session_id}: contains {other_id}'s marker {other_marker} — cross-session bleed"
                );
            }
        }
        // And the real reader accepts every file the writer produced.
        let bytes = raw_trace_bytes(&dir, session_id);
        let parsed = mcp_trace_validator::reader::parse_trace(
            &bytes,
            &mcp_trace_validator::reader::Limits::default(),
        )
        .expect("every concurrently written trace parses");
        assert!(parsed.len() >= 11);
    }

    let _ = std::fs::remove_dir_all(dir);
}
