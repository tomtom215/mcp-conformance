// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the proxy leaves out of the trace rather than record wrongly: JSON
//! whose parsed value would not say what was sent, and SSE events no MCP client
//! reads as messages. Each is forwarded unchanged and counted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::response::Response;
use mcp_trace_capture::{Recorder, http};

async fn bounded(body: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(20), body)
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

const STREAM: &str = "event: ping\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"typed\"}\n\n\
                      data: {\"jsonrpc\":\"2.0\",\"method\":\"big\",\"params\":{\"n\":123456789012345678901234567890}}\n\n\
                      event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n\n";
const ALTERED_BODY: &str = r#"{"jsonrpc":"2.0","id":2,"id":3,"result":{}}"#;

/// An upstream answering `POST /sse` with [`STREAM`], an event per chunk (so
/// the counts must add up across chunks), and `POST /json` with
/// [`ALTERED_BODY`].
async fn upstream() -> std::net::SocketAddr {
    let answer = |content_type: &'static str, body: &'static str| {
        Response::builder()
            .header("content-type", content_type)
            .body(axum::body::Body::from(body))
            .unwrap()
    };
    let events = || {
        let chunks = STREAM.split_inclusive("\n\n").map(str::to_owned);
        let stream = futures::StreamExt::then(futures::stream::iter(chunks), |chunk| async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<_, std::convert::Infallible>(chunk)
        });
        Response::builder()
            .header("content-type", "text/event-stream")
            .body(axum::body::Body::from_stream(stream))
            .unwrap()
    };
    let app = axum::Router::new()
        .route("/sse", axum::routing::post(move || async move { events() }))
        .route(
            "/json",
            axum::routing::post(move || async move { answer("application/json", ALTERED_BODY) }),
        );
    let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = upstream.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(upstream, app).await.unwrap() });
    address
}

/// POSTs a request to `url` and returns the response body.
async fn post(url: String) -> String {
    reqwest::Client::new()
        .post(url)
        .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn altered_json_and_typed_events_are_forwarded_and_counted_not_recorded() {
    bounded(async {
        let upstream_address = upstream().await;
        let sink = Shared::default();
        let recorder = Arc::new(Recorder::new(sink.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let options = http::Options::new(
            format!("http://{upstream_address}").parse().unwrap(),
            1 << 20,
        )
        .unwrap();
        let serving = tokio::spawn(http::serve(listener, recorder, options, async {
            stopped.await.ok();
        }));
        assert_eq!(post(format!("http://{address}/sse")).await, STREAM);
        assert_eq!(post(format!("http://{address}/json")).await, ALTERED_BODY);
        stop.send(()).unwrap();
        let unrecorded = serving.await.unwrap().unwrap();
        assert_eq!((unrecorded.altered, unrecorded.other_events), (2, 1));
        assert_eq!((unrecorded.not_json, unrecorded.oversized), (0, 0));

        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let payloads: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .filter(|event| event["kind"] == "message" && event["direction"] == "server-to-client")
            .map(|event| event["payload"].clone())
            .collect();
        assert_eq!(
            payloads,
            [serde_json::json!({"jsonrpc": "2.0", "id": 1, "result": {}})],
            "{text}"
        );
    })
    .await;
}
