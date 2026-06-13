// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Scripted-seam proofs for the resumption dance: fakes implementing
//! `StreamableHttpClient` drive `drive` through the scenario's exact frame
//! shapes — priming event, graceful close, resumed delivery — with the
//! reconnect instant and offered `Last-Event-ID` recorded for assertion.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use futures::stream;
use std::sync::Mutex;
use std::time::Instant;

/// A scripted seam: answers each POST in order, then serves the GET
/// reconnect — recording when it happened and with which Last-Event-ID.
#[derive(Clone, Default)]
struct ScriptedClient {
    state: Arc<ScriptedState>,
}

#[derive(Default)]
struct ScriptedState {
    posts: Mutex<Vec<String>>,
    close_instant: Mutex<Option<Instant>>,
    get_instant: Mutex<Option<Instant>>,
    /// One entry per GET, carrying the `Last-Event-ID` it offered.
    gets: Mutex<Vec<Option<String>>>,
}

fn sse(id: Option<&str>, retry: Option<u64>, data: Option<&str>) -> sse_stream::Sse {
    sse_stream::Sse {
        id: id.map(ToOwned::to_owned),
        retry,
        data: data.map(ToOwned::to_owned),
        ..Default::default()
    }
}

impl StreamableHttpClient for ScriptedClient {
    type Error = std::io::Error;

    async fn post_message(
        &self,
        _uri: Arc<str>,
        message: ClientJsonRpcMessage,
        _session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<
        StreamableHttpPostResponse,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        let value = serde_json::to_value(&message).unwrap();
        let method = value["method"].as_str().unwrap_or("").to_owned();
        self.state.posts.lock().unwrap().push(method.clone());
        match method.as_str() {
            "initialize" => Ok(StreamableHttpPostResponse::Json(
                serde_json::from_value(serde_json::json!({
                    "jsonrpc": "2.0", "id": 1,
                    "result": {
                        "protocolVersion": "2025-03-26",
                        "serverInfo": {"name": "fake", "version": "0"},
                        "capabilities": {},
                    },
                }))
                .unwrap(),
                Some("session-1".to_owned()),
            )),
            "notifications/initialized" => Ok(StreamableHttpPostResponse::Accepted),
            "tools/call" => {
                // Priming frame names retry + event id; the stream then
                // closes WITHOUT the result — the scenario's exact shape.
                let state = Arc::clone(&self.state);
                let frames = stream::iter(vec![Ok(sse(Some("event-7"), Some(120), Some("")))])
                    .chain(stream::poll_fn(move |_| {
                        *state.close_instant.lock().unwrap() = Some(Instant::now());
                        std::task::Poll::Ready(None)
                    }));
                Ok(StreamableHttpPostResponse::Sse(frames.boxed(), None))
            }
            other => panic!("unexpected POST {other}"),
        }
    }

