// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The server type and its [`ServerHandler`] implementation.
//!
//! One rule governs `get_info`: **advertise only what is implemented.** A
//! conformance reference that claims a capability and then answers
//! `method not found` would fail the very suite it exists to pass, so the
//! capability set below grows commit-by-commit with the modules that
//! implement it — tools today; resources, prompts, logging, and completions
//! as their modules land (roadmap M2).

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo};
use rmcp::{ServerHandler, tool_handler};

/// Reference MCP server exercising the `2025-11-25` protocol surface.
///
/// Construct with [`EverythingServer::new`], then serve over any rmcp
/// transport — stdio for subprocess wiring, streamable HTTP behind the
/// default-secure [`crate::policy::HttpSecurityPolicy`].
#[derive(Debug)]
pub struct EverythingServer {
    pub(crate) tool_router: ToolRouter<Self>,
}

impl EverythingServer {
    /// Creates the server with every implemented capability wired.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tool_router: crate::tools::all_tools(),
        }
    }
}

impl Default for EverythingServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for EverythingServer {
    fn get_info(&self) -> ServerInfo {
        // Not `Implementation::from_build_env()`: that helper expands
        // `env!(..)` inside rmcp and therefore always says "rmcp 1.x".
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_protocol_version(ProtocolVersion::V_2025_11_25)
            .with_instructions(
                "Reference server for MCP conformance testing: every advertised \
                 capability is implemented and exercised by the official suite. \
                 Tools: echo, add."
                    .to_owned(),
            )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn advertises_exactly_the_implemented_capabilities() {
        let info = EverythingServer::new().get_info();
        let capabilities = info.capabilities;
        assert!(capabilities.tools.is_some(), "tools are implemented");
        // The rest must stay off until their modules land — advertising them
        // now would promise methods the handler cannot answer.
        assert!(capabilities.resources.is_none());
        assert!(capabilities.prompts.is_none());
        assert!(capabilities.logging.is_none());
        assert!(capabilities.completions.is_none());
    }

    #[test]
    fn pins_the_protocol_revision_the_registry_covers() {
        let info = EverythingServer::new().get_info();
        assert_eq!(info.protocol_version, ProtocolVersion::V_2025_11_25);
    }

    #[test]
    fn names_itself_from_the_crate_metadata() {
        let info = EverythingServer::new().get_info();
        assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
    }
}
