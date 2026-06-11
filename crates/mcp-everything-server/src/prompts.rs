// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Prompt surface of the everything server.
//!
//! The suite's `prompts-*` scenarios name four prompts and their exact
//! message shapes; each handler documents the scenario it satisfies. The
//! doc comment on each `#[prompt]` method doubles as the prompt's listed
//! description (the scenarios require one).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{PromptMessage, PromptMessageRole};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, prompt, prompt_router};

use crate::fixtures::TINY_PNG_BASE64;
use crate::server::EverythingServer;

/// Arguments of `test_prompt_with_arguments`.
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct PromptArgs {
    /// First test argument
    pub arg1: String,
    /// Second test argument
    pub arg2: String,
}

/// Arguments of `test_prompt_with_embedded_resource`.
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedResourceArgs {
    /// URI of the resource to embed
    pub resource_uri: String,
}

// The #[prompt] macro generates undocumented `pub fn *_prompt_attr()`
// companions (rmcp-macros prompt.rs hardcodes `pub`), which trips the
// workspace's missing_docs=deny; scoped allow rather than weakening the lint.
#[allow(missing_docs)]
#[prompt_router(router = "prompt_router_all", vis = "pub(crate)")]
impl EverythingServer {
    /// A simple prompt for testing
    #[prompt(name = "test_simple_prompt")]
    async fn test_simple_prompt(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        Ok(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            "This is a simple prompt for testing.",
        )])
    }

    /// A prompt that formats its two required arguments
    #[prompt(name = "test_prompt_with_arguments")]
    async fn test_prompt_with_arguments(
        &self,
        Parameters(PromptArgs { arg1, arg2 }): Parameters<PromptArgs>,
        _context: RequestContext<RoleServer>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        Ok(vec![PromptMessage::new_text(
            PromptMessageRole::User,
            format!("Prompt with arguments: arg1='{arg1}', arg2='{arg2}'"),
        )])
    }

    /// A prompt embedding the resource named by its argument
    #[prompt(name = "test_prompt_with_embedded_resource")]
    async fn test_prompt_with_embedded_resource(
        &self,
        Parameters(EmbeddedResourceArgs { resource_uri }): Parameters<EmbeddedResourceArgs>,
        _context: RequestContext<RoleServer>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        Ok(vec![
            PromptMessage::new_resource(
                PromptMessageRole::User,
                resource_uri,
                Some("text/plain".into()),
                Some("Embedded resource content for testing.".into()),
                None,
                None,
                None,
            ),
            PromptMessage::new_text(
                PromptMessageRole::User,
                "Please process the embedded resource above.",
            ),
        ])
    }

    /// A prompt carrying image content
    #[prompt(name = "test_prompt_with_image")]
    async fn test_prompt_with_image(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        // `PromptMessage` is non-exhaustive and its `new_image` constructor
        // is gated behind rmcp's base64 feature (re-encoding raw bytes the
        // fixture already has encoded); a serde round-trip is the documented
        // construction path for non-exhaustive protocol types.
        let image_message: PromptMessage = serde_json::from_value(serde_json::json!({
            "role": "user",
            "content": {
                "type": "image",
                "data": TINY_PNG_BASE64,
                "mimeType": "image/png",
            },
        }))
        .map_err(|error| {
            ErrorData::internal_error(
                "image prompt message construction failed",
                Some(serde_json::json!({ "error": error.to_string() })),
            )
        })?;
        Ok(vec![
            image_message,
            PromptMessage::new_text(PromptMessageRole::User, "Please analyze the image above."),
        ])
    }
}
