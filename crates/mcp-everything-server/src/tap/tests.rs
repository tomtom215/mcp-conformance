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

    let returned = record_json(&tap, "json-session", response).await;
    let returned_body = axum::body::to_bytes(returned.into_body(), 1024 * 1024)
        .await
        .expect("body");
    assert_eq!(returned_body.as_ref(), body.as_bytes(), "byte-identical");

    let path = dir.join("001-json-session.jsonl");
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    let recorded = loop {
        if let Ok(text) = std::fs::read_to_string(&path)
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
    let event: serde_json::Value =
        serde_json::from_str(recorded.lines().next().expect("one line")).expect("event");
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
