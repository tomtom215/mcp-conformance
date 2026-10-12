// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which event streams a stopping proxy cuts. The standalone stream a client
//! holds open for its whole session (the answer to a GET) is cut at once — and
//! cut, not ended, so the client can tell. An event stream answering a POST
//! carries that request's response, and gets the grace period every other
//! request in flight gets.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::convert::Infallible;
use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
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

/// An event stream of `events`, each `(delay before it, data)`, that then ends
/// — or, with `endless`, keeps sending comments.
fn sse(events: Vec<(u64, &'static str)>, endless: bool) -> Response {
    let stream = futures::stream::unfold(
        (events.into_iter(), endless),
        |(mut rest, endless)| async move {
            if let Some((delay, data)) = rest.next() {
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let chunk = Bytes::from(format!("data: {data}\n\n"));
                return Some((Ok::<_, Infallible>(chunk), (rest, endless)));
            }
            if !endless {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            Some((Ok(Bytes::from_static(b":\n\n")), (rest, endless)))
        },
    );
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}

async fn upstream() -> SocketAddr {
    let app = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::get(|| async {
                sse(vec![(0, r#"{"jsonrpc":"2.0","method":"hello"}"#)], true)
            })
            .post(|| async {
                sse(
                    vec![
                        (0, r#"{"jsonrpc":"2.0","method":"notifications/progress"}"#),
                        (800, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
                    ],
                    false,
                )
            }),
        )
        .route(
            "/short",
            axum::routing::get(|| async {
                sse(vec![(0, r#"{"jsonrpc":"2.0","method":"bye"}"#)], false)
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// Reads `response` until a chunk containing `text` has arrived.
async fn read_until(response: &mut reqwest::Response, text: &str) {
    let mut seen = String::new();
    while !seen.contains(text) {
        let chunk = response.chunk().await.unwrap().expect("more of the stream");
        seen.push_str(&String::from_utf8_lossy(&chunk));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_cuts_the_standalone_stream_and_lets_a_posts_stream_finish() {
    bounded(async {
        let upstream = upstream().await;
        let sink = Shared::default();
        let recorder = Arc::new(Recorder::new(sink.clone()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let options =
            http::Options::new(format!("http://{upstream}").parse().unwrap(), 1 << 20).unwrap();
        let serving = tokio::spawn(http::serve(listener, recorder, options, async {
            stopped.await.ok();
        }));
        let client = reqwest::Client::new();

        // A stream the upstream ends by itself ends cleanly through the proxy.
        let mut short = client
            .get(format!("http://{address}/short"))
            .send()
            .await
            .unwrap();
        read_until(&mut short, "bye").await;
        assert!(
            short.chunk().await.unwrap().is_none(),
            "a finished stream ends cleanly"
        );

        let mut standalone = client
            .get(format!("http://{address}/mcp"))
            .send()
            .await
            .unwrap();
        read_until(&mut standalone, "hello").await;
        let mut answer = client
            .post(format!("http://{address}/mcp"))
            .header("content-type", "application/json")
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#)
            .send()
            .await
            .unwrap();
        read_until(&mut answer, "progress").await;
        stop.send(()).unwrap();

        // The standalone stream is cut: an error, not a clean end.
        let cut = loop {
            match standalone.chunk().await {
                Ok(Some(_)) => {}
                Ok(None) => break false,
                Err(_) => break true,
            }
        };
        assert!(cut, "the standalone stream was aborted, not ended");

        // The POST's stream gets its grace: its response arrives, then it ends.
        read_until(&mut answer, "result").await;
        assert!(answer.chunk().await.unwrap().is_none());

        serving.await.unwrap().unwrap();
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        assert!(text.contains(r#""result":{}"#), "{text}");
        assert!(
            text.lines().last().unwrap().contains("transport-close"),
            "{text}"
        );
    })
    .await;
}
