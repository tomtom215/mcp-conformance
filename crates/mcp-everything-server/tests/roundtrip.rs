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

/// `(level, message)` pairs captured from `notifications/message`.
type LogLog = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;
/// `(progress, total)` pairs captured from `notifications/progress`.
type ProgressLog = std::sync::Arc<std::sync::Mutex<Vec<(f64, Option<f64>)>>>;

/// Client handler that records logging and progress notifications, so the
/// mid-call notification contracts are assertable from a real session.
#[derive(Debug, Clone, Default)]
struct Recorder {
    logs: LogLog,
    progress: ProgressLog,
}

impl rmcp::ClientHandler for Recorder {
    async fn on_logging_message(
        &self,
        params: rmcp::model::LoggingMessageNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        let text = params
            .data
            .as_str()
            .map_or_else(|| params.data.to_string(), ToOwned::to_owned);
        self.logs
            .lock()
            .unwrap()
            .push((format!("{:?}", params.level), text));
    }

    async fn on_progress(
        &self,
        params: rmcp::model::ProgressNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.progress
            .lock()
            .unwrap()
            .push((params.progress, params.total));
    }
}

async fn connect_recording() -> (RunningService<RoleClient, Recorder>, Recorder) {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    let recorder = Recorder::default();
    let client = recorder
        .clone()
        .serve(client_io)
        .await
        .expect("client initialize");
    (client, recorder)
}

#[tokio::test]
async fn suite_contract_tools_are_all_listed() {
    let client = connect().await;
    let tools = client.list_tools(None).await.expect("tools/list");
    let names: Vec<&str> = tools.tools.iter().map(|tool| tool.name.as_ref()).collect();
    for required in [
        "test_simple_text",
        "test_image_content",
        "test_audio_content",
        "test_embedded_resource",
        "test_multiple_content_types",
        "test_error_handling",
        "test_tool_with_logging",
        "test_tool_with_progress",
        "json_schema_2020_12_tool",
    ] {
        assert!(
            names.contains(&required),
            "{required} missing from {names:?}"
        );
    }
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn json_schema_tool_preserves_2020_12_keywords_verbatim() {
    let client = connect().await;
    let tools = client.list_tools(None).await.expect("tools/list");
    let tool = tools
        .tools
        .iter()
        .find(|tool| tool.name == "json_schema_2020_12_tool")
        .expect("schema tool listed");
    let schema = serde_json::to_value(&tool.input_schema).unwrap();
    assert_eq!(
        schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "$schema preserved"
    );
    assert!(schema["$defs"]["address"].is_object(), "$defs preserved");
    assert_eq!(
        schema["properties"]["address"]["$ref"], "#/$defs/address",
        "$ref preserved"
    );
    assert_eq!(schema["additionalProperties"], false);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn content_tools_return_their_scenario_shapes() {
    let client = connect().await;

    let simple = client
        .call_tool(CallToolRequestParams::new("test_simple_text"))
        .await
        .expect("simple text");
    assert_eq!(
        simple.content[0].as_text().map(|t| t.text.as_str()),
        Some("This is a simple text response for testing.")
    );

    let image = client
        .call_tool(CallToolRequestParams::new("test_image_content"))
        .await
        .expect("image");
    let image_content = image.content[0].as_image().expect("image content");
    assert_eq!(image_content.mime_type, "image/png");
    assert!(!image_content.data.is_empty());

    let audio = client
        .call_tool(CallToolRequestParams::new("test_audio_content"))
        .await
        .expect("audio");
    let audio_json = serde_json::to_value(&audio.content).unwrap();
    assert_eq!(audio_json[0]["type"], "audio");
    assert_eq!(audio_json[0]["mimeType"], "audio/wav");

    let embedded = client
        .call_tool(CallToolRequestParams::new("test_embedded_resource"))
        .await
        .expect("embedded resource");
    let resource = embedded.content[0].as_resource().expect("resource content");
    let resource_json = serde_json::to_value(resource).unwrap();
    assert_eq!(resource_json["resource"]["uri"], "test://embedded-resource");
    assert_eq!(resource_json["resource"]["mimeType"], "text/plain");
    assert_eq!(
        resource_json["resource"]["text"],
        "This is an embedded resource content."
    );

    let mixed = client
        .call_tool(CallToolRequestParams::new("test_multiple_content_types"))
        .await
        .expect("mixed content");
    let mixed_json = serde_json::to_value(&mixed.content).unwrap();
    assert_eq!(mixed_json[0]["type"], "text");
    assert_eq!(mixed_json[1]["type"], "image");
    assert_eq!(mixed_json[2]["type"], "resource");
    assert_eq!(
        mixed_json[2]["resource"]["text"],
        r#"{"test":"data","value":123}"#
    );

    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn error_tool_reports_in_band_failure_without_breaking_the_session() {
    let client = connect().await;
    let result = client
        .call_tool(CallToolRequestParams::new("test_error_handling"))
        .await
        .expect("call succeeds at the protocol level");
    assert_eq!(result.is_error, Some(true), "isError must be set");
    assert_eq!(
        result.content[0].as_text().map(|t| t.text.as_str()),
        Some("This tool intentionally returns an error for testing")
    );
    let again = client.list_tools(None).await.expect("session survives");
    assert!(!again.tools.is_empty());
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn logging_tool_emits_three_staged_info_messages() {
    let (client, recorder) = connect_recording().await;
    let result = client
        .call_tool(CallToolRequestParams::new("test_tool_with_logging"))
        .await
        .expect("logging tool");
    assert!(result.content[0].as_text().is_some());
    let logs = recorder.logs.lock().unwrap().clone();
    let messages: Vec<&str> = logs.iter().map(|(_, m)| m.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "Tool execution started",
            "Tool processing data",
            "Tool execution completed"
        ],
        "exact scenario messages in order"
    );
    assert!(
        logs.iter().all(|(level, _)| level == "Info"),
        "all info level: {logs:?}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn progress_tool_walks_0_50_100_against_the_request_token() {
    let (client, recorder) = connect_recording().await;
    let mut params = CallToolRequestParams::new("test_tool_with_progress");
    let mut meta = rmcp::model::Meta::default();
    meta.set_progress_token(rmcp::model::ProgressToken(
        rmcp::model::NumberOrString::String("probe-1".into()),
    ));
    params.meta = Some(meta);
    let result = client.call_tool(params).await.expect("progress tool");
    assert!(result.content[0].as_text().is_some());
    let progress = recorder.progress.lock().unwrap().clone();
    // JSON-value comparison sidesteps float-equality lints; the values are
    // exact integral floats produced by our own u32 conversions.
    assert_eq!(
        serde_json::json!(progress),
        serde_json::json!([[0.0, 100.0], [50.0, 100.0], [100.0, 100.0]]),
        "exact progression"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn rmcp_clients_always_carry_a_progress_token() {
    // Pinned upstream behavior: rmcp's Peer injects a progress token into
    // every outgoing request (AtomicU32ProgressTokenProvider), so the
    // server's no-token branch is unreachable from an rmcp client and the
    // notifications flow even when the caller sets no meta. If this test
    // starts failing, rmcp changed that policy — revisit the progress tests.
    let (client, recorder) = connect_recording().await;
    let result = client
        .call_tool(CallToolRequestParams::new("test_tool_with_progress"))
        .await
        .expect("progress tool without explicit token");
    assert!(result.content[0].as_text().is_some());
    let count = recorder.progress.lock().unwrap().len();
    assert_eq!(count, 3, "auto-token still produces the staged updates");
    client.cancel().await.expect("clean shutdown");
}
