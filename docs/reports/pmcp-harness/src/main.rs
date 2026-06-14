// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)
//
// Conformance harness for the community Rust MCP SDK `pmcp` (crates.io `pmcp` 2.9.0).
//
// This is a STANDALONE, NON-WORKSPACE cargo project. It builds a pmcp
// streamable-HTTP MCP server that implements the full server surface exercised by
// `@modelcontextprotocol/conformance@0.1.16` (spec revision 2025-11-25), so the
// suite's `server` scenarios can be run against pmcp and the real per-check
// verdicts captured.
//
// Design note on *faithfulness*: every tool/prompt/resource below is implemented
// using the most expressive public pmcp API available for that capability. Where
// pmcp's API cannot express the wire shape the suite requires, the handler still
// returns the spec-ideal value (so the residual failure is attributable to pmcp's
// serialization/dispatch, not to a wiring gap in this harness).

use async_trait::async_trait;
use pmcp::types::capabilities::{
    PromptCapabilities, ResourceCapabilities, ServerCapabilities, ToolCapabilities,
};
use pmcp::types::{
    Content, GetPromptResult, ListResourcesResult, PromptArgument, PromptInfo, PromptMessage,
    ReadResourceResult, ResourceInfo, ToolInfo,
};
use pmcp::{
    PromptHandler, RequestHandlerExtra, ResourceHandler, Server, ToolHandler,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

// A 1x1 red-pixel PNG, base64-encoded (smallest valid PNG).
const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
// A minimal 44-byte WAV header (RIFF/WAVE, no samples), base64-encoded.
const WAV_EMPTY: &str = "UklGRiQAAABXQVZFZm10IBAAAAABAAEARKwAAIhYAQACABAAZGF0YQAAAAA=";

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// Helper: build a ToolInfo with a description and a trivial empty-object schema.
fn tool_info(name: &str, description: &str) -> ToolInfo {
    ToolInfo::new(
        name,
        Some(description.to_string()),
        json!({ "type": "object", "properties": {} }),
    )
}

/// `test_simple_text` — returns a single text content block.
struct SimpleText;
#[async_trait]
impl ToolHandler for SimpleText {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Ok(json!({
            "content": [ { "type": "text", "text": "This is a simple text response for testing." } ]
        }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_simple_text", "Returns a simple text response."))
    }
}

/// `test_image_content` — ideal shape includes an image content block.
struct ImageContent;
#[async_trait]
impl ToolHandler for ImageContent {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Ok(json!({
            "content": [ { "type": "image", "data": PNG_1X1, "mimeType": "image/png" } ]
        }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_image_content", "Returns image content."))
    }
}

/// `test_audio_content` — ideal shape includes an audio content block.
struct AudioContent;
#[async_trait]
impl ToolHandler for AudioContent {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Ok(json!({
            "content": [ { "type": "audio", "data": WAV_EMPTY, "mimeType": "audio/wav" } ]
        }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_audio_content", "Returns audio content."))
    }
}

/// `test_embedded_resource` — ideal nested embedded-resource content.
struct EmbeddedResource;
#[async_trait]
impl ToolHandler for EmbeddedResource {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Ok(json!({
            "content": [ {
                "type": "resource",
                "resource": {
                    "uri": "test://embedded-resource",
                    "mimeType": "text/plain",
                    "text": "This is an embedded resource content."
                }
            } ]
        }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_embedded_resource", "Returns embedded resource content."))
    }
}

/// `test_multiple_content_types` — text + image + embedded resource.
struct MultipleContent;
#[async_trait]
impl ToolHandler for MultipleContent {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Ok(json!({
            "content": [
                { "type": "text", "text": "Multiple content types test:" },
                { "type": "image", "data": PNG_1X1, "mimeType": "image/png" },
                { "type": "resource", "resource": {
                    "uri": "test://mixed-content-resource",
                    "mimeType": "application/json",
                    "text": "{\"test\":\"data\",\"value\":123}"
                } }
            ]
        }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_multiple_content_types", "Returns multiple content types."))
    }
}

/// `test_tool_with_logging` — emits 3 info log notifications during execution.
struct ToolWithLogging;
#[async_trait]
impl ToolHandler for ToolWithLogging {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        use pmcp::types::protocol::LogLevel;
        pmcp::log(LogLevel::Info, "Tool execution started", None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        pmcp::log(LogLevel::Info, "Tool processing data", None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        pmcp::log(LogLevel::Info, "Tool execution completed", None).await;
        Ok(json!({ "content": [ { "type": "text", "text": "Logging tool completed." } ] }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_tool_with_logging", "Emits log notifications during execution."))
    }
}

/// `test_error_handling` — must produce a result with `isError: true`.
///
/// A `ToolHandler` returning `Ok(Value)` cannot set `isError` (pmcp stringifies
/// the whole value into a single text block; see the content-type tools). The
/// idiomatic pmcp way to flag a tool-level error is `Error::tool_rejected`,
/// which pmcp's `tools/call` dispatch maps to a `CallToolResult { isError: true,
/// content: [text(message)] }` — exactly the shape the suite expects.
struct ErrorHandling;
#[async_trait]
impl ToolHandler for ErrorHandling {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Err(pmcp::Error::tool_rejected(
            "This tool intentionally returns an error for testing",
            None,
        ))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_error_handling", "Always returns an error result."))
    }
}

/// `test_tool_with_progress` — attempts to send 3 progress notifications
/// referencing the client's `_meta.progressToken`.
///
/// Over `StreamableHttpServer`, NO progress can actually be emitted in pmcp
/// 2.9.0: `Server::notification_tx` and the server-request dispatcher/peer are
/// wired ONLY inside `Server::run<T: Transport>()` (the stdio/WebSocket loop),
/// never by `StreamableHttpServer` (which calls `Server::handle_request`
/// directly on the locked `Server`). Consequently here:
///   * `extra.report_progress(..)` is a silent no-op — its `progress_reporter`
///     is `None` because `notification_tx` is `None`.
///   * `extra.peer()` is `None`, so the peer path can't run at all; and even
///     when populated, `DispatchPeerHandle::progress_notify` is a documented
///     no-op in 2.9.0 (peer_impl.rs).
/// We invoke both paths anyway (the spec-ideal behavior); the suite observes
/// zero notifications, which is the true pmcp/StreamableHttp limitation.
struct ToolWithProgress;
#[async_trait]
impl ToolHandler for ToolWithProgress {
    async fn handle(&self, _args: Value, extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        // Idiomatic path: the reporter auto-derives the client's progressToken
        // from `_meta` when `notification_tx` is wired (stdio/WS only).
        let _ = extra.report_progress(0.0, Some(100.0), None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = extra.report_progress(50.0, Some(100.0), None).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let _ = extra.report_progress(100.0, Some(100.0), None).await;
        // Peer path (no-op under HTTP; present for completeness).
        if let Some(peer) = extra.peer() {
            use pmcp::types::ProgressToken;
            let token = ProgressToken::String("progress-test-1".to_string());
            let _ = peer.progress_notify(token, 100.0, Some(100.0), None).await;
        }
        Ok(json!({ "content": [ { "type": "text", "text": "Progress tool completed." } ] }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info("test_tool_with_progress", "Reports progress notifications."))
    }
}

/// `test_sampling` — requests `sampling/createMessage` from the client via the
/// transport-wired peer back-channel.
struct SamplingTool;
#[async_trait]
impl ToolHandler for SamplingTool {
    async fn handle(&self, args: Value, extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        use pmcp::types::{CreateMessageParams, SamplingMessage, SamplingMessageContent, Role};
        let prompt = args.get("prompt").and_then(|v| v.as_str()).unwrap_or("Hello");
        let peer = extra
            .peer()
            .ok_or_else(|| pmcp::Error::protocol(
                pmcp::ErrorCode::INTERNAL_ERROR,
                "Client does not support sampling (no peer back-channel)",
            ))?;
        let mut params = CreateMessageParams::new(vec![SamplingMessage::new(
            Role::User,
            SamplingMessageContent::Text { text: prompt.to_string(), meta: None },
        )]);
        params.max_tokens = Some(100);
        let result = peer.sample(params).await?;
        let text = match &result.content {
            Content::Text { text } => text.clone(),
            _ => "<non-text sampling response>".to_string(),
        };
        Ok(json!({ "content": [ { "type": "text", "text": format!("LLM response: {}", text) } ] }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(ToolInfo::new(
            "test_sampling",
            Some("Requests LLM sampling from the client.".to_string()),
            json!({
                "type": "object",
                "properties": { "prompt": { "type": "string" } },
                "required": ["prompt"]
            }),
        ))
    }
}

/// `test_elicitation` — the suite expects this to issue `elicitation/create`.
/// pmcp 2.9.0 exposes NO transport-wired path for a tool handler to send an
/// elicitation request to the client: `PeerHandle` (the only back-channel a
/// handler receives via `extra.peer()`) provides only `sample`, `list_roots`,
/// and `progress_notify`. `ElicitationManager`/`ElicitationContext` exist but
/// are standalone (require a manually-set mpsc channel + manual response
/// delivery) and are not wired to `StreamableHttpServer`'s request dispatcher.
/// We therefore cannot make a real `elicitation/create` go out. The handler
/// returns the spec-ideal result shape so the residual failure is attributable
/// purely to "no elicitation was requested".
struct ElicitationTool {
    schema: Value,
    label: &'static str,
    name: &'static str,
}
#[async_trait]
impl ToolHandler for ElicitationTool {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        // No public pmcp API to emit elicitation/create over the live transport.
        // Returning ideal content; the suite will report "Server did not request
        // elicitation from client", which is the true pmcp limitation.
        let _ = &self.schema; // schema is what we *would* send if the API allowed it
        Ok(json!({ "content": [ {
            "type": "text",
            "text": format!("{}: elicitation not supported by pmcp transport API", self.label)
        } ] }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(tool_info(self.name, "Requests user input (elicitation) from the client."))
    }
}

/// `json_schema_2020_12_tool` — advertises an inputSchema carrying JSON Schema
/// 2020-12 keywords (`$schema`, `$defs`, `additionalProperties: false`). pmcp's
/// tools/list passes ToolInfo.input_schema through verbatim (no normalization),
/// so these keywords are preserved.
struct JsonSchema2020Tool;
#[async_trait]
impl ToolHandler for JsonSchema2020Tool {
    async fn handle(&self, _args: Value, _extra: RequestHandlerExtra) -> pmcp::Result<Value> {
        Ok(json!({ "content": [ { "type": "text", "text": "ok" } ] }))
    }
    fn metadata(&self) -> Option<ToolInfo> {
        Some(ToolInfo::new(
            "json_schema_2020_12_tool",
            Some("Tool with JSON Schema 2020-12 features".to_string()),
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "$defs": {
                    "address": {
                        "type": "object",
                        "properties": {
                            "street": { "type": "string" },
                            "city": { "type": "string" }
                        }
                    }
                },
                "properties": {
                    "name": { "type": "string" },
                    "address": { "$ref": "#/$defs/address" }
                },
                "additionalProperties": false
            }),
        ))
    }
}

// ---------------------------------------------------------------------------
// Resources
// ---------------------------------------------------------------------------

/// Serves `test://static-text`, `test://static-binary`, and the template
/// `test://template/{id}/data` (matched by prefix on read).
struct Resources;
#[async_trait]
impl ResourceHandler for Resources {
    async fn read(
        &self,
        uri: &str,
        _extra: RequestHandlerExtra,
    ) -> pmcp::Result<ReadResourceResult> {
        if uri == "test://static-text" {
            Ok(ReadResourceResult::new(vec![Content::Resource {
                uri: uri.to_string(),
                text: Some("This is the content of the static text resource.".to_string()),
                mime_type: Some("text/plain".to_string()),
                meta: None,
            }]))
        } else if uri == "test://static-binary" {
            // pmcp's Content::Resource has NO `blob` field (only `text`), and its
            // resource_contents_serde never emits a `blob` key. There is no public
            // pmcp type to return base64 blob data from resources/read. We return
            // the closest pmcp can express; the suite requires `blob` and will fail.
            Ok(ReadResourceResult::new(vec![Content::Resource {
                uri: uri.to_string(),
                text: Some(PNG_1X1.to_string()),
                mime_type: Some("image/png".to_string()),
                meta: None,
            }]))
        } else if let Some(rest) = uri.strip_prefix("test://template/") {
            // rest = "{id}/data"
            let id = rest.split('/').next().unwrap_or("");
            Ok(ReadResourceResult::new(vec![Content::Resource {
                uri: uri.to_string(),
                text: Some(format!(
                    "{{\"id\":\"{id}\",\"templateTest\":true,\"data\":\"Data for ID: {id}\"}}"
                )),
                mime_type: Some("application/json".to_string()),
                meta: None,
            }]))
        } else {
            Err(pmcp::Error::protocol(
                pmcp::ErrorCode::METHOD_NOT_FOUND,
                format!("Resource not found: {uri}"),
            ))
        }
    }

    async fn list(
        &self,
        _cursor: Option<String>,
        _extra: RequestHandlerExtra,
    ) -> pmcp::Result<ListResourcesResult> {
        Ok(ListResourcesResult::new(vec![
            ResourceInfo::new("test://static-text", "Static Text")
                .with_description("A static text resource")
                .with_mime_type("text/plain"),
            ResourceInfo::new("test://static-binary", "Static Binary")
                .with_description("A static binary resource")
                .with_mime_type("image/png"),
        ]))
    }
}

// ---------------------------------------------------------------------------
// Prompts
// ---------------------------------------------------------------------------

struct SimplePromptH;
#[async_trait]
impl PromptHandler for SimplePromptH {
    async fn handle(
        &self,
        _args: HashMap<String, String>,
        _extra: RequestHandlerExtra,
    ) -> pmcp::Result<GetPromptResult> {
        Ok(GetPromptResult::new(
            vec![PromptMessage::user(Content::text(
                "This is a simple prompt for testing.",
            ))],
            Some("A simple prompt".to_string()),
        ))
    }
    fn metadata(&self) -> Option<PromptInfo> {
        Some(
            PromptInfo::new("test_simple_prompt")
                .with_description("A simple prompt for testing"),
        )
    }
}

struct PromptWithArgs;
#[async_trait]
impl PromptHandler for PromptWithArgs {
    async fn handle(
        &self,
        args: HashMap<String, String>,
        _extra: RequestHandlerExtra,
    ) -> pmcp::Result<GetPromptResult> {
        let arg1 = args.get("arg1").cloned().unwrap_or_default();
        let arg2 = args.get("arg2").cloned().unwrap_or_default();
        Ok(GetPromptResult::new(
            vec![PromptMessage::user(Content::text(format!(
                "Prompt with arguments: arg1='{arg1}', arg2='{arg2}'"
            )))],
            Some("Prompt with arguments".to_string()),
        ))
    }
    fn metadata(&self) -> Option<PromptInfo> {
        Some(
            PromptInfo::new("test_prompt_with_arguments")
                .with_description("A prompt with arguments")
                .with_arguments(vec![
                    PromptArgument::new("arg1")
                        .with_description("First test argument")
                        .required(),
                    PromptArgument::new("arg2")
                        .with_description("Second test argument")
                        .required(),
                ]),
        )
    }
}

struct PromptWithEmbedded;
#[async_trait]
impl PromptHandler for PromptWithEmbedded {
    async fn handle(
        &self,
        args: HashMap<String, String>,
        _extra: RequestHandlerExtra,
    ) -> pmcp::Result<GetPromptResult> {
        let uri = args
            .get("resourceUri")
            .cloned()
            .unwrap_or_else(|| "test://example-resource".to_string());
        Ok(GetPromptResult::new(
            vec![
                PromptMessage::user(Content::Resource {
                    uri,
                    text: Some("Embedded resource content for testing.".to_string()),
                    mime_type: Some("text/plain".to_string()),
                    meta: None,
                }),
                PromptMessage::user(Content::text("Please process the embedded resource above.")),
            ],
            Some("Prompt with embedded resource".to_string()),
        ))
    }
    fn metadata(&self) -> Option<PromptInfo> {
        Some(
            PromptInfo::new("test_prompt_with_embedded_resource")
                .with_description("A prompt with an embedded resource")
                .with_arguments(vec![PromptArgument::new("resourceUri")
                    .with_description("URI of the resource to embed")
                    .required()]),
        )
    }
}

struct PromptWithImage;
#[async_trait]
impl PromptHandler for PromptWithImage {
    async fn handle(
        &self,
        _args: HashMap<String, String>,
        _extra: RequestHandlerExtra,
    ) -> pmcp::Result<GetPromptResult> {
        Ok(GetPromptResult::new(
            vec![
                PromptMessage::user(Content::image(PNG_1X1, "image/png")),
                PromptMessage::user(Content::text("Please analyze the image above.")),
            ],
            Some("Prompt with image".to_string()),
        ))
    }
    fn metadata(&self) -> Option<PromptInfo> {
        Some(
            PromptInfo::new("test_prompt_with_image")
                .with_description("A prompt with image content"),
        )
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn elicit_sep1034_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "name":     { "type": "string",  "default": "John Doe" },
            "age":      { "type": "integer", "default": 30 },
            "score":    { "type": "number",  "default": 95.5 },
            "status":   { "type": "string",  "enum": ["active","inactive","pending"], "default": "active" },
            "verified": { "type": "boolean", "default": true }
        }
    })
}

fn elicit_sep1330_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "untitledSingle": { "type": "string", "enum": ["option1","option2","option3"] },
            "titledSingle":   { "type": "string", "oneOf": [
                { "const": "value1", "title": "First Option" },
                { "const": "value2", "title": "Second Option" },
                { "const": "value3", "title": "Third Option" }
            ] },
            "legacyEnum":     { "type": "string", "enum": ["opt1","opt2","opt3"],
                                "enumNames": ["Option One","Option Two","Option Three"] },
            "untitledMulti":  { "type": "array", "items": { "type": "string", "enum": ["option1","option2","option3"] } },
            "titledMulti":    { "type": "array", "items": { "anyOf": [
                { "const": "value1", "title": "First Choice" },
                { "const": "value2", "title": "Second Choice" },
                { "const": "value3", "title": "Third Choice" }
            ] } }
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8181);
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();

    // Enable tools + resources (with subscribe) + prompts + logging + completions.
    // ServerCapabilities is #[non_exhaustive], so build from default and set the
    // public fields rather than using struct-literal syntax.
    let mut capabilities = ServerCapabilities::default();
    capabilities.tools = Some(ToolCapabilities { list_changed: Some(true) });
    capabilities.resources = Some(ResourceCapabilities {
        subscribe: Some(true),
        list_changed: Some(true),
    });
    capabilities.prompts = Some(PromptCapabilities { list_changed: Some(true) });
    capabilities.logging = Some(Default::default());
    capabilities.completions = Some(Default::default());

    let server = Server::builder()
        .name("pmcp-conformance-harness")
        .version("0.1.0")
        .capabilities(capabilities)
        // tools
        .tool("test_simple_text", SimpleText)
        .tool("test_image_content", ImageContent)
        .tool("test_audio_content", AudioContent)
        .tool("test_embedded_resource", EmbeddedResource)
        .tool("test_multiple_content_types", MultipleContent)
        .tool("test_tool_with_logging", ToolWithLogging)
        .tool("test_error_handling", ErrorHandling)
        .tool("test_tool_with_progress", ToolWithProgress)
        .tool("test_sampling", SamplingTool)
        .tool("test_elicitation", ElicitationTool {
            schema: json!({
                "type": "object",
                "properties": {
                    "username": { "type": "string", "description": "User's response" },
                    "email": { "type": "string", "description": "User's email address" }
                },
                "required": ["username", "email"]
            }),
            label: "test_elicitation",
            name: "test_elicitation",
        })
        .tool("test_elicitation_sep1034_defaults", ElicitationTool {
            schema: elicit_sep1034_schema(),
            label: "test_elicitation_sep1034_defaults",
            name: "test_elicitation_sep1034_defaults",
        })
        .tool("test_elicitation_sep1330_enums", ElicitationTool {
            schema: elicit_sep1330_schema(),
            label: "test_elicitation_sep1330_enums",
            name: "test_elicitation_sep1330_enums",
        })
        .tool("json_schema_2020_12_tool", JsonSchema2020Tool)
        // resources
        .resources(Resources)
        // prompts
        .prompt("test_simple_prompt", SimplePromptH)
        .prompt("test_prompt_with_arguments", PromptWithArgs)
        .prompt("test_prompt_with_embedded_resource", PromptWithEmbedded)
        .prompt("test_prompt_with_image", PromptWithImage)
        .build()?;

    let http = pmcp::server::streamable_http_server::StreamableHttpServer::new(
        addr,
        Arc::new(tokio::sync::Mutex::new(server)),
    );
    let (bound, handle) = http.start().await?;
    eprintln!("pmcp-conformance-harness listening on http://{bound}/");
    handle.await?;
    Ok(())
}
