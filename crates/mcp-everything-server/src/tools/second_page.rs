// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The tools `tools/list` serves on its second page ([`crate::pagination`]).
//!
//! The first page is the suite's contract and the TypeScript server's parity
//! tools, unchanged. These two exist for what that page never puts on the
//! wire:
//!
//! - [`RESOURCE_LINK_TOOL`] returns `resource_link` content — a link to a
//!   resource rather than its body — beside an annotated text block, and is
//!   itself listed with a title, tool annotations and icons.
//! - [`PARAMETER_HEADERS_TOOL`] designates three arguments for mirroring into
//!   `Mcp-Param-*` headers (`x-mcp-header`, SEP-2243): a required string, a
//!   required integer and an optional string. A conforming client calling it
//!   over Streamable HTTP mirrors the two it supplies and omits the one it
//!   leaves out, and the server checks each header against the body — so one
//!   ordinary call carries the header table's "value provided" and "not in
//!   arguments" rows, from both sides.

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, JsonObject, TextContent, Tool};
use rmcp::{ErrorData, tool, tool_router};

use crate::server::EverythingServer;
use crate::{fixtures, resources};

/// Name of the tool returning `resource_link` content.
pub const RESOURCE_LINK_TOOL: &str = "test_resource_link";

/// Name of the tool whose arguments are designated for header mirroring.
pub const PARAMETER_HEADERS_TOOL: &str = "test_parameter_headers";

/// Which page of `tools/list` a tool is listed on.
#[must_use]
pub fn page_of(tool: &Tool) -> usize {
    usize::from([RESOURCE_LINK_TOOL, PARAMETER_HEADERS_TOOL].contains(&tool.name.as_ref()))
}

/// Arguments of [`PARAMETER_HEADERS_TOOL`].
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ParameterHeadersArgs {
    /// Mirrored as `Mcp-Param-Region`.
    pub region: String,
    /// Mirrored as `Mcp-Param-Limit`.
    pub limit: i64,
    /// Mirrored as `Mcp-Param-Zone` when supplied.
    pub zone: Option<String>,
}

/// The `inputSchema` of [`PARAMETER_HEADERS_TOOL`], written out rather than
/// derived.
///
/// Derivation cannot produce it: `x-mcp-header` is not a keyword `schemars`
/// knows, and the derived form of `Option<String>` is the type *array*
/// `["string", "null"]`, which the designation forbids — a header carries a
/// primitive, and rmcp's client drops a tool whose designated property names
/// anything else. So the optional argument is a plain `string` left out of
/// `required`, which is exactly the shape the header table's "not in
/// arguments" row is about.
#[must_use]
pub fn parameter_headers_schema() -> Arc<JsonObject> {
    let mut schema = JsonObject::new();
    schema.insert("type".to_owned(), serde_json::json!("object"));
    schema.insert(
        "properties".to_owned(),
        serde_json::json!({
            "region": {
                "type": "string",
                "description": "Mirrored as Mcp-Param-Region",
                "x-mcp-header": "Region"
            },
            "limit": {
                "type": "integer",
                "description": "Mirrored as Mcp-Param-Limit",
                "x-mcp-header": "Limit"
            },
            "zone": {
                "type": "string",
                "description": "Mirrored as Mcp-Param-Zone when supplied",
                "x-mcp-header": "Zone"
            }
        }),
    );
    schema.insert(
        "required".to_owned(),
        serde_json::json!(["region", "limit"]),
    );
    Arc::new(schema)
}

