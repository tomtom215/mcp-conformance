// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `test_reconnection` (SEP-1699), on each transport shape the server has.
//!
//! Over a session-bearing HTTP transport the tool must end its POST stream
//! without answering and deliver the result to the GET that presents
//! `Last-Event-ID` — the exchange the official suite's `server-sse-polling`
//! scenario performs, driven here in-process via `tower::ServiceExt::oneshot`
//! (no sockets, no ports). Where there is no stream to close — stdio, and
//! `2026-07-28`'s session-less HTTP — it must simply answer.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use mcp_everything_server::EverythingServer;
use mcp_everything_server::reconnection::RECONNECTION_RESULT;
use rmcp::ServiceExt as _;
use rmcp::model::CallToolRequestParams;

/// Bound on every wait below: far above anything a passing run needs, and
/// what turns a regression into a failure instead of a hung job.
const PATIENCE: Duration = Duration::from_secs(30);

#[tokio::test]
async fn without_an_http_stream_the_tool_is_listed_and_answers_directly() {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    let client = ().serve(client_io).await.expect("client initialize");
    let tools = client.list_tools(None).await.expect("tools/list");
    let tool = tools
        .tools
        .iter()
        .find(|tool| tool.name == "test_reconnection")
        .expect("test_reconnection listed on the first page");
    // The suite calls it with `arguments: {}`: no required properties.
    let schema = serde_json::to_value(&tool.input_schema).unwrap();
    assert!(schema.get("required").is_none(), "{schema}");
    let result = tokio::time::timeout(
        PATIENCE,
        client.call_tool(CallToolRequestParams::new("test_reconnection")),
    )
    .await
    .expect("no stream to wait on")
    .expect("tools/call");
    assert_eq!(
        result.content[0].as_text().map(|text| text.text.as_str()),
        Some(RECONNECTION_RESULT)
    );
    assert_ne!(result.is_error, Some(true));
    client.cancel().await.expect("clean shutdown");
}

