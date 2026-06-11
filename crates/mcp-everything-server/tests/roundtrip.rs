// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! In-process protocol round-trips: a real rmcp client drives the server over
//! `tokio::io::duplex` — full initialize/list/call exchanges through the same
//! codec the stdio transport uses, with no sockets and no subprocesses.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_everything_server::EverythingServer;
use rmcp::ServiceExt as _;
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, ProtocolVersion};
use rmcp::service::{RoleClient, RunningService};

/// Unwraps an MCP protocol error, panicking when the call succeeded or died
/// at the transport layer. Error-path tests must pin *which* error: a test
/// that accepts any `Err` cannot tell a rejected request from a handler that
/// ran anyway and failed downstream.
#[track_caller]
fn mcp_error<T: std::fmt::Debug>(outcome: Result<T, rmcp::ServiceError>) -> ErrorData {
    match outcome {
        Err(rmcp::ServiceError::McpError(error)) => error,
        other => panic!("expected an MCP protocol error, got {other:?}"),
    }
}

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
    let error = mcp_error(outcome);
    assert_eq!(
        error.code,
        ErrorCode::INVALID_PARAMS,
        "unknown tool is the spec's -32602 protocol error: {error:?}"
    );
    assert!(
        error.message.contains("no-such-tool")
            || error.message.to_lowercase().contains("not found"),
        "the error names the problem: {error:?}"
    );
    // The session must survive the error: the next request still works.
    let tools = client
        .list_tools(None)
        .await
        .expect("tools/list after error");
    assert!(!tools.tools.is_empty());
    client.cancel().await.expect("clean shutdown");
}

/// Builds a `tools/call` with an explicit arguments object (which may be
/// deliberately malformed — the arguments map is untyped JSON, so wrong types
/// and missing members reach the server exactly as a hostile client would
/// send them).
fn call_with_args(tool: &str, args: &serde_json::Value) -> CallToolRequestParams {
    CallToolRequestParams::new(tool.to_owned())
        .with_arguments(args.as_object().cloned().unwrap_or_default())
}

