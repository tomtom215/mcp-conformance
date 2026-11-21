// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The one kind of request path the proxy records: a well-known URI's, with
//! the document it answered. Nothing else about where a request went is
//! kept — not the MCP endpoint's path, and never a query.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use axum::routing::get;
use mcp_trace_capture::{Recorder, http};
use serde_json::Value;

const METADATA: &str = "/.well-known/oauth-protected-resource/mcp";
const DOCUMENT: &str = r#"{"resource":"https://h/mcp","authorization_servers":["https://as"]}"#;

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

/// A server with metadata, a plain-text well-known file, an endpoint that
/// answers JSON to a GET, and nothing else.
async fn upstream() -> SocketAddr {
    let json =
        |body: &'static str| move || async move { ([("content-type", "application/json")], body) };
    let app = axum::Router::new()
        .route(METADATA, get(json(DOCUMENT)))
        .route("/.well-known/security.txt", get(|| async { "Contact: x" }))
        .route("/mcp", get(json(r#"{"not":"metadata"}"#)))
        .route(
            "/mcp/.well-known/x",
            get(json(r#"{"under":"the endpoint"}"#)),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// Gets each of `paths` through a proxy in front of `upstream`; the statuses
/// and bodies the client saw, and the trace's records.
async fn fetched(upstream: String, paths: &[&str]) -> (Vec<(StatusCode, String)>, Vec<Value>) {
    let sink = Shared::default();
    let recorder = Arc::new(Recorder::new(sink.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let options = http::Options::new(upstream.parse().unwrap(), 1024 * 1024).unwrap();
    let task = tokio::spawn(http::serve(listener, recorder, options, async {
        stopped.await.ok();
    }));
    let mut answers = Vec::new();
    for path in paths {
        let response = reqwest::Client::new()
            .get(format!("http://{address}{path}"))
            .header("authorization", "Bearer a-real-credential")
            .send()
            .await
            .unwrap();
        answers.push((response.status(), response.text().await.unwrap()));
    }
    stop.send(()).ok();
    task.await.unwrap().unwrap();
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    assert!(!text.contains("a-real-credential"), "{text}");
    let records = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        // The exchanges, without the proxy's own closing of the transport.
        .filter(|record| record["kind"] != "lifecycle")
        .collect();
    (answers, records)
}

async fn bounded<T>(body: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(20), body)
        .await
        .expect("the test finished within 20 s")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_well_known_document_is_recorded_with_its_path_and_relayed_unchanged() {
    let origin = format!("http://{}", bounded(upstream()).await);
    let asked = format!("{METADATA}?token=in-the-query");
    let (answers, records) = bounded(fetched(origin, &[&asked])).await;
    // The client got the server's bytes.
    assert_eq!(answers, [(StatusCode::OK, DOCUMENT.to_owned())]);
    // Two records, both `http`: the request and its answer. The document is
    // on the answer, not a message of its own.
    assert_eq!(records.len(), 2, "{records:?}");
    assert!(records.iter().all(|record| record["kind"] == "http"));
    let (request, answer) = (&records[0], &records[1]);
    assert_eq!(request["method"], "GET");
    assert_eq!(request["path"], METADATA, "the path, without its query");
    assert!(request.get("document").is_none());
    assert_eq!(answer["status"], 200);
    assert_eq!(answer["path"], METADATA);
    assert_eq!(
        answer["document"],
        serde_json::from_str::<Value>(DOCUMENT).unwrap()
    );
    assert!(
        !records
            .iter()
            .any(|record| record.to_string().contains("in-the-query"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_that_is_not_json_or_not_there_carries_a_path_and_no_document() {
    let origin = format!("http://{}", bounded(upstream()).await);
    let paths = ["/.well-known/security.txt", "/.well-known/nothing-here"];
    let (answers, records) = bounded(fetched(origin, &paths)).await;
    assert_eq!(answers[0], (StatusCode::OK, "Contact: x".to_owned()));
    assert_eq!(answers[1].0, StatusCode::NOT_FOUND);
    assert_eq!(records.len(), 4, "{records:?}");
    for (record, path) in records.iter().zip([
        "security.txt",
        "security.txt",
        "nothing-here",
        "nothing-here",
    ]) {
        assert_eq!(record["kind"], "http");
        assert_eq!(record["path"], format!("/.well-known/{path}"));
        assert!(record.get("document").is_none(), "{record}");
    }
    assert_eq!(records[1]["status"], 200);
    assert_eq!(records[3]["status"], 404);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_other_path_is_recorded() {
    let address = bounded(upstream()).await;
    // The endpoint itself, and the bare prefix, which names nothing.
    let (answers, records) = bounded(fetched(
        format!("http://{address}"),
        &["/mcp", "/.well-known/"],
    ))
    .await;
    assert_eq!(answers[0].1, r#"{"not":"metadata"}"#);
    assert!(
        records
            .iter()
            .all(|record| record.get("path").is_none() && record.get("document").is_none()),
        "{records:?}"
    );
    // Its JSON is a message, as any body on the endpoint is.
    assert!(records.iter().any(|record| record["kind"] == "message"));

    // Behind an upstream with a path of its own, the proxy's `/.well-known/`
    // reaches a path under the endpoint's, which is not a well-known URI.
    let (answers, records) = bounded(fetched(
        format!("http://{address}/mcp"),
        &["/.well-known/x"],
    ))
    .await;
    assert_eq!(answers[0].1, r#"{"under":"the endpoint"}"#);
    assert!(
        records.iter().all(|record| record.get("path").is_none()),
        "{records:?}"
    );
}
