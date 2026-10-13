// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What bounds the proxy's resources against a client or upstream that does
//! not play along: a client that never finishes its request head, an upstream
//! that never accepts the connection, and many large bodies at once.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

/// The proxy in front of `upstream` with `options` adjusted by `adjust`.
async fn proxy(
    upstream: String,
    adjust: impl FnOnce(&mut http::Options),
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
    let mut options = http::Options::new(upstream.parse().unwrap(), 1024).unwrap();
    adjust(&mut options);
    let task = tokio::spawn(http::serve(listener, recorder, options, async {
        stopped.await.ok();
    }));
    (address, sink, move || {
        stop.send(()).ok();
        task
    })
}

/// A client that sends part of a request head and then nothing (slowloris)
/// had its connection held open for as long as it liked: no read timeout was
/// set. It is now closed once the header read timeout passes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_head_that_never_ends_is_cut_off() {
    bounded(async {
        let (address, _, stop) = proxy("http://127.0.0.1:9".to_owned(), |options| {
            options.header_read_timeout = Duration::from_millis(300);
        })
        .await;
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(b"POST /mcp HTTP/1.1\r\nhost: proxy\r\n")
            .await
            .unwrap();
        let started = Instant::now();
        let mut rest = Vec::new();
        // Closed (with hyper's 408, or reset): either ends the read.
        let _ = tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut rest))
            .await
            .expect("the proxy closed the connection");
        assert!(started.elapsed() >= Duration::from_millis(250));
        stop().await.unwrap().unwrap();
    })
    .await;
}

/// The timeout is on a head's arrival, not on a connection's life: a
/// kept-alive connection serves request after request.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_kept_alive_connection_serves_request_after_request() {
    bounded(async {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let app = axum::Router::new().fallback(|| async { "{}" });
        tokio::spawn(async move { axum::serve(upstream, app).await.unwrap() });
        let (address, _, stop) = proxy(format!("http://{upstream_address}"), |options| {
            options.header_read_timeout = Duration::from_millis(500);
        })
        .await;
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        for _ in 0..3 {
            stream
                .write_all(b"GET /mcp HTTP/1.1\r\nhost: proxy\r\n\r\n")
                .await
                .unwrap();
            let mut response = Vec::new();
            let mut byte = [0_u8; 1];
            while !response.ends_with(b"\r\n\r\n{}") {
                assert_eq!(
                    stream.read(&mut byte).await.unwrap(),
                    1,
                    "the connection stayed open"
                );
                response.push(byte[0]);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        stop().await.unwrap().unwrap();
    })
    .await;
}

/// An upstream whose accept queue is full drops the proxy's SYNs, so its
/// connect never completes: without a connect timeout the client waited for
/// as long as the operating system kept retrying (minutes). It now gets the
/// proxy's 502 once the connect timeout passes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upstream_that_never_accepts_the_connection_is_a_502() {
    bounded(async {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let upstream = socket.listen(0).unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        // Fill the accept queue; nothing ever accepts.
        let mut held = Vec::new();
        for _ in 0..4 {
            let connect = tokio::net::TcpStream::connect(upstream_address);
            if let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_millis(200), connect).await
            {
                held.push(stream);
            }
        }
        let (address, _, stop) = proxy(format!("http://{upstream_address}"), |options| {
            options.connect_timeout = Duration::from_millis(300);
        })
        .await;
        let started = Instant::now();
        let response = reqwest::Client::new()
            .post(format!("http://{address}/mcp"))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 502);
        assert!(started.elapsed() < Duration::from_secs(5));
        let unrecorded = stop().await.unwrap().unwrap();
        assert_eq!(unrecorded.upstream_failures, 1);
        drop((held, upstream));
    })
    .await;
}

/// Every exchange in flight could hold a body of up to the message limit, so
/// memory grew with their number: eight concurrent 60 MB POSTs held 488 MB.
/// What the proxy holds is now bounded by its buffer budget; a body that
/// arrives while it is spent is forwarded intact without being recorded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bodies_past_the_buffer_budget_are_streamed_not_held() {
    bounded(async {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let app = axum::Router::new().fallback(|body: axum::body::Bytes| async move {
            format!("{{\"length\":{}}}", body.len())
        });
        tokio::spawn(async move { axum::serve(upstream, app).await.unwrap() });
        let (address, sink, stop) = proxy(format!("http://{upstream_address}"), |options| {
            options.buffer_budget = 1500;
        })
        .await;
        let body = |fill: char| format!("{{\"pad\":\"{}\"}}", fill.to_string().repeat(1000 - 10));
        let (first, second) = (body('a'), body('b'));
        // The first body arrives in part, and the proxy holds that part…
        let mut slow = tokio::net::TcpStream::connect(address).await.unwrap();
        let request_head = "POST /mcp HTTP/1.1\r\nhost: proxy\r\ncontent-type: application/json\r\n\
                    content-length: 1000\r\nconnection: close\r\n\r\n";
        slow.write_all(request_head.as_bytes()).await.unwrap();
        slow.write_all(&first.as_bytes()[..900]).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        // …so a second, which would need 1000 of the 600 left, is streamed.
        let answer = reqwest::Client::new()
            .post(format!("http://{address}/mcp"))
            .header("content-type", "application/json")
            .body(second.clone())
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(answer, "{\"length\":1000}", "forwarded intact");
        slow.write_all(&first.as_bytes()[900..]).await.unwrap();
        let mut answer = Vec::new();
        slow.read_to_end(&mut answer).await.unwrap();
        assert!(String::from_utf8_lossy(&answer).ends_with("{\"length\":1000}"));
        let unrecorded = stop().await.unwrap().unwrap();
        assert_eq!(unrecorded.over_budget, 1);
        assert_eq!(unrecorded.oversized, 0);
        let trace = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(trace.contains(&"a".repeat(990)), "the first is recorded");
        assert!(!trace.contains(&"b".repeat(990)), "the second is not");
    })
    .await;
}
