// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tool surface of the everything server.
//!
//! Two sources define it. The official suite's server scenarios name exact
//! tools (`test_simple_text`, `test_image_content`, …) with exact response
//! shapes — those are implemented verbatim, each documenting the scenario it
//! satisfies. The TypeScript everything server contributes `echo`, `add`, and
//! `get-structured-content` (register 2.10 parity; the suite exercises none
//! of the three). Divergence from either source is a reviewable decision,
//! never an accident.

use std::sync::Arc;

use rmcp::handler::server::router::tool::{ToolRoute, ToolRouter};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    AnnotateAble as _, CallToolResult, Content, RawAudioContent, RawContent, ResourceContents, Tool,
};
use rmcp::{ErrorData, tool, tool_router};

use crate::fixtures::{TINY_PNG_BASE64, TINY_WAV_BASE64};
use crate::server::EverythingServer;

/// Name of the JSON Schema 2020-12 keyword-preservation tool (SEP-1613); the
/// `json-schema-2020-12` scenario looks it up by exactly this name.
pub const JSON_SCHEMA_TOOL_NAME: &str = "json_schema_2020_12_tool";

/// Arguments for [`EverythingServer::echo`].
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EchoArgs {
    /// Message to echo back unchanged.
    pub message: String,
}

/// Arguments for [`EverythingServer::get_structured_content`].
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct StructuredContentArgs {
    /// Choose city.
    pub location: Location,
}

/// The cities the TypeScript everything server's weather fixture knows; the
/// wire values are its exact zod enum (`"New York"`, `"Chicago"`,
/// `"Los Angeles"`).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub enum Location {
    /// Fixture: 33 °C, cloudy, 82 %.
    #[serde(rename = "New York")]
    NewYork,
    /// Fixture: 36 °C, light rain, 82 %.
    Chicago,
    /// Fixture: 73 °C, sunny, 48 %.
    #[serde(rename = "Los Angeles")]
    LosAngeles,
}

/// Structured weather report returned by
/// [`EverythingServer::get_structured_content`]; its derived schema is the
/// tool's `outputSchema`.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct WeatherOutput {
    /// Temperature in celsius.
    pub temperature: f64,
    /// Weather conditions description.
    pub conditions: String,
    /// Humidity percentage.
    pub humidity: f64,
}

/// Arguments for [`EverythingServer::add`].
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AddArgs {
    /// First addend.
    pub a: f64,
    /// Second addend.
    pub b: f64,
}