    async fn delete_session(
        &self,
        _uri: Arc<str>,
        _session_id: Arc<str>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<(), rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>> {
        Ok(())
    }

    async fn get_stream(
        &self,
        _uri: Arc<str>,
        _session_id: Arc<str>,
        last_event_id: Option<String>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<sse_stream::Sse, sse_stream::Error>>,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        *self.state.get_instant.lock().unwrap() = Some(Instant::now());
        self.state.gets.lock().unwrap().push(last_event_id);
        let result = serde_json::json!({
            "jsonrpc": "2.0", "id": CALL_ID,
            "result": {
                "content": [
                    {"type": "text", "text": "Reconnection test completed successfully"},
                ],
            },
        });
        Ok(stream::iter(vec![Ok(sse(
            Some("event-8"),
            None,
            Some(&result.to_string()),
        ))])
        .boxed())
    }
}

#[tokio::test]
async fn dance_honors_retry_and_offers_last_event_id() {
    let client = ScriptedClient::default();
    let state = Arc::clone(&client.state);
    let report = drive(client, "http://127.0.0.1:1/").await.unwrap();

    assert_eq!(
        report.tool_result_text,
        "Reconnection test completed successfully"
    );
    assert_eq!(
        report.waited,
        Duration::from_millis(120),
        "server-named delay"
    );
    assert_eq!(report.last_event_id.as_deref(), Some("event-7"));

    // The wire order the scenario expects.
    assert_eq!(
        *state.posts.lock().unwrap(),
        ["initialize", "notifications/initialized", "tools/call"]
    );
    // Exactly one GET reconnect, offering the priming event's id…
    assert_eq!(*state.gets.lock().unwrap(), [Some("event-7".to_owned())]);
    // …and respected the retry timing: not early (the scenario fails
    // anything under retry − 50 ms). The upper bound is left to the real
    // measurement — a sleepy test runner must not flake this suite.
    let closed = state.close_instant.lock().unwrap().unwrap();
    let reconnected = state.get_instant.lock().unwrap().unwrap();
    assert!(
        reconnected.duration_since(closed) >= Duration::from_millis(70),
        "reconnect must wait the named retry (120 ms − 50 ms tolerance), got {:?}",
        reconnected.duration_since(closed)
    );
}

#[tokio::test]
// Over the 60-line threshold because the inline fake must spell out all
// three trait signatures; moving it out of the test would hide the scripted
// behavior the test is about.
#[allow(clippy::too_many_lines)]
async fn immediate_json_result_needs_no_resumption() {
    #[derive(Clone)]
    struct ImmediateClient;
    impl StreamableHttpClient for ImmediateClient {
        type Error = std::io::Error;
        async fn post_message(
            &self,
            _uri: Arc<str>,
            message: ClientJsonRpcMessage,
            _session_id: Option<Arc<str>>,
            _auth_header: Option<String>,
            _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
        ) -> Result<
            StreamableHttpPostResponse,
            rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
        > {
            let value = serde_json::to_value(&message).unwrap();
            let reply = match value["method"].as_str().unwrap_or("") {
                "initialize" => serde_json::json!({
                    "jsonrpc": "2.0", "id": 1,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "serverInfo": {"name": "f", "version": "0"},
                        "capabilities": {},
                    },
                }),
                "notifications/initialized" => {
                    return Ok(StreamableHttpPostResponse::Accepted);
                }
                _ => serde_json::json!({
                    "jsonrpc": "2.0", "id": CALL_ID,
                    "result": {
                        "content": [{"type": "text", "text": "no resumption needed"}],
                    },
                }),
            };
            Ok(StreamableHttpPostResponse::Json(
                serde_json::from_value(reply).unwrap(),
                None,
            ))
        }
        async fn delete_session(
            &self,
            _uri: Arc<str>,
            _session_id: Arc<str>,
            _auth_header: Option<String>,
            _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
        ) -> Result<(), rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>>
        {
            Ok(())
        }
        async fn get_stream(
            &self,
            _uri: Arc<str>,
            _session_id: Arc<str>,
            _last_event_id: Option<String>,
            _auth_header: Option<String>,
            _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
        ) -> Result<
            futures::stream::BoxStream<'static, Result<sse_stream::Sse, sse_stream::Error>>,
            rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
        > {
            panic!("an immediate JSON result must not trigger a GET");
        }
    }

    let report = drive(ImmediateClient, "http://127.0.0.1:1/").await.unwrap();
    assert_eq!(report.tool_result_text, "no resumption needed");
    assert_eq!(report.waited, Duration::ZERO);
    assert_eq!(report.last_event_id, None);
}

#[test]
fn errors_name_the_failed_step() {
    let rendered = ResumeError::Post("tools/call", "boom".to_owned()).to_string();
    assert!(rendered.contains("tools/call") && rendered.contains("boom"));
    let rendered = ResumeError::DeadlineElapsed("resumed GET stream").to_string();
    assert!(rendered.contains("resumed GET stream"));
}

/// A seam whose scripted answers come from a table: each POST method maps to
/// a fixed response shape. GETs panic — these fakes exercise pre-reconnect
/// failure shapes only.
#[derive(Clone)]
struct ShapeClient {
    /// `(method, response)` pairs; a POST with an unlisted method panics.
    table: Arc<Vec<(String, ShapeResponse)>>,
}

#[derive(Clone)]
enum ShapeResponse {
    Accepted,
    /// An initialize-style result framed as one SSE message, then close.
    SseResult {
        id: u64,
        session: Option<String>,
    },
}

impl StreamableHttpClient for ShapeClient {
    type Error = std::io::Error;

