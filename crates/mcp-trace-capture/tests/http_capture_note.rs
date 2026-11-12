// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the proxy's trace says about itself: a body that is not JSON is in the
//! trace as the string it was, so the validator judges it, and whatever the
//! trace still cannot hold is counted in its closing event, so the account
//! outlives the terminal it was first printed on.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse as _, Response};
use mcp_conformance_core::trace::{CaptureNote, Direction, EventBody, LifecycleEvent, TraceEvent};
use mcp_trace_capture::{Recorder, http};
use serde_json::Value;

/// The message limit the proxy under test records up to.
const LIMIT: usize = 1024;

async fn bounded(body: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(std::time::Duration::from_secs(20), body)
        .await
        .expect("the test finished within 20 s");
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Shared {
    fn events(&self) -> Vec<TraceEvent> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

/// An upstream whose answer is chosen by the request path.
async fn upstream(request: Uri, _headers: HeaderMap) -> Response {
    let json = [(header::CONTENT_TYPE, "application/json")];
    let sse = [(header::CONTENT_TYPE, "text/event-stream")];
    let result = r#"{"jsonrpc":"2.0","id":1,"result":{}}"#;
    let big = "x".repeat(2000);
    match request.path() {
        "/clean" => (StatusCode::OK, json, result).into_response(),
        "/truncated" => (
            StatusCode::OK,
            json,
            r#"{"jsonrpc":"2.0","id":3,"result":{"#,
        )
            .into_response(),
        "/accepted-with-text" => (
            StatusCode::ACCEPTED,
            [(header::CONTENT_TYPE, "text/plain")],
            "Accepted",
        )
            .into_response(),
        "/missing-page" => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/html")],
            "<html>Not Found</html>",
        )
            .into_response(),
        "/error-claiming-json" => {
            (StatusCode::INTERNAL_SERVER_ERROR, json, r#"{"error":"#).into_response()
        }
        "/sse-broken" => (
            StatusCode::OK,
            sse,
            format!("data: {{broken\n\ndata: {result}\n\n"),
        )
            .into_response(),
        "/sse-mixed" => (
            StatusCode::OK,
            sse,
            format!(
                "event: ping\ndata: 1\n\ndata: {big}\n\nevent: endpoint\ndata: /x\n\n\
                 event: ping\ndata: 2\n\ndata: {big}\n\ndata: {result}\n\n"
            ),
        )
            .into_response(),
        "/big" => (StatusCode::OK, json, big).into_response(),
        // Events arriving over time, one to a chunk: what the parser counts it
        // adds up across chunks, which one body in one chunk never exercises.
        "/sse-chunked" => {
            let chunks = vec![
                format!("data: {big}\n\n"),
                "event: ping\ndata: 1\n\n".to_owned(),
                format!("data: {big}\n\n"),
                "event: ping\ndata: 2\n\n".to_owned(),
                format!("data: {big}\n\n"),
            ];
            let stream = futures::StreamExt::then(futures::stream::iter(chunks), |chunk| async {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                Ok::<_, std::convert::Infallible>(chunk)
            });
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(axum::body::Body::from_stream(stream))
                .unwrap()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

type Stop = Box<dyn FnOnce() -> tokio::task::JoinHandle<std::io::Result<http::Unrecorded>>>;

/// The proxy, recording into the returned sink, in front of [`upstream`].
async fn start(adjust: impl FnOnce(&mut http::Options)) -> (SocketAddr, Shared, Stop) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, axum::Router::new().fallback(upstream))
            .await
            .unwrap();
    });
    let sink = Shared::default();
    let recorder = Arc::new(Recorder::new(sink.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let mut options =
        http::Options::new(format!("http://{upstream_address}").parse().unwrap(), LIMIT).unwrap();
    adjust(&mut options);
    let task = tokio::spawn(http::serve(listener, recorder, options, async {
        stopped.await.ok();
    }));
    (
        address,
        sink,
        Box::new(move || {
            stop.send(()).ok();
            task
        }),
    )
}

/// POSTs `body` to `path` through the proxy and reads the response out.
async fn post(address: SocketAddr, path: &str, body: &str) -> (u16, String) {
    let response = reqwest::Client::new()
        .post(format!("http://{address}{path}"))
        .body(body.to_owned())
        .send()
        .await
        .unwrap();
    (
        response.status().as_u16(),
        response.text().await.unwrap_or_default(),
    )
}

const PING: &str = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;

/// The payloads of the trace's message events in `direction`.
fn payloads(events: &[TraceEvent], direction: Direction) -> Vec<Value> {
    events
        .iter()
        .filter(|event| event.direction == direction)
        .filter_map(|event| event.message_payload().cloned())
        .collect()
}

/// What the closing event says, and that it is the trace's last event.
fn note_of(events: &[TraceEvent]) -> &CaptureNote {
    let last = events.last().unwrap();
    assert!(
        matches!(
            last.body,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose
            }
        ),
        "{last:?}"
    );
    last.capture
        .as_ref()
        .expect("the closing event carries a note")
}

fn left_out(events: &[TraceEvent]) -> Vec<(String, u64)> {
    note_of(events)
        .left_out()
        .map(|(kind, count)| (kind.to_owned(), count))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_that_left_nothing_out_says_so_on_its_closing_event() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        assert_eq!(post(address, "/clean", PING).await.0, 200);
        stop().await.unwrap().unwrap();
        let events = sink.events();
        assert_eq!(note_of(&events), &CaptureNote::default());
        assert!(
            events[..events.len() - 1]
                .iter()
                .all(|event| event.capture.is_none()),
            "only the closing event carries a note"
        );
    })
    .await;
}