#[tool_router(router = tool_router_basic, vis = "pub(crate)")]
impl EverythingServer {
    /// `echo` — returns the input message, prefixed exactly like the TypeScript
    /// everything server (`Echo: <message>`).
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Echoes back the input")]
    pub fn echo(
        &self,
        Parameters(EchoArgs { message }): Parameters<EchoArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Echo: {message}"
        ))]))
    }

    /// `add` — adds two numbers, phrasing the result exactly like the
    /// TypeScript everything server.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Adds two numbers")]
    pub fn add(
        &self,
        Parameters(AddArgs { a, b }): Parameters<AddArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(format!(
            "The sum of {a} and {b} is {sum}.",
            sum = a + b
        ))]))
    }

    /// `get-structured-content` — the TypeScript everything server's
    /// structured-output tool (register 2.10 parity): a fixed weather fixture
    /// returned as `structuredContent` under a derived `outputSchema`, plus
    /// the backward-compatible JSON text block — the exact pairing TOOL-010
    /// and TOOL-011 require of any server that declares an output schema.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(
        name = "get-structured-content",
        description = "Returns structured content along with an output schema for client data validation"
    )]
    pub fn get_structured_content(
        &self,
        Parameters(StructuredContentArgs { location }): Parameters<StructuredContentArgs>,
    ) -> Result<Json<WeatherOutput>, ErrorData> {
        let (temperature, conditions, humidity) = match location {
            Location::NewYork => (33.0, "Cloudy", 82.0),
            Location::Chicago => (36.0, "Light rain / drizzle", 82.0),
            Location::LosAngeles => (73.0, "Sunny / Clear", 48.0),
        };
        Ok(Json(WeatherOutput {
            temperature,
            conditions: conditions.to_owned(),
            humidity,
        }))
    }

    /// `tools-call-simple-text`: exact text the scenario checks for.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Returns a simple text response for conformance testing")]
    pub fn test_simple_text(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            "This is a simple text response for testing.",
        )]))
    }

    /// `tools-call-image`: a minimal PNG as base64 image content.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Returns image content for conformance testing")]
    pub fn test_image_content(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::image(
            TINY_PNG_BASE64,
            "image/png",
        )]))
    }

    /// `tools-call-audio`: a minimal WAV as base64 audio content.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Returns audio content for conformance testing")]
    pub fn test_audio_content(&self) -> Result<CallToolResult, ErrorData> {
        // No `Content::audio` convenience constructor exists in rmcp 1.7;
        // build the variant directly.
        Ok(CallToolResult::success(vec![
            RawContent::Audio(RawAudioContent {
                data: TINY_WAV_BASE64.into(),
                mime_type: "audio/wav".into(),
            })
            .no_annotation(),
        ]))
    }

    /// `tools-call-embedded-resource`: embedded text resource content.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Returns embedded resource content for conformance testing")]
    pub fn test_embedded_resource(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::resource(
            ResourceContents::TextResourceContents {
                uri: "test://embedded-resource".into(),
                mime_type: Some("text/plain".into()),
                text: "This is an embedded resource content.".into(),
                meta: None,
            },
        )]))
    }

    /// `tools-call-mixed-content`: text + image + embedded resource in one
    /// result, in the scenario's order.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(description = "Returns multiple content types for conformance testing")]
    pub fn test_multiple_content_types(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![
            Content::text("Multiple content types test:"),
            Content::image(TINY_PNG_BASE64, "image/png"),
            Content::resource(ResourceContents::TextResourceContents {
                uri: "test://mixed-content-resource".into(),
                mime_type: Some("application/json".into()),
                text: r#"{"test":"data","value":123}"#.into(),
                meta: None,
            }),
        ]))
    }

    /// `tools-call-error`: always reports a tool-level failure (`isError`),
    /// exercising error reporting without breaking the session.
    ///
    /// # Errors
    ///
    /// Never returns `Err`: the scenario requires an in-band `isError: true`
    /// result, not a protocol error.
    #[tool(description = "Intentionally returns an error result for conformance testing")]
    pub fn test_error_handling(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::error(vec![Content::text(
            "This tool intentionally returns an error for testing",
        )]))
    }
}

/// The `json-schema-2020-12` scenario's tool: its `inputSchema` must reach
/// `tools/list` byte-faithful — `$schema`, `$defs`, `$ref`,
/// `additionalProperties` all preserved (SEP-1613). A derived schema would
/// not guarantee that, so the route is built by hand from the verbatim JSON.
pub(crate) fn json_schema_2020_12_route() -> ToolRoute<EverythingServer> {
    // Built as a `Map` directly (not `json!({…})` then destructured) so there
    // is no Object-or-else branch and thus no `unreachable!` in shipping code;
    // the nested values still use `json!` for legibility. `serde_json::Map` is
    // `BTreeMap`-backed (no `preserve_order` feature), so key order — and the
    // byte-faithful `tools/list` schema the SEP-1613 scenario checks — is
    // identical to the previous form.
    let mut schema = serde_json::Map::new();
    schema.insert(
        "$schema".to_owned(),
        serde_json::json!("https://json-schema.org/draft/2020-12/schema"),
    );
    schema.insert("type".to_owned(), serde_json::json!("object"));
    schema.insert(
        "$defs".to_owned(),
        serde_json::json!({
            "address": {
                "type": "object",
                "properties": {
                    "street": { "type": "string" },
                    "city": { "type": "string" }
                }
            }
        }),
    );
    schema.insert(
        "properties".to_owned(),
        serde_json::json!({
            "name": { "type": "string" },
            "address": { "$ref": "#/$defs/address" }
        }),
    );
    schema.insert("additionalProperties".to_owned(), serde_json::json!(false));
    let tool = Tool::new(
        JSON_SCHEMA_TOOL_NAME,
        "Tool with JSON Schema 2020-12 features",
        Arc::new(schema),
    );
    ToolRoute::new_dyn(tool, |_context| {
        Box::pin(async {
            Ok(CallToolResult::success(vec![Content::text(
                "JSON Schema 2020-12 tool executed.",
            )]))
        })
    })
}

/// Combines every tool router the server exposes; [`EverythingServer::new`]
/// is the single caller, so the tool inventory has one assembly point.
pub(crate) fn all_tools() -> ToolRouter<EverythingServer> {
    let routed = EverythingServer::tool_router_basic()
        + EverythingServer::tool_router_notifying()
        + EverythingServer::tool_router_interactive();
    routed.with_route(json_schema_2020_12_route())
}
