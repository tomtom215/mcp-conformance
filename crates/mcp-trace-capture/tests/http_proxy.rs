// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The HTTP recorder over real sockets: a mock upstream for the proxy's own
//! guarantees, and the real everything server for the end-to-end claim that a
//! recorded session is one the validator judges clean.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::routing::post;
use mcp_conformance_core::trace::{Direction, EventBody, TraceEvent};
use mcp_trace_capture::{Recorder, http};

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
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
    fn events(&self) -> Vec<TraceEvent> {
        self.text()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

/// Plain HTTP to loopback.
fn client() -> reqwest::Client {
    reqwest::Client::new()
}

async fn serve(app: axum::Router) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// Starts the proxy in front of `upstream`; returns its address, the trace, and a
/// handle that stops it and yields its counters.
async fn proxy(
    upstream: String,
    max_message: usize,
) -> (
    SocketAddr,
    Shared,
    impl FnOnce() -> tokio::task::JoinHandle<std::io::Result<http::Unrecorded>>,
) {
    let sink = Shared::default();
    let recorder = Arc::new(Recorder::new(sink.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let options = http::Options::new(upstream.parse().unwrap(), max_message).unwrap();
    let task = tokio::spawn(http::serve(listener, recorder, options, async {
        stopped.await.ok();
    }));
    (address, sink, move || {
        stop.send(()).ok();
        task
    })
}

const SSE: &str = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"text\":\"é日本\"}}\n\n: keep-alive\n\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n\n";

/// Two events over the proxy's 1024-byte limit, each in its own chunk, around one
/// that fits.
fn sse_big_chunks() -> Vec<String> {
    let big = |n: u8| format!("data: {{\"n\":{n},\"pad\":\"{}\"}}\n\n", "x".repeat(2000));
    vec![big(1), "data: {\"n\":2}\n\n".to_owned(), big(3)]
}

/// The routes that exercise the proxy's message limit.
fn limit_routes() -> axum::Router {
    axum::Router::new()
        .route(
            "/mcp/sse-big",
            post(|| async {
                let chunks: Vec<Result<String, std::io::Error>> =
                    sse_big_chunks().into_iter().map(Ok).collect();
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures::stream::iter(chunks)))
                    .unwrap()
            }),
        )
        .route(
            "/mcp/exact",
            post(|| async {
                // Exactly the proxy's 1024-byte limit: recorded, not oversized.
                let body = format!("{{\"pad\":\"{}\"}}", "x".repeat(1024 - 10));
                assert_eq!(body.len(), 1024);
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap()
            }),
        )
}

fn mock_upstream(seen_authorization: Arc<Mutex<Vec<String>>>) -> axum::Router {
    axum::Router::new()
        .route(
            "/mcp/json",
            post(move |headers: HeaderMap, body: String| async move {
                if let Some(value) = headers.get("authorization") {
                    seen_authorization
                        .lock()
                        .unwrap()
                        .push(value.to_str().unwrap().to_owned());
                }
                let request: serde_json::Value = serde_json::from_str(&body).unwrap();
                Response::builder()
                    .header("content-type", "application/json")
                    .header("mcp-session-id", "s-1")
                    .body(Body::from(
                        serde_json::json!({"jsonrpc": "2.0", "id": request["id"], "result": {}})
                            .to_string(),
                    ))
                    .unwrap()
            }),
        )
        .route(
            "/mcp/sse",
            post(|| async {
                // Split inside a multi-byte character and inside a line ending.
                let bytes = SSE.as_bytes().to_vec();
                let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
                    bytes.chunks(7).map(|chunk| Ok(chunk.to_vec())).collect();
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures::stream::iter(chunks)))
                    .unwrap()
            }),
        )
        .route("/mcp/accepted", post(|| async { StatusCode::ACCEPTED }))
        .merge(limit_routes())
        .route(
            "/mcp/big",
            post(|| async {
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(format!("{{\"blob\":\"{}\"}}", "x".repeat(5000))))
                    .unwrap()
            }),
        )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[allow(clippy::too_many_lines)] // One proxy run, asserted across every response shape.
