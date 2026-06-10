// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! In-process protocol round-trips: a real rmcp client drives the server over
//! `tokio::io::duplex` — full initialize/list/call exchanges through the same
//! codec the stdio transport uses, with no sockets and no subprocesses.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_everything_server::EverythingServer;
use rmcp::ServiceExt as _;
use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::service::{RoleClient, RunningService};

/// Spawns the server on one end of an in-memory pipe and initializes a
/// trivial client on the other; returns the connected client handle.
async fn connect() -> RunningService<RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    ().serve(client_io).await.expect("client initialize")
}

#[tokio::test]
async fn initialize_negotiates_the_pinned_revision_and_capabilities() {
    let client = connect().await;
    let info = client.peer_info().expect("initialize result");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25);
    assert!(info.capabilities.tools.is_some());
    assert_eq!(info.server_info.name, "mcp-everything-server");
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn tools_list_exposes_the_basic_router_with_schemas() {
    let client = connect().await;
    let tools = client.list_tools(None).await.expect("tools/list");
    let names: Vec<&str> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
    assert!(names.contains(&"echo"), "echo missing from {names:?}");
    assert!(names.contains(&"add"), "add missing from {names:?}");
    let echo = tools.tools.iter().find(|tool| tool.name == "echo").unwrap();
    let schema = serde_json::to_value(&echo.input_schema).unwrap();
    assert_eq!(schema["type"], "object", "echo schema must be an object");
    assert!(
        schema["properties"]["message"].is_object(),
        "echo schema must describe its message parameter: {schema}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn echo_returns_the_upstream_everything_server_phrasing() {
    let client = connect().await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("echo").with_arguments(
                serde_json::json!({"message": "conformance"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .expect("tools/call echo");
    let text = result.content.first().and_then(|content| content.as_text());
    assert_eq!(text.map(|t| t.text.as_str()), Some("Echo: conformance"));
    assert_ne!(result.is_error, Some(true));
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn add_phrases_sums_like_the_typescript_server() {
    let client = connect().await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("add").with_arguments(
                serde_json::json!({"a": 2, "b": 3})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .expect("tools/call add");
    let text = result.content.first().and_then(|content| content.as_text());
    assert_eq!(
        text.map(|t| t.text.as_str()),
        Some("The sum of 2 and 3 is 5.")
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn unknown_tool_is_a_protocol_error_not_a_crash() {
    let client = connect().await;
    let outcome = client
        .call_tool(CallToolRequestParams::new("no-such-tool"))
        .await;
    assert!(outcome.is_err(), "calling an unknown tool must error");
    // The session must survive the error: the next request still works.
    let tools = client
        .list_tools(None)
        .await
        .expect("tools/list after error");
    assert!(!tools.tools.is_empty());
    client.cancel().await.expect("clean shutdown");
}