#[tool_router(router = tool_router_second_page, vis = "pub(crate)")]
impl EverythingServer {
    /// `test_resource_link` — a `resource_link` to the annotated resource on
    /// `resources/list`'s second page, after an annotated text block.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(
        name = "test_resource_link",
        title = "Resource link",
        description = "Returns a resource_link to a listed resource, after annotated text",
        annotations(
            title = "Resource link",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        ),
        icons = crate::fixtures::icons()
    )]
    pub fn test_resource_link(&self) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![
            ContentBlock::Text(
                TextContent::new("The resource linked below is listed by resources/list.")
                    .with_annotations(fixtures::annotations()),
            ),
            ContentBlock::resource_link(resources::annotated()),
        ]))
    }

    /// `test_parameter_headers` — echoes the three designated arguments, so
    /// a recording shows the body each `Mcp-Param-*` header was checked
    /// against.
    ///
    /// The header check itself is rmcp's, in its Streamable HTTP layer, before
    /// this handler runs: a header that disagrees with the body, or a
    /// designated value sent without its header, is refused with `-32020`
    /// there, and this tool never sees the request.
    ///
    /// # Errors
    ///
    /// Never fails; the `Result` is the `#[tool]` calling convention.
    #[tool(
        name = "test_parameter_headers",
        description = "Echoes arguments designated for Mcp-Param-* header mirroring (x-mcp-header)",
        input_schema = parameter_headers_schema()
    )]
    pub fn test_parameter_headers(
        &self,
        Parameters(ParameterHeadersArgs {
            region,
            limit,
            zone,
        }): Parameters<ParameterHeadersArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        let zone = zone.as_deref().unwrap_or("(not supplied)");
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "region={region} limit={limit} zone={zone}"
        ))]))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn listed(name: &str) -> Tool {
        EverythingServer::tool_router_second_page()
            .get(name)
            .cloned()
            .unwrap()
    }

    #[test]
    fn both_tools_are_on_the_second_page_and_nothing_else_is() {
        assert_eq!(page_of(&listed(RESOURCE_LINK_TOOL)), 1);
        assert_eq!(page_of(&listed(PARAMETER_HEADERS_TOOL)), 1);
        assert_eq!(
            page_of(&Tool::new("echo", "", Arc::new(JsonObject::new()))),
            0
        );
    }

    #[test]
    fn the_resource_link_tool_lists_annotations_title_and_icons() {
        let value = serde_json::to_value(listed(RESOURCE_LINK_TOOL)).unwrap();
        assert_eq!(value["title"], "Resource link");
        assert_eq!(value["annotations"]["readOnlyHint"], true);
        assert_eq!(value["annotations"]["openWorldHint"], false);
        assert_eq!(value["icons"][0]["mimeType"], "image/png");
    }

    #[test]
    fn the_resource_link_has_the_schema_shape_and_names_a_listed_resource() {
        let result = EverythingServer::new().test_resource_link().unwrap();
        let value = serde_json::to_value(&result.content).unwrap();
        // `ResourceLink` is a `Resource` plus `type: "resource_link"`; `uri`
        // and `name` are required.
        let link = &value[1];
        assert_eq!(link["type"], "resource_link");
        assert_eq!(link["uri"], resources::ANNOTATED_URI);
        assert_eq!(link["name"], "annotated-text");
        assert!(
            resources::catalog()
                .iter()
                .any(|resource| resource.uri == resources::ANNOTATED_URI),
            "the link points at a resource the server lists"
        );
        assert_eq!(value[0]["annotations"]["priority"], 0.5);
    }

    #[test]
    fn every_designated_property_is_a_top_level_primitive() {
        let schema = parameter_headers_schema();
        let properties = schema["properties"].as_object().unwrap();
        let designated: Vec<(&str, &str)> = properties
            .iter()
            .map(|(name, property)| (name.as_str(), property["x-mcp-header"].as_str().unwrap()))
            .collect();
        assert_eq!(
            designated,
            [("limit", "Limit"), ("region", "Region"), ("zone", "Zone")]
        );
        for property in properties.values() {
            let kind = property["type"].as_str().unwrap();
            assert!(["string", "integer", "boolean"].contains(&kind), "{kind}");
        }
        assert_eq!(schema["required"], serde_json::json!(["region", "limit"]));
        // The listing carries the written-out schema, not a derived one.
        assert_eq!(listed(PARAMETER_HEADERS_TOOL).input_schema, schema);
    }

    #[test]
    fn the_parameter_tool_echoes_what_it_was_given() {
        let server = EverythingServer::new();
        let call = |zone: Option<&str>| {
            let result = server
                .test_parameter_headers(Parameters(ParameterHeadersArgs {
                    region: "us-west1".to_owned(),
                    limit: 7,
                    zone: zone.map(str::to_owned),
                }))
                .unwrap();
            serde_json::to_value(&result.content).unwrap()[0]["text"].clone()
        };
        assert_eq!(call(Some("b")), "region=us-west1 limit=7 zone=b");
        assert_eq!(call(None), "region=us-west1 limit=7 zone=(not supplied)");
    }
}
