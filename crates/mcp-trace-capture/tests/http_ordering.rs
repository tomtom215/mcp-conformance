// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Concurrent requests through one proxy: a request's `http` event and its
//! message, and a JSON response's status event and its message, are written next
//! to each other. The validator pairs a message with the headers around it, so
//! another exchange's events landing between the two pairs one request's body
//! with another's headers.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::response::Response;
use axum::routing::post;
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
    fn events(&self) -> Vec<serde_json::Value> {
        String::from_utf8(self.0.lock().unwrap().clone())
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

/// Answers each request with a result carrying its id; `/mcp/slow` sends its
/// headers at once and its body 300 ms later.
async fn upstream() -> SocketAddr {
    let answer = |body: String| {
        let request: serde_json::Value = serde_json::from_str(&body).unwrap();
        serde_json::json!({"jsonrpc": "2.0", "id": request["id"], "result": {}}).to_string()
    };
    let app = axum::Router::new()
        .route(
            "/mcp/fast",
            post(move |body: String| async move {
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(answer(body)))
                    .unwrap()
            }),
        )
        .route(
            "/mcp/slow",
            post(move |body: String| async move {
                let late = futures::stream::once(async move {
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    Ok::<_, std::io::Error>(answer(body))
                });
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from_stream(late))
                    .unwrap()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// Every `http` event of `direction` is followed directly by the message with
/// `id` it carried.
fn assert_paired(events: &[serde_json::Value], direction: &str) {
    for (index, event) in events.iter().enumerate() {
        if event["kind"] != "http" || event["direction"] != direction {
            continue;
        }
        let next = &events[index + 1];
        assert_eq!(
            (next["kind"].as_str(), next["direction"].as_str()),
            (Some("message"), Some(direction)),
            "seq {}'s http event is not followed by its message: {events:#?}",
            event["seq"]
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[allow(clippy::too_many_lines)] // One overlapping scenario, asserted over the whole trace.
async fn each_http_event_is_written_next_to_its_message() {
    bounded(async {
        let upstream = upstream().await;
        let sink = Shared::default();
        let recorder = Arc::new(Recorder::new(sink.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let options =
            http::Options::new(format!("http://{upstream}").parse().unwrap(), 1024).unwrap();
        let proxy = tokio::spawn(http::serve(listener, recorder, options, async {
            stopped.await.ok();
        }));

        // A request whose body arrives 300 ms after its headers, and a slow
        // response, each overlapping a quick exchange.
        let slow_request = tokio::spawn(async move {
            let body = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
            let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let head = format!(
                "POST /mcp/fast HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\n\
                 content-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            tokio::time::sleep(Duration::from_millis(300)).await;
            stream.write_all(body.as_bytes()).await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
        });
        let client = reqwest::Client::new();
        let slow_response = tokio::spawn({
            let client = client.clone();
            async move {
                client
                    .post(format!("http://{address}/mcp/slow"))
                    .body(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#)
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        for id in 3..6 {
            client
                .post(format!("http://{address}/mcp/fast"))
                .body(format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
        }
        slow_request.await.unwrap();
        assert!(slow_response.await.unwrap().contains(r#""id":2"#));
        stop.send(()).ok();
        proxy.await.unwrap().unwrap();

        let events = sink.events();
        assert_paired(&events, "client-to-server");
        assert_paired(&events, "server-to-client");
        let requests = events
            .iter()
            .filter(|event| event["kind"] == "http" && event["direction"] == "client-to-server")
            .count();
        assert_eq!(requests, 5);
    })
    .await;
}
