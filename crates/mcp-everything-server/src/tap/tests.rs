// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;

#[tokio::test]
async fn record_json_preserves_the_body_and_records_the_message() {
    // rmcp currently frames every request response as SSE, so this path
    // is pinned at the unit level: a JSON response must reach the client
    // byte-identical and land in the trace as one message event.
    let dir = std::env::temp_dir().join(format!("tap-json-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tap = Tap::new(dir.clone()).expect("tap directory");
    let body = r#"{"jsonrpc":"2.0","id":9,"result":{"ok":true}}"#;
    let response = Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("response");

    let status = (
        Direction::ServerToClient,
        EventBody::Http {
            method: None,
            status: Some(200),
            headers: std::collections::BTreeMap::new(),
        },
    );
    let returned = record_json(&tap, "json-session", status, response).await;
    let returned_body = axum::body::to_bytes(returned.into_body(), 1024 * 1024)
        .await
        .expect("body");
    assert_eq!(returned_body.as_ref(), body.as_bytes(), "byte-identical");

    let path = dir.join("001-json-session.jsonl");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let recorded = loop {
        if let Ok(text) = std::fs::read_to_string(&path)
            && text.lines().count() == 2
            && text.ends_with('\n')
        {
            break text;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the writer did not persist the message within 10s"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    };
    // The status first, then the message on the very next line.
    let mut lines = recorded.lines();
    let status: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(status["kind"], "http");
    assert_eq!(status["status"], 200);
    let event: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
    assert_eq!(event["seq"], 1);
    assert_eq!(event["kind"], "message");
    assert_eq!(event["direction"], "server-to-client");
    assert_eq!(event["payload"]["id"], 9);
    let _ = std::fs::remove_dir_all(dir);
}

/// A body past the recording cap reaches the other side byte for byte, in
/// both directions: through the tap to an echo service, and back.
#[tokio::test]
async fn an_oversized_body_passes_through_intact_both_ways() {
    use tower::ServiceExt as _;
    let dir = std::env::temp_dir().join(format!("tap-oversized-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let tap = Tap::new(dir.clone()).expect("tap directory");
    let echo = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(|body: axum::body::Bytes| async move {
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap()
            }),
        )
        // The echo's own extractor would refuse over 2 MB; the tap is the
        // subject here, not axum's default limit.
        .layer(axum::extract::DefaultBodyLimit::disable())
        .layer(axum::middleware::from_fn_with_state(tap, tap_layer));
    let sent: Vec<u8> = (0..MAX_RECORDED_BODY + 70_000)
        .map(|index| u8::try_from(index % 251).unwrap())
        .collect();
    // Many small chunks, so the cap is crossed mid-chunk-sequence.
    let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = sent
        .chunks(10_000)
        .map(|chunk| Ok(axum::body::Bytes::copy_from_slice(chunk)))
        .collect();
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .body(Body::from_stream(tokio_stream::iter(chunks)))
        .unwrap();
    let response = echo.oneshot(request).await.unwrap();
    let received = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(received.len(), sent.len());
    assert!(received == sent, "byte-identical round trip");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn debug_names_the_trace_directory_without_dumping_internals() {
    let dir = std::env::temp_dir().join(format!("tap-debug-{}", std::process::id()));
    let tap = Tap::new(dir.clone()).expect("tap directory");
    let rendered = format!("{tap:?}");
    assert!(
        rendered.contains("Tap") && rendered.contains("tap-debug"),
        "Debug names the type and its directory: {rendered}"
    );
    assert!(
        rendered.contains(".."),
        "the non-exhaustive marker shows fields are elided: {rendered}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// `buffer` at the cap's edge: exactly the cap is recorded whole, one byte past
/// it is passed through — the arithmetic pinned from both sides.
#[tokio::test]
async fn a_body_of_exactly_the_cap_is_recorded_and_one_byte_more_is_not() {
    use super::body::{Buffered, buffer};
    let body = |len: usize| Body::from(vec![b'x'; len]);
    assert!(matches!(
        buffer(body(MAX_RECORDED_BODY)).await,
        Buffered::Whole(bytes) if bytes.len() == MAX_RECORDED_BODY
    ));
    let Buffered::Passed(passed) = buffer(body(MAX_RECORDED_BODY + 1)).await else {
        panic!("one byte over the cap is passed through");
    };
    let delivered = axum::body::to_bytes(passed, usize::MAX).await.unwrap();
    assert_eq!(delivered.len(), MAX_RECORDED_BODY + 1);
}

/// A request's HTTP event and its message travel to the writer as one record,
/// so they become adjacent lines: sent separately, a concurrent GET's request
/// event could land between them, and the validator then read the POST's
/// message as having ridden the GET (`TRAN-049`).
#[tokio::test]
async fn a_request_and_its_message_are_one_record() {
    use tower::ServiceExt as _;
    let (sender, mut receiver) = tokio::sync::mpsc::channel(16);
    let dir = std::env::temp_dir().join(format!("tap-together-{}", std::process::id()));
    let tap = Arc::new(Tap {
        dir: dir.clone(),
        sender,
        sessions: Mutex::new(HashMap::new()),
        created: AtomicU64::new(0),
    });
    let service = axum::Router::new()
        .route(
            "/mcp",
            axum::routing::post(|| async {
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#))
                    .unwrap()
            }),
        )
        .layer(axum::middleware::from_fn_with_state(tap, tap_layer));
    let request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
        .unwrap();
    let _ = service.oneshot(request).await.unwrap();

    let kinds = |record: &Record| -> Vec<(Direction, &'static str)> {
        record
            .events
            .iter()
            .map(|(direction, body)| {
                let kind = match body {
                    EventBody::Http { .. } => "http",
                    EventBody::Message { .. } => "message",
                    _ => "other",
                };
                (*direction, kind)
            })
            .collect()
    };
    let request_record = receiver.recv().await.unwrap();
    assert_eq!(
        kinds(&request_record),
        [
            (Direction::ClientToServer, "http"),
            (Direction::ClientToServer, "message")
        ]
    );
    let response_record = receiver.recv().await.unwrap();
    assert_eq!(
        kinds(&response_record),
        [
            (Direction::ServerToClient, "http"),
            (Direction::ServerToClient, "message")
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}
