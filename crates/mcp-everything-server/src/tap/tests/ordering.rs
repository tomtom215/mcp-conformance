// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `seq` is observation order: pinned with a stand-in service whose exchanges
//! overlap on purpose, so the request observed first is the one answered last.

use super::*;
use tower::ServiceExt as _;

/// A stand-in MCP endpoint: a body with `"id":1` is held until a body with
/// `"id":2` arrives, and one with `"id":3` is never answered at all. A body
/// saying `initialize` sent with no session is answered with
/// `mcp-session-id: s1`, the way the real service assigns a session.
fn overlapping_endpoint(tap: Arc<Tap>) -> (axum::Router, Arc<tokio::sync::Notify>) {
    let arrived = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let signal = Arc::clone(&arrived);
    let handler = move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
        let arrived = Arc::clone(&signal);
        let release = Arc::clone(&release);
        async move {
            let text = String::from_utf8_lossy(&body).into_owned();
            if text.contains(r#""id":1"#) {
                arrived.notify_one();
                release.notified().await;
            } else if text.contains(r#""id":2"#) {
                release.notify_one();
            } else if text.contains(r#""id":3"#) {
                arrived.notify_one();
                std::future::pending::<()>().await;
            }
            let mut response = Response::builder().header("content-type", "application/json");
            if text.contains("initialize") && !headers.contains_key(SESSION_ID_HEADER) {
                response = response.header(SESSION_ID_HEADER, "s1");
            }
            response
                .body(Body::from(r#"{"jsonrpc":"2.0","id":0,"result":{}}"#))
                .unwrap()
        }
    };
    let router = axum::Router::new()
        .route("/mcp", axum::routing::post(handler))
        .layer(axum::middleware::from_fn_with_state(tap, tap_layer));
    (router, arrived)
}

/// A POST of `body`, naming `session` when there is one.
fn post(body: &'static str, session: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("content-type", "application/json");
    if let Some(session) = session {
        request = request.header(SESSION_ID_HEADER, session);
    }
    request.body(Body::from(body)).unwrap()
}

/// A fresh, empty trace directory for one test.
fn fresh_dir(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("tap-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The parsed events of the trace file in `dir` whose name contains `label`,
/// once it holds `lines` complete lines (10 s deadline).
async fn trace_with(dir: &std::path::Path, label: &str, lines: usize) -> Vec<serde_json::Value> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let text = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .find(|entry| entry.file_name().to_string_lossy().contains(label))
            .and_then(|entry| std::fs::read_to_string(entry.path()).ok())
            .unwrap_or_default();
        if text.lines().count() >= lines && text.ends_with('\n') {
            return text
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "{label}: {} of {lines} lines within 10s",
            text.lines().count()
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

/// The `seq` of the client message whose JSON-RPC id is `id`.
fn request_seq(events: &[serde_json::Value], id: u64) -> u64 {
    events
        .iter()
        .find(|event| event["direction"] == "client-to-server" && event["payload"]["id"] == id)
        .and_then(|event| event["seq"].as_u64())
        .unwrap_or_else(|| panic!("no request {id} in {events:#?}"))
}

/// Every event's `seq` is its line index: contiguous from zero.
fn assert_contiguous(events: &[serde_json::Value]) {
    for (index, event) in events.iter().enumerate() {
        assert_eq!(event["seq"], index as u64, "contiguous seq: {events:#?}");
    }
}

/// Waits until the service holds a request (10 s deadline: a tap that never
/// passed the request on fails the test rather than hanging it).
async fn held(arrived: &tokio::sync::Notify) {
    tokio::time::timeout(std::time::Duration::from_secs(10), arrived.notified())
        .await
        .expect("the request reached the service within 10s");
}

/// Sends request 1, waits until the service holds it, then sends request 2,
/// which releases it: request 1 is observed first and answered last.
async fn overlap(router: &axum::Router, arrived: &tokio::sync::Notify, session: Option<&str>) {
    let first = tokio::spawn(router.clone().oneshot(post(
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call"}"#,
        session,
    )));
    held(arrived).await;
    let _ = router
        .clone()
        .oneshot(post(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#, session))
        .await
        .unwrap();
    let _ = first.await.unwrap().unwrap();
}

/// A request is sequenced when the tap sees it, not when its response is
/// ready. It used to be recorded only after the service answered, so of two
/// overlapping requests on one session the one answered first was written
/// first — and anything the service sent meanwhile (a notification on the
/// session's GET stream) preceded the request that caused it.
#[tokio::test]
async fn overlapping_requests_on_a_session_are_sequenced_as_observed() {
    let dir = fresh_dir("observed");
    let (router, arrived) = overlapping_endpoint(Tap::new(dir.clone()).unwrap());
    let _ = router
        .clone()
        .oneshot(post(
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#,
            None,
        ))
        .await
        .unwrap();
    overlap(&router, &arrived, Some("s1")).await;

    // initialize (4) + two exchanges of four events each.
    let events = trace_with(&dir, "s1", 12).await;
    assert!(
        request_seq(&events, 1) < request_seq(&events, 2),
        "request 1 was observed first: {events:#?}"
    );
    assert_contiguous(&events);
    let _ = std::fs::remove_dir_all(dir);
}

/// The same for exchanges naming no session, whose trace file is known only
/// once the response says whether it assigned one: the request's place in the
/// `stateless` trace is reserved when it is observed.
#[tokio::test]
async fn overlapping_sessionless_requests_are_sequenced_as_observed() {
    let dir = fresh_dir("observed-stateless");
    let (router, arrived) = overlapping_endpoint(Tap::new(dir.clone()).unwrap());
    overlap(&router, &arrived, None).await;

    let events = trace_with(&dir, STATELESS_TRACE, 8).await;
    assert!(
        request_seq(&events, 1) < request_seq(&events, 2),
        "request 1 was observed first: {events:#?}"
    );
    assert_contiguous(&events);
    let _ = std::fs::remove_dir_all(dir);
}

/// A reserved place is given up, leaving no gap and holding nothing back, both
/// when the response turns out to open a session of its own and when the
/// exchange is abandoned before any response.
#[tokio::test]
async fn a_reserved_place_is_released_when_the_request_lands_elsewhere_or_never() {
    let dir = fresh_dir("released");
    let (router, arrived) = overlapping_endpoint(Tap::new(dir.clone()).unwrap());
    // Abandoned: the client goes away while the service still holds it.
    let abandoned = tokio::spawn(router.clone().oneshot(post(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call"}"#,
        None,
    )));
    held(&arrived).await;
    abandoned.abort();
    let _ = abandoned.await;
    // Elsewhere: an initialize whose response assigns session `s1`.
    let _ = router
        .clone()
        .oneshot(post(
            r#"{"jsonrpc":"2.0","id":0,"method":"initialize"}"#,
            None,
        ))
        .await
        .unwrap();
    let _ = router
        .clone()
        .oneshot(post(r#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#, None))
        .await
        .unwrap();

    let stateless = trace_with(&dir, STATELESS_TRACE, 4).await;
    assert_eq!(stateless.len(), 4, "only the ping exchange: {stateless:#?}");
    assert_eq!(request_seq(&stateless, 4), 1);
    assert_contiguous(&stateless);
    let session = trace_with(&dir, "s1", 4).await;
    assert_eq!(request_seq(&session, 0), 1, "initialize opens its session");
    assert_contiguous(&session);
    let _ = std::fs::remove_dir_all(dir);
}