#[tokio::test]
async fn tool_call_with_missing_required_argument_is_a_protocol_error() {
    let client = connect().await;
    // `add` requires both a and b; omitting b must be rejected at the
    // parameter boundary (-32602), never reach the handler, never crash.
    let outcome = client
        .call_tool(call_with_args("add", &serde_json::json!({"a": 1})))
        .await;
    let error = mcp_error(outcome);
    assert_eq!(
        error.code,
        ErrorCode::INVALID_PARAMS,
        "missing required arg is rejected at the parameter boundary: {error:?}"
    );
    // The session survives: the next request still works.
    assert!(
        !client
            .list_tools(None)
            .await
            .expect("list after error")
            .tools
            .is_empty()
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn tool_call_with_wrong_typed_arguments_is_a_protocol_error() {
    let client = connect().await;
    // `add` wants numbers; a string must be rejected, not coerced or panicked.
    let bad_add = client
        .call_tool(call_with_args(
            "add",
            &serde_json::json!({"a": "x", "b": 2}),
        ))
        .await;
    assert_eq!(
        mcp_error(bad_add).code,
        ErrorCode::INVALID_PARAMS,
        "wrong-typed add arg is rejected at the parameter boundary"
    );
    // `echo` wants a string message; a number must be rejected.
    let bad_echo = client
        .call_tool(call_with_args("echo", &serde_json::json!({"message": 123})))
        .await;
    assert_eq!(
        mcp_error(bad_echo).code,
        ErrorCode::INVALID_PARAMS,
        "wrong-typed echo arg is rejected at the parameter boundary"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn add_with_overflowing_finite_inputs_saturates_without_crashing() {
    let client = connect().await;
    // Two near-f64::MAX inputs sum to IEEE infinity. The contract is a
    // successful result (saturation), not an error and not a panic — pinning
    // the arithmetic's documented total behavior.
    let result = client
        .call_tool(call_with_args(
            "add",
            &serde_json::json!({"a": 1.0e308, "b": 1.0e308}),
        ))
        .await
        .expect("overflowing add still returns a result");
    let text = result.content.first().and_then(|content| content.as_text());
    assert!(
        text.is_some_and(|t| t.text.contains("inf")),
        "overflow saturates to inf: {text:?}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn prompt_get_with_missing_required_argument_is_a_protocol_error() {
    let client = connect().await;
    // test_prompt_with_arguments requires arg1 and arg2; omitting arg2 must
    // be rejected (-32602) before the handler formats anything.
    let mut params = rmcp::model::GetPromptRequestParams::new("test_prompt_with_arguments");
    params.arguments = serde_json::json!({"arg1": "only"}).as_object().cloned();
    let outcome = client.get_prompt(params).await;
    let error = mcp_error(outcome);
    assert_eq!(
        error.code,
        ErrorCode::INVALID_PARAMS,
        "missing prompt arg is rejected before the handler formats: {error:?}"
    );
    assert!(
        error.message.contains("arg2"),
        "the error names the missing argument: {error:?}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn completion_over_a_resource_reference_returns_no_candidates() {
    let client = connect().await;
    // The Resource arm of complete() is conformant minimal support: it
    // completes to nothing. Without this test the mutation gate cannot kill a
    // mutant that, say, returned the prompt candidates for a resource ref.
    let result = client
        .complete(rmcp::model::CompleteRequestParams::new(
            rmcp::model::Reference::for_resource("test://static-text"),
            rmcp::model::ArgumentInfo {
                name: "anything".into(),
                value: "pa".into(),
            },
        ))
        .await
        .expect("completion over a resource ref");
    assert!(
        result.completion.values.is_empty(),
        "resource-ref completion must yield no candidates: {:?}",
        result.completion.values
    );
    client.cancel().await.expect("clean shutdown");
}

/// `(level, message)` pairs captured from `notifications/message`.
type LogLog = std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>;
/// `(progress, total)` pairs captured from `notifications/progress`.
type ProgressLog = std::sync::Arc<std::sync::Mutex<Vec<(f64, Option<f64>)>>>;
/// URIs captured from `notifications/resources/updated`.
type UpdatedLog = std::sync::Arc<std::sync::Mutex<Vec<String>>>;
/// Which `notifications/*/list_changed` messages arrived, in arrival order.
type ListChangedLog = std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>;

/// Client handler that records logging and progress notifications, so the
/// mid-call notification contracts are assertable from a real session.
#[derive(Debug, Clone, Default)]
struct Recorder {
    logs: LogLog,
    progress: ProgressLog,
    updated: UpdatedLog,
    list_changed: ListChangedLog,
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

    async fn on_resource_updated(
        &self,
        params: rmcp::model::ResourceUpdatedNotificationParam,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.updated.lock().unwrap().push(params.uri);
    }

    async fn on_tool_list_changed(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.list_changed.lock().unwrap().push("tools");
    }

    async fn on_resource_list_changed(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.list_changed.lock().unwrap().push("resources");
    }

    async fn on_prompt_list_changed(
        &self,
        _context: rmcp::service::NotificationContext<rmcp::RoleClient>,
    ) {
        self.list_changed.lock().unwrap().push("prompts");
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
    assert_eq!(
        result.content[0].as_text().map(|t| t.text.as_str()),
        Some("Tool with logging executed successfully.")
    );
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
async fn list_changed_tool_emits_all_three_notifications() {
    let (client, recorder) = connect_recording().await;
    let result = client
        .call_tool(CallToolRequestParams::new("test_list_changed"))
        .await
        .expect("list-changed tool");
    assert!(
        result.content[0]
            .as_text()
            .is_some_and(|text| text.text.contains("emitted")),
        "the confirming result text: {result:?}"
    );
    let seen = recorder.list_changed.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec!["tools", "resources", "prompts"],
        "all three list_changed notifications, in emission order"
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
    assert_eq!(
        result.content[0].as_text().map(|t| t.text.as_str()),
        Some("Tool with progress executed successfully.")
    );
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
    assert_eq!(
        result.content[0].as_text().map(|t| t.text.as_str()),
        Some("Tool with progress executed successfully.")
    );
    let count = recorder.progress.lock().unwrap().len();
    assert_eq!(count, 3, "auto-token still produces the staged updates");
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn resources_surface_matches_the_scenarios() {
    let client = connect().await;

    let listed = client.list_resources(None).await.expect("resources/list");
    let uris: Vec<&str> = listed.resources.iter().map(|r| r.uri.as_str()).collect();
    assert!(uris.contains(&"test://static-text"), "{uris:?}");
    assert!(uris.contains(&"test://static-binary"), "{uris:?}");
    assert!(
        listed.resources.iter().all(|r| r.description.is_some()),
        "every listed resource carries a description"
    );

    let templates = client
        .list_resource_templates(None)
        .await
        .expect("resources/templates/list");
    assert_eq!(
        templates.resource_templates[0].uri_template,
        "test://template/{id}/data"
    );

    let text = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "test://static-text",
        ))
        .await
        .expect("read text");
    let text_json = serde_json::to_value(&text).unwrap();
    assert_eq!(
        text_json["contents"][0]["text"],
        "This is the content of the static text resource."
    );

    let binary = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "test://static-binary",
        ))
        .await
        .expect("read binary");
    let binary_json = serde_json::to_value(&binary).unwrap();
    assert_eq!(binary_json["contents"][0]["mimeType"], "image/png");
    assert!(binary_json["contents"][0]["blob"].is_string());

    let instantiated = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "test://template/123/data",
        ))
        .await
        .expect("read template instantiation");
    let instantiated_json = serde_json::to_value(&instantiated).unwrap();
    assert_eq!(
        instantiated_json["contents"][0]["text"],
        r#"{"id":"123","templateTest":true,"data":"Data for ID: 123"}"#
    );

    let missing = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(
            "test://no-such-resource",
        ))
        .await;
    let error = mcp_error(missing);
    assert_eq!(
        error.code,
        ErrorCode::RESOURCE_NOT_FOUND,
        "unknown URIs are the spec's -32002, not a generic failure: {error:?}"
    );

    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn subscribe_acknowledges_with_an_update_then_unsubscribe_round_trips() {
    let (client, recorder) = connect_recording().await;
    client
        .subscribe(rmcp::model::SubscribeRequestParams::new(
            "test://watched-resource",
        ))
        .await
        .expect("resources/subscribe");
    // The server acknowledges new subscriptions with an immediate update
    // notification — the observable consequence of tracking the URI.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if recorder.updated.lock().unwrap().as_slice() == ["test://watched-resource"] {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("update notification arrives");
    client
        .unsubscribe(rmcp::model::UnsubscribeRequestParams::new(
            "test://watched-resource",
        ))
        .await
        .expect("resources/unsubscribe");
    // Unsubscription is observable through resubscription: only a URI that
    // was actually dropped is "newly tracked" again, so a second
    // acknowledgment update must arrive.
    client
        .subscribe(rmcp::model::SubscribeRequestParams::new(
            "test://watched-resource",
        ))
        .await
        .expect("resubscribe");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if recorder.updated.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("second update proves the unsubscribe dropped the URI");
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn prompts_list_carries_names_descriptions_and_arguments() {
    let client = connect().await;

    let listed = client.list_prompts(None).await.expect("prompts/list");
    let names: Vec<&str> = listed.prompts.iter().map(|p| p.name.as_str()).collect();
    for required in [
        "test_simple_prompt",
        "test_prompt_with_arguments",
        "test_prompt_with_embedded_resource",
        "test_prompt_with_image",
    ] {
        assert!(names.contains(&required), "{required} missing: {names:?}");
    }
    assert!(
        listed.prompts.iter().all(|p| p.description.is_some()),
        "every prompt carries a description: {listed:?}"
    );
    let with_args = listed
        .prompts
        .iter()
        .find(|p| p.name == "test_prompt_with_arguments")
        .unwrap();
    let arg_names: Vec<&str> = with_args
        .arguments
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(arg_names, ["arg1", "arg2"]);

    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn prompts_get_returns_the_scenario_message_shapes() {
    let client = connect().await;

    let simple = client
        .get_prompt(rmcp::model::GetPromptRequestParams::new(
            "test_simple_prompt",
        ))
        .await
        .expect("simple prompt");
    let simple_json = serde_json::to_value(&simple).unwrap();
    assert_eq!(simple_json["messages"][0]["role"], "user");
    assert_eq!(
        simple_json["messages"][0]["content"]["text"],
        "This is a simple prompt for testing."
    );

    let mut with_args_params =
        rmcp::model::GetPromptRequestParams::new("test_prompt_with_arguments");
    with_args_params.arguments = serde_json::json!({"arg1": "hello", "arg2": "world"})
        .as_object()
        .cloned();
    let formatted = client
        .get_prompt(with_args_params)
        .await
        .expect("prompt with args");
    let formatted_json = serde_json::to_value(&formatted).unwrap();
    assert_eq!(
        formatted_json["messages"][0]["content"]["text"],
        "Prompt with arguments: arg1='hello', arg2='world'"
    );

    let mut embedded_params =
        rmcp::model::GetPromptRequestParams::new("test_prompt_with_embedded_resource");
    embedded_params.arguments = serde_json::json!({"resourceUri": "test://static-text"})
        .as_object()
        .cloned();
    let embedded = client
        .get_prompt(embedded_params)
        .await
        .expect("prompt with embedded resource");
    let embedded_json = serde_json::to_value(&embedded).unwrap();
    assert_eq!(
        embedded_json["messages"][0]["content"]["type"], "resource",
        "{embedded_json}"
    );
    assert_eq!(
        embedded_json["messages"][0]["content"]["resource"]["uri"],
        "test://static-text"
    );
    assert_eq!(embedded_json["messages"][1]["content"]["type"], "text");

    let image = client
        .get_prompt(rmcp::model::GetPromptRequestParams::new(
            "test_prompt_with_image",
        ))
        .await
        .expect("prompt with image");
    let image_json = serde_json::to_value(&image).unwrap();
    assert_eq!(image_json["messages"][0]["content"]["type"], "image");
    assert_eq!(
        image_json["messages"][0]["content"]["mimeType"],
        "image/png"
    );
    assert_eq!(image_json["messages"][1]["content"]["type"], "text");

    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn completion_filters_the_documented_candidates_by_prefix() {
    let client = connect().await;
    let mut params = rmcp::model::CompleteRequestParams::new(
        rmcp::model::Reference::for_prompt("test_prompt_with_arguments"),
        rmcp::model::ArgumentInfo {
            name: "arg1".into(),
            value: "par".into(),
        },
    );
    params.context = None;
    let result = client.complete(params).await.expect("completion/complete");
    assert_eq!(result.completion.values, ["paris", "park", "party"]);
    assert_eq!(result.completion.has_more, Some(false));

    let narrowed = client
        .complete(rmcp::model::CompleteRequestParams::new(
            rmcp::model::Reference::for_prompt("test_prompt_with_arguments"),
            rmcp::model::ArgumentInfo {
                name: "arg1".into(),
                value: "pari".into(),
            },
        ))
        .await
        .expect("narrowed completion");
    assert_eq!(narrowed.completion.values, ["paris"]);

    let unrelated = client
        .complete(rmcp::model::CompleteRequestParams::new(
            rmcp::model::Reference::for_prompt("some_other_prompt"),
            rmcp::model::ArgumentInfo {
                name: "arg1".into(),
                value: "x".into(),
            },
        ))
        .await
        .expect("unrelated completion");
    assert!(unrelated.completion.values.is_empty());

    // Right prompt, wrong argument: the guard requires BOTH name matches —
    // arg2 completes to nothing even with a matching prefix value.
    let wrong_argument = client
        .complete(rmcp::model::CompleteRequestParams::new(
            rmcp::model::Reference::for_prompt("test_prompt_with_arguments"),
            rmcp::model::ArgumentInfo {
                name: "arg2".into(),
                value: "par".into(),
            },
        ))
        .await
        .expect("wrong-argument completion");
    assert!(wrong_argument.completion.values.is_empty());

    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn set_level_filters_subsequent_log_notifications() {
    let (client, recorder) = connect_recording().await;

    client
        .set_level(rmcp::model::SetLevelRequestParams::new(
            rmcp::model::LoggingLevel::Error,
        ))
        .await
        .expect("logging/setLevel");

    let result = client
        .call_tool(CallToolRequestParams::new("test_tool_with_logging"))
        .await
        .expect("logging tool above threshold");
    assert_eq!(
        result.content[0].as_text().map(|t| t.text.as_str()),
        Some("Tool with logging executed successfully.")
    );
    assert!(
        recorder.logs.lock().unwrap().is_empty(),
        "info messages must be filtered at error threshold"
    );

    client
        .set_level(rmcp::model::SetLevelRequestParams::new(
            rmcp::model::LoggingLevel::Debug,
        ))
        .await
        .expect("loosen level");
    let _ = client
        .call_tool(CallToolRequestParams::new("test_tool_with_logging"))
        .await
        .expect("logging tool below threshold");
    assert_eq!(
        recorder.logs.lock().unwrap().len(),
        3,
        "messages flow again once the threshold drops"
    );

    client.cancel().await.expect("clean shutdown");
}

/// Captured `elicitation/create` request parameters, as raw JSON.
type ElicitationCaptures = std::sync::Arc<std::sync::Mutex<Vec<serde_json::Value>>>;

/// Client scripted to answer sampling and elicitation requests, capturing
/// the elicitation schemas so their wire shapes are assertable.
#[derive(Debug, Clone, Default)]
struct InteractiveClient {
    elicitations: ElicitationCaptures,
}

impl rmcp::ClientHandler for InteractiveClient {
    fn get_info(&self) -> rmcp::model::ClientInfo {
        let mut info = rmcp::model::ClientInfo::default();
        info.capabilities = rmcp::model::ClientCapabilities::builder()
            .enable_sampling()
            .enable_elicitation()
            .build();
        info
    }

    async fn create_message(
        &self,
        _params: rmcp::model::CreateMessageRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleClient>,
    ) -> Result<rmcp::model::CreateMessageResult, rmcp::ErrorData> {
        Ok(rmcp::model::CreateMessageResult::new(
            rmcp::model::SamplingMessage::assistant_text("Scripted response"),
            "test-model".to_owned(),
        ))
    }

    async fn create_elicitation(
        &self,
        params: rmcp::model::CreateElicitationRequestParams,
        _context: rmcp::service::RequestContext<rmcp::RoleClient>,
    ) -> Result<rmcp::model::CreateElicitationResult, rmcp::ErrorData> {
        self.elicitations
            .lock()
            .unwrap()
            .push(serde_json::to_value(&params).unwrap());
        let mut result =
            rmcp::model::CreateElicitationResult::new(rmcp::model::ElicitationAction::Accept);
        result.content = Some(serde_json::json!({"username": "tester", "email": "t@example.com"}));
        Ok(result)
    }
}

async fn connect_interactive() -> (
    RunningService<RoleClient, InteractiveClient>,
    InteractiveClient,
) {
    let (server_io, client_io) = tokio::io::duplex(4096);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    let handler = InteractiveClient::default();
    let client = handler
        .clone()
        .serve(client_io)
        .await
        .expect("client initialize");
    (client, handler)
}

#[tokio::test]
async fn sampling_tool_round_trips_through_the_client() {
    let (client, _) = connect_interactive().await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("test_sampling").with_arguments(
                serde_json::json!({"prompt": "Say hello"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .expect("test_sampling");
    assert_eq!(
        result.content[0].as_text().map(|t| t.text.as_str()),
        Some("LLM response: Scripted response")
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn sampling_tool_errors_when_the_client_lacks_the_capability() {
    // The trivial `()` client advertises no sampling capability.
    let client = connect().await;
    let outcome = client
        .call_tool(
            CallToolRequestParams::new("test_sampling").with_arguments(
                serde_json::json!({"prompt": "Say hello"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await;
    // The capability gate must be what rejected this — INVALID_REQUEST with
    // the gate's message. A bare is_err() cannot tell "rejected by the gate"
    // from "the gate is gone and the doomed sampling/createMessage failed
    // downstream as -32603", which is an illegal request on the wire.
    let error = mcp_error(outcome);
    assert_eq!(
        error.code,
        ErrorCode::INVALID_REQUEST,
        "the capability gate rejects before any sampling request: {error:?}"
    );
    assert!(
        error.message.contains("sampling"),
        "the rejection names the missing capability: {error:?}"
    );
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn elicitation_tool_sends_the_contract_schema_and_formats_the_reply() {
    let (client, handler) = connect_interactive().await;
    let result = client
        .call_tool(
            CallToolRequestParams::new("test_elicitation").with_arguments(
                serde_json::json!({"message": "Please provide your details"})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .expect("test_elicitation");
    let text = result.content[0]
        .as_text()
        .map(|t| t.text.as_str())
        .unwrap();
    assert!(
        text.starts_with("User response: action=accept, content="),
        "scenario phrasing: {text}"
    );
    assert!(text.contains("tester"), "scripted content echoed: {text}");

    let captured = handler.elicitations.lock().unwrap().clone();
    assert_eq!(captured.len(), 1);
    let request = &captured[0];
    assert_eq!(request["mode"], "form");
    assert_eq!(request["message"], "Please provide your details");
    let schema = &request["requestedSchema"];
    assert_eq!(schema["type"], "object");
    assert_eq!(
        schema["properties"]["username"]["description"],
        "User's response"
    );
    assert_eq!(
        schema["properties"]["email"]["description"],
        "User's email address"
    );
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(required.contains(&"username") && required.contains(&"email"));
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn sep1034_defaults_reach_the_wire_for_every_primitive() {
    let (client, handler) = connect_interactive().await;
    let result = client
        .call_tool(CallToolRequestParams::new(
            "test_elicitation_sep1034_defaults",
        ))
        .await
        .expect("sep1034 tool");
    let text = result.content[0]
        .as_text()
        .map(|t| t.text.as_str())
        .unwrap();
    assert!(text.starts_with("Elicitation completed: action=accept"));

    let captured = handler.elicitations.lock().unwrap().clone();
    let schema = &captured[0]["requestedSchema"]["properties"];
    assert_eq!(schema["name"]["default"], "John Doe");
    assert_eq!(schema["age"]["default"], 30);
    assert_eq!(schema["score"]["default"], 95.5);
    assert_eq!(schema["status"]["default"], "active");
    assert_eq!(
        schema["status"]["enum"],
        serde_json::json!(["active", "inactive", "pending"])
    );
    assert_eq!(schema["verified"]["default"], true);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn sep1330_sends_all_five_enum_variants() {
    let (client, handler) = connect_interactive().await;
    let _ = client
        .call_tool(CallToolRequestParams::new("test_elicitation_sep1330_enums"))
        .await
        .expect("sep1330 tool");

    let captured = handler.elicitations.lock().unwrap().clone();
    let props = &captured[0]["requestedSchema"]["properties"];

    // 1. Untitled single-select: type string + enum.
    assert_eq!(props["untitledSingle"]["type"], "string");
    assert_eq!(
        props["untitledSingle"]["enum"],
        serde_json::json!(["option1", "option2", "option3"])
    );
    // 2. Titled single-select: oneOf const/title.
    assert_eq!(
        props["titledSingle"]["oneOf"][0],
        serde_json::json!({"const": "value1", "title": "First Option"})
    );
    // 3. Legacy: the enum values survive the round-trip; `enumNames` does
    // NOT — rmcp's client-side untagged EnumSchema deserialization matches
    // the legacy form as Untitled first and drops the field. The true wire
    // shape (enumNames included) is pinned at serialization in
    // interactive.rs's unit tests; this assertion documents the loss so an
    // upstream fix is immediately visible.
    assert_eq!(
        props["legacyEnum"]["enum"],
        serde_json::json!(["opt1", "opt2", "opt3"])
    );
    assert_eq!(
        props["legacyEnum"]["enumNames"],
        serde_json::Value::Null,
        "rmcp round-trip currently drops enumNames; a value here means \
         upstream fixed their untagged ordering — update this test and the \
         register row"
    );
    // 4. Untitled multi-select: array of enum items.
    assert_eq!(props["untitledMulti"]["type"], "array");
    assert_eq!(
        props["untitledMulti"]["items"]["enum"],
        serde_json::json!(["option1", "option2", "option3"])
    );
    // 5. Titled multi-select: array of anyOf const/title.
    assert_eq!(props["titledMulti"]["type"], "array");
    assert_eq!(
        props["titledMulti"]["items"]["anyOf"][0],
        serde_json::json!({"const": "value1", "title": "First Choice"})
    );
    client.cancel().await.expect("clean shutdown");
}