async fn the_proxy_relays_bytes_unchanged_and_records_what_the_validator_reads() {
    let authorization = Arc::new(Mutex::new(Vec::new()));
    let upstream = serve(mock_upstream(Arc::clone(&authorization))).await;
    let (address, sink, stop) = proxy(format!("http://{upstream}"), 1024).await;
    let client = client();
    let url = |path: &str| format!("http://{address}/mcp/{path}");

    let json = client
        .post(url("json"))
        .header("authorization", "Bearer do-not-record")
        .header("content-type", "application/json")
        .header("mcp-protocol-version", "2025-11-25")
        .body(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(json.status(), 200);
    assert_eq!(json.headers()["mcp-session-id"], "s-1");
    assert_eq!(
        json.text().await.unwrap(),
        r#"{"id":7,"jsonrpc":"2.0","result":{}}"#
    );

    let sse = client.post(url("sse")).body("{}").send().await.unwrap();
    assert_eq!(
        sse.bytes().await.unwrap(),
        SSE.as_bytes(),
        "SSE bytes relayed unchanged"
    );

    let accepted = client
        .post(url("accepted"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), 202);

    let big = client.post(url("big")).body("{}").send().await.unwrap();
    assert_eq!(
        big.text().await.unwrap().len(),
        5000 + 11,
        "oversized body relayed intact"
    );

    let sse_big = client.post(url("sse-big")).body("{}").send().await.unwrap();
    assert_eq!(
        sse_big.bytes().await.unwrap(),
        sse_big_chunks().concat().as_bytes(),
        "relayed intact"
    );
    let exact = client.post(url("exact")).body("{}").send().await.unwrap();
    assert_eq!(exact.bytes().await.unwrap().len(), 1024);

    let unrecorded = stop().await.unwrap().unwrap();
    // /big, and the two oversized SSE events; /exact is at the limit, not over it.
    assert_eq!(
        (
            unrecorded.oversized,
            unrecorded.not_json,
            unrecorded.upstream_failures
        ),
        (3, 0, 0)
    );
    let text = sink.text();
    assert!(
        text.contains(r#"{"n":2}"#),
        "the event that fits is recorded"
    );
    assert!(!text.contains(r#""n":1"#) && !text.contains(r#""n":3"#));
    assert!(
        text.contains(&"x".repeat(1014)),
        "the exact-limit body is recorded"
    );

    // Credentials reach the server and never the trace.
    assert_eq!(*authorization.lock().unwrap(), ["Bearer do-not-record"]);
    assert!(!sink.text().contains("do-not-record"));

    let events = sink.events();
    let messages: Vec<&serde_json::Value> = events
        .iter()
        .filter_map(|event| match &event.body {
            EventBody::Message { payload } => Some(payload),
            _ => None,
        })
        .collect();
    // ping and its result; the `{}` request to /sse and its two events; the `{}`
    // requests to /accepted (answered with no body) and /big (answered with a
    // body over the limit, so not recorded); the `{}` request to /sse-big and
    // the one event of its three that fits; the `{}` request to /exact and its
    // body at the limit.
    assert_eq!(messages.len(), 11, "{messages:#?}");
    assert_eq!(messages[1]["id"], 7);
    assert_eq!(messages[3]["result"]["text"], "é日本");
    assert_eq!(messages[4]["method"], "notifications/message");
    // seq is dense and in file order, and every request precedes its response.
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event.seq, index as u64);
    }
    let first_response = events
        .iter()
        .position(|event| event.direction == Direction::ServerToClient)
        .unwrap();
    assert!(
        matches!(events[0].body, EventBody::Http { ref method, .. } if method.as_deref() == Some("POST"))
    );
    assert!(first_response > 1);
    assert!(sink.text().contains(r#""status":202"#));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unreachable_upstream_is_a_502_recorded_as_a_transport_abort() {
    // Bind and drop, so nothing is listening on this port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (address, sink, stop) = proxy(format!("http://127.0.0.1:{port}"), 1024).await;
    let response = client()
        .post(format!("http://{address}/mcp"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 502);
    assert_eq!(stop().await.unwrap().unwrap().upstream_failures, 1);
    let text = sink.text();
    assert!(text.contains(r#""event":"transport-abort""#), "{text}");
    assert!(
        !text.contains(r#""status":502"#),
        "the proxy's 502 is not the server's response"
    );
}

/// The end-to-end claim: a real client-to-server session recorded through the
/// proxy is judged by the real validator, against the revision it declares, with
/// no failure.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_session_with_the_everything_server_records_a_trace_the_validator_passes() {
    use mcp_everything_server::policy::HttpSecurityPolicy;
    use mcp_everything_server::server::ServedRevision;

    let server = serve(mcp_everything_server::http::router(
        HttpSecurityPolicy::default(),
        ServedRevision::V2026_07_28,
    ))
    .await;
    let (address, sink, stop) = proxy(format!("http://{server}"), 1 << 20).await;
    let client = client();
    let meta = r#""_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"capture-test","version":"0.0.0"}}"#;
    for (id, method, name, extra) in [
        (1, "server/discover", None, ""),
        (2, "tools/list", None, ""),
        (
            3,
            "tools/call",
            Some("echo"),
            r#","name":"echo","arguments":{"message":"hi"}"#,
        ),
    ] {
        let mut request = client
            .post(format!("http://{address}/mcp"))
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", method)
            .body(format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{{meta}{extra}}}}}"#
            ));
        if let Some(name) = name {
            request = request.header("mcp-name", name);
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), 200, "{method}");
        let body = response.text().await.unwrap();
        assert!(body.contains(&format!(r#""id":{id}"#)), "{method}: {body}");
    }
    stop().await.unwrap().unwrap();

    let events = sink.events();
    let set = mcp_conformance_core::requirement::RegistrySet::builtin().unwrap();
    let selection = mcp_trace_validator::declared::select(set.revisions(), &events).unwrap();
    assert_eq!(selection.revisions, ["2026-07-28".parse().unwrap()]);
    let registry = set.registry(selection.revisions[0]).unwrap();
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    assert!(
        !report.has_errors(),
        "{}\ntrace:\n{}",
        report.render_human(),
        sink.text()
    );
    assert!(report.totals.pass > 10, "{}", report.render_human());
}