    async fn post_message(
        &self,
        _uri: Arc<str>,
        message: ClientJsonRpcMessage,
        _session_id: Option<Arc<str>>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<
        StreamableHttpPostResponse,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        let value = serde_json::to_value(&message).unwrap();
        let method = value["method"].as_str().unwrap_or("").to_owned();
        let response = self
            .table
            .iter()
            .find(|(name, _)| *name == method)
            .map_or_else(
                || panic!("unexpected POST {method}"),
                |(_, response)| response.clone(),
            );
        match response {
            ShapeResponse::Accepted => Ok(StreamableHttpPostResponse::Accepted),
            ShapeResponse::SseResult { id, session } => {
                let result = serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {
                        "protocolVersion": "2025-11-25",
                        "serverInfo": {"name": "f", "version": "0"},
                        "capabilities": {},
                    },
                });
                Ok(StreamableHttpPostResponse::Sse(
                    stream::iter(vec![Ok(sse(None, None, Some(&result.to_string())))]).boxed(),
                    session,
                ))
            }
        }
    }

    async fn delete_session(
        &self,
        _uri: Arc<str>,
        _session_id: Arc<str>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<(), rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>> {
        Ok(())
    }

    async fn get_stream(
        &self,
        _uri: Arc<str>,
        _session_id: Arc<str>,
        _last_event_id: Option<String>,
        _auth_header: Option<String>,
        _custom_headers: HashMap<http::HeaderName, http::HeaderValue>,
    ) -> Result<
        futures::stream::BoxStream<'static, Result<sse_stream::Sse, sse_stream::Error>>,
        rmcp::transport::streamable_http_client::StreamableHttpError<Self::Error>,
    > {
        panic!("these shape fakes never reach the GET reconnect");
    }
}

fn shape_client(table: Vec<(&str, ShapeResponse)>) -> ShapeClient {
    ShapeClient {
        table: Arc::new(
            table
                .into_iter()
                .map(|(name, response)| (name.to_owned(), response))
                .collect(),
        ),
    }
}

#[tokio::test]
async fn initialize_answered_over_sse_still_completes_the_handshake() {
    // A spec-shaped server may frame the initialize result as SSE; the
    // handshake must drain it and proceed (deleting that match arm would
    // collapse this into "unknown variant").
    let client = shape_client(vec![
        (
            "initialize",
            ShapeResponse::SseResult {
                id: 1,
                session: Some("sse-session".to_owned()),
            },
        ),
        ("notifications/initialized", ShapeResponse::Accepted),
        // The call itself is answered as an immediate result so the dance
        // ends without a reconnect.
        (
            "tools/call",
            ShapeResponse::SseResult {
                id: CALL_ID,
                session: None,
            },
        ),
    ]);
    let report = drive(client, "http://127.0.0.1:1/").await.unwrap();
    assert_eq!(report.waited, Duration::ZERO);
}

#[tokio::test]
async fn accepted_initialize_is_named_as_the_wrong_shape() {
    let client = shape_client(vec![("initialize", ShapeResponse::Accepted)]);
    let error = drive(client, "http://127.0.0.1:1/").await.unwrap_err();
    let rendered = error.to_string();
    assert!(
        rendered.contains("initialize") && rendered.contains("202 Accepted"),
        "the error names the step and the shape: {rendered}"
    );
}

#[tokio::test]
async fn accepted_tools_call_is_named_as_the_wrong_shape() {
    let client = shape_client(vec![
        (
            "initialize",
            ShapeResponse::SseResult {
                id: 1,
                session: None,
            },
        ),
        ("notifications/initialized", ShapeResponse::Accepted),
        ("tools/call", ShapeResponse::Accepted),
    ]);
    let error = drive(client, "http://127.0.0.1:1/").await.unwrap_err();
    let rendered = error.to_string();
    assert!(
        rendered.contains("tools/call") && rendered.contains("202 Accepted"),
        "the error names the step and the shape: {rendered}"
    );
}

#[tokio::test(start_paused = true)]
async fn deadline_is_in_the_future_not_the_past() {
    // Pinned with paused time: a frame arriving *before* STEP_DEADLINE is
    // accepted. The `now + STEP_DEADLINE` → `now - STEP_DEADLINE` mutant
    // puts the deadline in the past, so a stream that is pending on first
    // poll times out instantly instead of waiting for its frame — invisible
    // to the instant-ready fakes above, decisive here.
    let result = serde_json::json!({"jsonrpc": "2.0", "id": 9, "result": {}});
    let frame = sse(None, None, Some(&result.to_string()));
    let mut stream = stream::once(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        Ok(frame)
    })
    .boxed();
    let message = read_message_with_id(&mut stream, 9, &mut None, &mut None)
        .await
        .expect("a frame inside the deadline is read, not timed out")
        .expect("the frame carries the wanted id");
    assert_eq!(message["id"], 9);
}
