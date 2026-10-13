// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the proxy puts on the wire, byte for byte, read off raw sockets on both
//! sides: the framing a body keeps, the headers a proxy must not pass on, the
//! paths it refuses, and where it points a redirect back to.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mcp_trace_capture::{Recorder, http};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::sync::mpsc;

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
    fn events(&self) -> Vec<serde_json::Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

/// Reads one HTTP/1.1 message's head, and its body by its framing.
async fn read_request(stream: &mut tokio::net::TcpStream) -> Option<Vec<u8>> {
    let mut request = Vec::new();
    let mut byte = [0_u8; 1];
    while !request.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await.ok()? == 0 {
            return None;
        }
        request.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&request).to_ascii_lowercase();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map(|value| value.trim().parse::<usize>().unwrap());
    if let Some(length) = length {
        let mut body = vec![0_u8; length];
        stream.read_exact(&mut body).await.ok()?;
        request.extend(body);
    } else if head.contains("transfer-encoding: chunked") {
        let start = request.len();
        while !request[start..].ends_with(b"0\r\n\r\n") {
            if stream.read(&mut byte).await.ok()? == 0 {
                return None;
            }
            request.push(byte[0]);
        }
    }
    Some(request)
}

/// An upstream that answers every request with `response` (which should say
/// `Connection: close`) and closes; each request, as it arrived, comes out of
/// the channel.
async fn upstream(response: Vec<u8>) -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    upstream_with(|_| response).await
}

/// [`upstream`], its response made from its own address.
async fn upstream_with(
    response: impl FnOnce(SocketAddr) -> Vec<u8>,
) -> (SocketAddr, mpsc::UnboundedReceiver<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (seen, requests) = mpsc::unbounded_channel();
    let response = Arc::new(response(address));
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let (seen, response) = (seen.clone(), Arc::clone(&response));
            tokio::spawn(async move {
                if let Some(request) = read_request(&mut stream).await {
                    let _ = seen.send(String::from_utf8_lossy(&request).into_owned());
                    stream.write_all(&response).await.ok();
                }
            });
        }
    });
    (address, requests)
}

/// The proxy in front of `upstream`, recording messages up to `max_message`.
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

/// Sends `request` (which should say `Connection: close`) to `address` and
/// returns everything that comes back, lowercased.
async fn exchange(address: SocketAddr, request: &[u8]) -> String {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(request).await.unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.ok();
    String::from_utf8_lossy(&response).to_ascii_lowercase()
}

/// A `HEAD` response's `Content-Length` is the length the `GET` would have
/// had, and there is no body to compute it from: the proxy dropped the
/// upstream's and sent `content-length: 0`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_head_response_keeps_its_content_length() {
    bounded(async {
        let (upstream, _requests) = upstream(
            b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 42\r\n\
              connection: close\r\n\r\n"
                .to_vec(),
        )
        .await;
        let (address, _, stop) = proxy(format!("http://{upstream}"), 1024).await;
        let response = exchange(
            address,
            b"HEAD /mcp HTTP/1.1\r\nhost: proxy\r\nconnection: close\r\n\r\n",
        )
        .await;
        assert!(response.starts_with("http/1.1 200"), "{response}");
        assert!(response.contains("content-length: 42\r\n"), "{response}");
        stop().await.unwrap().unwrap();
    })
    .await;
}

/// A body past the message limit is streamed, not read whole — but the
/// `Content-Length` its sender gave still frames it: it went out chunked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_over_the_limit_keeps_its_content_length() {
    bounded(async {
        let body = "x".repeat(100);
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 100\r\n\
             connection: close\r\n\r\n{body}"
        );
        let (upstream, mut requests) = upstream(response.into_bytes()).await;
        let (address, _, stop) = proxy(format!("http://{upstream}"), 16).await;
        let request = format!(
            "POST /mcp HTTP/1.1\r\nhost: proxy\r\ncontent-type: application/json\r\n\
             content-length: 100\r\nconnection: close\r\n\r\n{body}"
        );
        let response = exchange(address, request.as_bytes()).await;
        let forwarded = requests.recv().await.unwrap().to_ascii_lowercase();
        assert!(forwarded.contains("content-length: 100\r\n"), "{forwarded}");
        assert!(!forwarded.contains("transfer-encoding"), "{forwarded}");
        assert!(forwarded.ends_with(&body), "{forwarded}");
        assert!(response.contains("content-length: 100\r\n"), "{response}");
        assert!(!response.contains("transfer-encoding"), "{response}");
        assert!(response.ends_with(&body), "{response}");
        let unrecorded = stop().await.unwrap().unwrap();
        assert_eq!(unrecorded.oversized, 2);
    })
    .await;
}

/// `Proxy-Connection` is a hop-by-hop field in all but name, and the proxy
/// already answered `Expect: 100-continue` itself when it read the body: the
/// upstream gets neither.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn proxy_connection_and_expect_are_not_forwarded() {
    bounded(async {
        let (upstream, mut requests) = upstream(
            b"HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec(),
        )
        .await;
        let (address, _, stop) = proxy(format!("http://{upstream}"), 1024).await;
        let body = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let request = format!(
            "POST /mcp HTTP/1.1\r\nhost: proxy\r\nproxy-connection: keep-alive\r\n\
             expect: 100-continue\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
             connection: close\r\n\r\n{body}",
            body.len()
        );
        let response = exchange(address, request.as_bytes()).await;
        assert!(response.contains("202 accepted"), "{response}");
        let forwarded = requests.recv().await.unwrap().to_ascii_lowercase();
        assert!(!forwarded.contains("proxy-connection"), "{forwarded}");
        assert!(!forwarded.contains("expect:"), "{forwarded}");
        assert!(forwarded.ends_with(body), "{forwarded}");
        stop().await.unwrap().unwrap();
    })
    .await;
}

/// A client that goes away mid-body leaves its request recorded without a
/// body; the trace must say the exchange was cut off, as it does for an
/// upstream that fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_client_body_cut_off_is_recorded_as_an_abort() {
    bounded(async {
        let (upstream, mut requests) =
            upstream(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec())
                .await;
        let (address, sink, stop) = proxy(format!("http://{upstream}"), 1024).await;
        let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
        stream
            .write_all(
                b"POST /mcp HTTP/1.1\r\nhost: proxy\r\ncontent-length: 100\r\n\r\n{\"jsonrpc\":",
            )
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).await.ok();
        stop().await.unwrap().unwrap();
        assert!(requests.try_recv().is_err(), "nothing was forwarded");
        let events = sink.events();
        let kinds: Vec<(&str, &str)> = events
            .iter()
            .map(|event| {
                (
                    event["kind"].as_str().unwrap(),
                    event["event"].as_str().unwrap_or(""),
                )
            })
            .collect();
        assert_eq!(
            kinds[..2],
            [("http", ""), ("lifecycle", "transport-abort")],
            "{events:?}"
        );
    })
    .await;
}