/// The audit's A02: a truncated 200 body reached the client, was left out of
/// the trace, and the validator said `pass`. It is in the trace as the string
/// it was, which the validator convicts, as it does the same line on stdio.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_2xx_body_that_is_not_json_is_recorded_as_the_string_it_was() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        for (path, text) in [
            ("/truncated", r#"{"jsonrpc":"2.0","id":3,"result":{"#),
            ("/accepted-with-text", "Accepted"),
        ] {
            let (_, relayed) = post(address, path, PING).await;
            assert_eq!(relayed, text, "relayed unchanged");
        }
        let unrecorded = stop().await.unwrap().unwrap();
        let events = sink.events();
        assert_eq!(
            payloads(&events, Direction::ServerToClient),
            [
                Value::String(r#"{"jsonrpc":"2.0","id":3,"result":{"#.to_owned()),
                Value::String("Accepted".to_owned())
            ]
        );
        assert_eq!(left_out(&events), [], "recorded, so not left out");
        assert_eq!(unrecorded.not_json, 2, "and still said on the terminal");

        let set = mcp_conformance_core::requirement::RegistrySet::builtin().unwrap();
        let registry = set.registry("2025-11-25".parse().unwrap()).unwrap();
        let report = mcp_trace_validator::engine::validate(&registry, &events);
        assert!(report.has_errors(), "{}", report.render_human());
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_sse_event_that_is_not_json_is_recorded_as_the_string_it_was() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        post(address, "/sse-broken", PING).await;
        stop().await.unwrap().unwrap();
        let events = sink.events();
        assert_eq!(
            payloads(&events, Direction::ServerToClient),
            [
                Value::String("{broken".to_owned()),
                serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}})
            ]
        );
        assert_eq!(left_out(&events), []);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_body_that_is_not_json_is_recorded_as_the_string_it_was() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        post(address, "/clean", "not json").await;
        stop().await.unwrap().unwrap();
        assert_eq!(
            payloads(&sink.events(), Direction::ClientToServer),
            [Value::String("not json".to_owned())]
        );
    })
    .await;
}

/// An error page is not a message: a server's 404 for a path it does not
/// serve is no JSON-RPC and not a defect of the session.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_error_page_that_does_not_claim_to_be_json_is_not_a_message() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        assert_eq!(post(address, "/missing-page", PING).await.0, 404);
        stop().await.unwrap().unwrap();
        let events = sink.events();
        assert_eq!(
            payloads(&events, Direction::ServerToClient),
            Vec::<Value>::new()
        );
        assert_eq!(left_out(&events), [], "an error page is not left out");
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_error_whose_body_claims_to_be_json_is_judged_as_the_string_it_was() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        assert_eq!(post(address, "/error-claiming-json", PING).await.0, 500);
        stop().await.unwrap().unwrap();
        assert_eq!(
            payloads(&sink.events(), Direction::ServerToClient),
            [Value::String(r#"{"error":"#.to_owned())]
        );
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_over_the_limit_is_counted_in_the_closing_event() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        let (_, relayed) = post(address, "/big", PING).await;
        assert_eq!(relayed.len(), 2000, "relayed whole");
        stop().await.unwrap().unwrap();
        let events = sink.events();
        assert_eq!(
            payloads(&events, Direction::ServerToClient),
            Vec::<Value>::new()
        );
        assert_eq!(left_out(&events), [("oversized".to_owned(), 1)]);
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sse_events_over_the_limit_or_of_another_type_are_counted_in_the_closing_event() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        post(address, "/sse-mixed", PING).await;
        stop().await.unwrap().unwrap();
        let events = sink.events();
        assert_eq!(
            payloads(&events, Direction::ServerToClient),
            [serde_json::json!({"jsonrpc":"2.0","id":1,"result":{}})],
            "only the event that fits and is a message"
        );
        assert_eq!(
            left_out(&events),
            [("other_events".to_owned(), 3), ("oversized".to_owned(), 2)]
        );
    })
    .await;
}

/// A message that arrives when the proxy already holds its whole buffer budget
/// is streamed on and not recorded: the closing event counts it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_past_the_buffer_budget_is_counted_in_the_closing_event() {
    bounded(async {
        let (address, sink, stop) = start(|options| options.buffer_budget = 10).await;
        post(address, "/clean", PING).await;
        let unrecorded = stop().await.unwrap().unwrap();
        assert!(unrecorded.over_budget > 0, "the setup spends the budget");
        let events = sink.events();
        let counted = left_out(&events);
        assert!(
            counted.contains(&("over_budget".to_owned(), unrecorded.over_budget)),
            "{counted:?} against {unrecorded:?}"
        );
    })
    .await;
}

/// What the parser has counted only grows, and the note adds what each chunk
/// contributed: three oversized events and two of another type, spread over
/// five chunks, are three and two, not a sum of running totals.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn losses_arriving_over_time_are_counted_once_each() {
    bounded(async {
        let (address, sink, stop) = start(|_| {}).await;
        post(address, "/sse-chunked", PING).await;
        stop().await.unwrap().unwrap();
        assert_eq!(
            left_out(&sink.events()),
            [("other_events".to_owned(), 2), ("oversized".to_owned(), 3)]
        );
    })
    .await;
}