#[cfg(feature = "http")]
mod http {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, request::Builder};
    use axum::response::Response;
    use mcp_everything_server::policy::HttpSecurityPolicy;
    use mcp_everything_server::reconnection::{RECONNECT_RETRY, RECONNECTION_RESULT};
    use mcp_everything_server::server::ServedRevision;
    use tower::ServiceExt as _;

    use super::PATIENCE;

    const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"reconnection-test","version":"0"}}}"#;
    const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    const CALL: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"test_reconnection","arguments":{}}}"#;

    fn router(revision: ServedRevision) -> axum::Router {
        mcp_everything_server::http::router(
            HttpSecurityPolicy::with_allowed_hosts(vec!["localhost".to_owned()]),
            revision,
        )
    }

    fn post(session: Option<&str>) -> Builder {
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("host", "localhost")
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream");
        match session {
            Some(session) => request
                .header("mcp-session-id", session)
                .header("mcp-protocol-version", "2025-11-25"),
            None => request,
        }
    }

    async fn send(app: &axum::Router, request: Request<Body>) -> Response {
        tokio::time::timeout(PATIENCE, app.clone().oneshot(request))
            .await
            .expect("the router answers in time")
            .expect("the router answers")
    }

    /// The whole body, which for an SSE response means "until the server ends
    /// the stream" — the property under test, hence the bound.
    async fn body_until_closed(response: Response) -> String {
        let bytes = tokio::time::timeout(
            PATIENCE,
            axum::body::to_bytes(response.into_body(), 1024 * 1024),
        )
        .await
        .expect("the server ends the stream")
        .expect("a readable body");
        String::from_utf8(bytes.to_vec()).expect("UTF-8 SSE")
    }

    /// Opens a `2025-11-25` session and returns its id.
    async fn session(app: &axum::Router) -> String {
        let response = send(app, post(None).body(Body::from(INITIALIZE)).unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let session = response.headers()["mcp-session-id"]
            .to_str()
            .unwrap()
            .to_owned();
        body_until_closed(response).await;
        let response = send(
            app,
            post(Some(&session)).body(Body::from(INITIALIZED)).unwrap(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        session
    }

    /// The `data:` payloads of an SSE body that are JSON-RPC messages.
    fn messages(sse: &str) -> Vec<serde_json::Value> {
        sse.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .filter_map(|data| serde_json::from_str(data.trim()).ok())
            .collect()
    }

    fn last_event_id(sse: &str) -> String {
        sse.lines()
            .filter_map(|line| line.strip_prefix("id:"))
            .next_back()
            .expect("an event id to resume from")
            .trim()
            .to_owned()
    }

    #[tokio::test]
    async fn the_post_stream_closes_unanswered_and_the_resuming_get_gets_the_result() {
        let app = router(ServedRevision::default());
        let session = session(&app).await;

        let response = send(&app, post(Some(&session)).body(Body::from(CALL)).unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        // Reading to the end at all is the first assertion: the server closed
        // the stream while the call was still in flight.
        let interrupted = body_until_closed(response).await;
        assert_eq!(
            messages(&interrupted),
            Vec::<serde_json::Value>::new(),
            "no JSON-RPC message before the disconnect: {interrupted}"
        );
        let retry = format!("retry: {}", RECONNECT_RETRY.as_millis());
        assert!(
            interrupted.lines().any(|line| line == retry),
            "the reconnection interval precedes the disconnect: {interrupted}"
        );

        let resume = Request::builder()
            .method("GET")
            .uri("/mcp")
            .header("host", "localhost")
            .header("accept", "text/event-stream")
            .header("mcp-session-id", &session)
            .header("mcp-protocol-version", "2025-11-25")
            .header("last-event-id", last_event_id(&interrupted))
            .body(Body::empty())
            .unwrap();
        let response = send(&app, resume).await;
        assert_eq!(response.status(), StatusCode::OK);
        let resumed = body_until_closed(response).await;
        let answers = messages(&resumed);
        assert_eq!(answers.len(), 1, "exactly the tool's response: {resumed}");
        assert_eq!(answers[0]["id"], 1);
        assert_eq!(
            answers[0]["result"]["content"][0]["text"],
            RECONNECTION_RESULT
        );
    }

    /// The wrapper around rmcp's session manager passes on what a client
    /// posts into a session, and not only the requests it opens streams for:
    /// here the client's answer to the server's own request, without which
    /// the tool call that asked never finishes.
    #[tokio::test]
    async fn a_message_the_client_posts_into_the_session_reaches_the_server() {
        use std::time::Duration;

        use tokio_stream::StreamExt as _;

        let app = router(ServedRevision::default());
        let initialize = INITIALIZE.replace(
            r#""capabilities":{}"#,
            r#""capabilities":{"elicitation":{}}"#,
        );
        let response = send(&app, post(None).body(Body::from(initialize)).unwrap()).await;
        let session = response.headers()["mcp-session-id"]
            .to_str()
            .unwrap()
            .to_owned();
        body_until_closed(response).await;
        send(
            &app,
            post(Some(&session)).body(Body::from(INITIALIZED)).unwrap(),
        )
        .await;

        let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"test_elicitation","arguments":{"message":"who?"}}}"#;
        let response = send(&app, post(Some(&session)).body(Body::from(call)).unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut stream = response.into_body().into_data_stream();
        let mut seen = String::new();
        // Read until the server's own request arrives on the call's stream.
        let asked = loop {
            let chunk = tokio::time::timeout(PATIENCE, stream.next())
                .await
                .expect("the server asks in time")
                .expect("the stream is still open")
                .expect("a readable chunk");
            seen.push_str(&String::from_utf8_lossy(&chunk));
            if let Some(request) = messages(&seen)
                .into_iter()
                .find(|message| message["method"] == "elicitation/create")
            {
                break request;
            }
        };
        let answer = serde_json::json!({"jsonrpc": "2.0", "id": asked["id"], "result": {
            "action": "accept", "content": {"username": "ada", "email": "ada@example.com"}}});
        let accepted = send(
            &app,
            post(Some(&session))
                .body(Body::from(answer.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);
        // The answer reached the tool: its result arrives and the stream ends.
        loop {
            // Short: the answer is already in the server's hands, and a
            // server that dropped it would otherwise hold this for PATIENCE.
            let next = tokio::time::timeout(Duration::from_secs(5), stream.next())
                .await
                .expect("the tool finishes once it has its answer");
            match next {
                Some(chunk) => seen.push_str(&String::from_utf8_lossy(&chunk.expect("a chunk"))),
                None => break,
            }
        }
        let result = messages(&seen)
            .into_iter()
            .find(|message| message["id"] == 1)
            .unwrap_or_else(|| panic!("no result for the call: {seen}"));
        let text = result["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("action=accept") && text.contains("ada"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn the_session_less_revision_answers_on_the_post_itself() {
        let call = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}},"name":"test_reconnection","arguments":{}}}"#;
        let request = post(None)
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "tools/call")
            .header("mcp-name", "test_reconnection")
            .body(Body::from(call))
            .unwrap();
        let response = send(&router(ServedRevision::V2026_07_28), request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_until_closed(response).await;
        assert!(body.contains(RECONNECTION_RESULT), "{body}");
    }
}
