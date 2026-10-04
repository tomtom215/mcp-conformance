// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a client sees, and what the trace says, when the upstream fails: before
//! it answers (the proxy's own 502, naming the upstream and the cause) and after
//! (the truncation, relayed as the client would have seen it directly).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mcp_trace_capture::{Recorder, http};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

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

impl Shared {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

/// An upstream that sends `response` to every request and closes the
/// connection, mid-body if `response` says more is coming.
async fn cutting_upstream(response: &'static [u8]) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut byte = [0_u8; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read(&mut byte).await.unwrap_or(0) == 0 {
                        return;
                    }
                    request.push(byte[0]);
                }
                stream.write_all(response).await.unwrap();
                tokio::time::sleep(Duration::from_millis(100)).await;
            });
        }
    });
    address
}

async fn proxy(
    upstream: String,
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
    let options = http::Options::new(upstream.parse().unwrap(), 1024).unwrap();
    let task = tokio::spawn(http::serve(listener, recorder, options, async {
        stopped.await.ok();
    }));
    (address, sink, move || {
        stop.send(()).ok();
        task
    })
}

/// The upstream answers 200 and dies mid-body. Directly, a client gets the 200
/// and a truncated body; through the proxy it must get the same — not a 502 the
/// proxy made up — and the trace must say the transport aborted, not record a
/// 200 the client then never completed reading as if all were well.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_response_cut_off_mid_body_reaches_the_client_as_a_truncation() {
    bounded(async {
        let upstream = cutting_upstream(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\r\n{\"jsonrpc\":",
        )
        .await;
        let (address, sink, stop) = proxy(format!("http://{upstream}")).await;
        let response = reqwest::Client::new()
            .post(format!("http://{address}/mcp"))
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "the upstream's status, relayed");
        assert!(
            response.bytes().await.is_err(),
            "the body ends early, as it did upstream"
        );
        let unrecorded = stop().await.unwrap().unwrap();
        assert_eq!(unrecorded.upstream_cut, 1, "{unrecorded:?}");
        assert_eq!(unrecorded.upstream_failures, 0, "{unrecorded:?}");
        let text = sink.text();
        assert!(!text.contains(r#""status":502"#), "{text}");
        let events: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let status = events
            .iter()
            .position(|event| event["status"] == 200)
            .unwrap();
        assert_eq!(events[status + 1]["event"], "transport-abort", "{text}");
    })
    .await;
}

/// The same for an event stream: the events that arrived are relayed and
/// recorded, and the cut is recorded as an abort.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_event_stream_cut_off_is_recorded_as_an_abort() {
    bounded(async {
        let upstream = cutting_upstream(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n\
              26\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"a\"}\n\n\r\n10\r\ndata: {\"jsonrpc\"",
        )
        .await;
        let (address, sink, stop) = proxy(format!("http://{upstream}")).await;
        let response = reqwest::Client::new()
            .get(format!("http://{address}/mcp"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(response.bytes().await.is_err(), "the stream ends early");
        let unrecorded = stop().await.unwrap().unwrap();
        assert_eq!(unrecorded.upstream_cut, 1, "{unrecorded:?}");
        let text = sink.text();
        let message = text.find(r#""method":"a""#).expect("the whole event is recorded");
        let abort = text.find("transport-abort").expect("the cut is recorded");
        assert!(message < abort, "{text}");
    })
    .await;
}

/// An upstream that cannot be reached is the one case the proxy answers itself;
/// its 502 says which upstream and why, rather than only "unreachable".
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_502_for_an_unreachable_upstream_names_it_and_the_cause() {
    bounded(async {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let (address, _sink, stop) = proxy(format!("http://127.0.0.1:{port}/base")).await;
        let response = reqwest::Client::new()
            .post(format!("http://{address}/mcp?token=secret"))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 502);
        let body = response.text().await.unwrap();
        assert!(
            body.contains(&format!("http://127.0.0.1:{port}/base/mcp")),
            "{body}"
        );
        assert!(body.to_lowercase().contains("refused"), "the cause: {body}");
        assert!(!body.contains("secret"), "the query is not echoed: {body}");
        assert_eq!(stop().await.unwrap().unwrap().upstream_failures, 1);
    })
    .await;
}
