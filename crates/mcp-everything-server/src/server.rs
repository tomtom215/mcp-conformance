// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The server type and its [`ServerHandler`] implementation.
//!
//! One rule governs `get_info`: **advertise only what is implemented.** A
//! conformance reference that claims a capability and then answers
//! `method not found` would fail the very suite it exists to pass, so the
//! capability set below grows commit-by-commit with the modules that
//! implement it — tools, resources (with subscriptions), prompts, logging,
//! and completions today.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    CompleteRequestParams, CompleteResult, CompletionInfo, ErrorData as McpError,
    GetPromptRequestParams, GetPromptResult, Implementation, ListPromptsResult,
    ListResourceTemplatesResult, ListResourcesResult, LoggingLevel, PaginatedRequestParams,
    ProtocolVersion, ReadResourceRequestParams, ReadResourceResult, Reference,
    ResourceUpdatedNotificationParam, ServerCapabilities, ServerInfo, SetLevelRequestParams,
    SubscribeRequestParams, UnsubscribeRequestParams,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, prompt_handler, tool_handler};

use crate::{logging, resources};

/// Reference MCP server exercising the `2025-11-25` protocol surface.
///
/// Construct with [`EverythingServer::new`], then serve over any rmcp
/// transport — stdio for subprocess wiring, streamable HTTP behind the
/// default-secure [`crate::policy::HttpSecurityPolicy`].
#[derive(Debug)]
pub struct EverythingServer {
    pub(crate) tool_router: ToolRouter<Self>,
    pub(crate) prompt_router: PromptRouter<Self>,
    /// URIs the client subscribed to (`resources/subscribe` bookkeeping).
    subscriptions: Arc<Mutex<HashSet<String>>>,
    /// Threshold set via `logging/setLevel`; messages below it are dropped.
    log_level: Arc<Mutex<LoggingLevel>>,
}

impl EverythingServer {
    /// Creates the server with every implemented capability wired.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tool_router: crate::tools::all_tools(),
            prompt_router: Self::prompt_router_all(),
            subscriptions: Arc::new(Mutex::new(HashSet::new())),
            log_level: Arc::new(Mutex::new(logging::default_level())),
        }
    }

    /// Whether a message at `candidate` level passes the current threshold.
    pub(crate) fn log_permits(&self, candidate: LoggingLevel) -> bool {
        let threshold = self.log_level.lock().map_or_else(
            // A poisoned lock means a panicked writer; failing open keeps
            // diagnostics flowing exactly when something is going wrong.
            |poisoned| *poisoned.into_inner(),
            |level| *level,
        );
        logging::permits(threshold, candidate)
    }

    /// Snapshot of the subscribed URIs (test and tap support).
    #[must_use]
    pub fn subscribed_uris(&self) -> Vec<String> {
        let mut uris: Vec<String> = self.subscriptions.lock().map_or_else(
            |poisoned| poisoned.into_inner().iter().cloned().collect(),
            |subscriptions| subscriptions.iter().cloned().collect(),
        );
        uris.sort();
        uris
    }

    /// Records a subscription; returns whether the URI was newly tracked.
    pub(crate) fn track_subscription(&self, uri: String) -> bool {
        self.subscriptions
            .lock()
            .is_ok_and(|mut subscriptions| subscriptions.insert(uri))
    }

    /// Drops a subscription; returns whether the URI had been tracked.
    pub(crate) fn untrack_subscription(&self, uri: &str) -> bool {
        self.subscriptions
            .lock()
            .is_ok_and(|mut subscriptions| subscriptions.remove(uri))
    }
}

impl Default for EverythingServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for EverythingServer {
    fn get_info(&self) -> ServerInfo {
        // Not `Implementation::from_build_env()`: that helper expands
        // `env!(..)` inside rmcp and therefore always says "rmcp 1.x".
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .enable_resources_subscribe()
                .enable_resources_list_changed()
                .enable_prompts()
                .enable_prompts_list_changed()
                .enable_logging()
                .enable_completions()
                .build(),
        )
        .with_server_info(Implementation::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
        ))
        .with_protocol_version(ProtocolVersion::V_2025_11_25)
        .with_instructions(
            "Reference server for MCP conformance testing: every advertised \
             capability is implemented and exercised by the official suite. \
             Tools include echo, add, and the suite's test_* contract; \
             resources test://static-text, test://static-binary, and the \
             test://template/{id}/data template; four test_* prompts; \
             logging/setLevel; completion/complete."
                .to_owned(),
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult {
            resources: resources::catalog(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult {
            resource_templates: resources::templates(),
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        resources::read(&request.uri).ok_or_else(|| {
            McpError::resource_not_found(
                "resource not found",
                Some(serde_json::json!({ "uri": request.uri })),
            )
        })
    }

    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        // An immediate update notification acknowledges the subscription in
        // an observable way (the idiom rmcp's own notification example uses);
        // the spec leaves update timing to the server.
        if self.track_subscription(request.uri.clone()) {
            let _ = context
                .peer
                .notify_resource_updated(ResourceUpdatedNotificationParam { uri: request.uri })
                .await;
        }
        Ok(())
    }

    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.untrack_subscription(&request.uri);
        Ok(())
    }

    async fn set_level(
        &self,
        request: SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        if let Ok(mut level) = self.log_level.lock() {
            *level = request.level;
        }
        Ok(())
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, McpError> {
        // The completion-complete scenario's documented example: prefix
        // completion over arg1 of test_prompt_with_arguments. Anything else
        // completes to nothing — minimal support is explicitly conformant.
        const CANDIDATES: [&str; 3] = ["paris", "park", "party"];
        let values = match &request.r#ref {
            Reference::Prompt(prompt)
                if prompt.name == "test_prompt_with_arguments"
                    && request.argument.name == "arg1" =>
            {
                CANDIDATES
                    .iter()
                    .filter(|candidate| candidate.starts_with(&request.argument.value))
                    .map(ToString::to_string)
                    .collect()
            }
            Reference::Prompt(_) | Reference::Resource(_) => Vec::new(),
        };
        let completion = CompletionInfo::with_all_values(values).map_err(|message| {
            McpError::internal_error(
                "completion overflow",
                Some(serde_json::json!({ "message": message })),
            )
        })?;
        Ok(CompleteResult::new(completion))
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
        assert!(
            capabilities.resources.is_some(),
            "resources are implemented"
        );
        assert_eq!(
            capabilities.resources.as_ref().unwrap().subscribe,
            Some(true),
            "subscriptions are implemented"
        );
        assert!(capabilities.prompts.is_some(), "prompts are implemented");
        for (declared, name) in [
            (capabilities.tools.as_ref().unwrap().list_changed, "tools"),
            (
                capabilities.resources.as_ref().unwrap().list_changed,
                "resources",
            ),
            (
                capabilities.prompts.as_ref().unwrap().list_changed,
                "prompts",
            ),
        ] {
            assert_eq!(
                declared,
                Some(true),
                "{name} listChanged is implemented via the test-list-changed tool"
            );
        }
        assert!(capabilities.logging.is_some(), "logging is implemented");
        assert!(
            capabilities.completions.is_some(),
            "completions are implemented"
        );
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

    #[test]
    fn subscription_tracking_inserts_removes_and_reports_sorted() {
        let server = EverythingServer::new();
        assert!(server.track_subscription("test://b".into()), "new URI");
        assert!(server.track_subscription("test://a".into()), "second URI");
        assert!(
            !server.track_subscription("test://a".into()),
            "duplicate is not newly tracked"
        );
        assert_eq!(server.subscribed_uris(), ["test://a", "test://b"]);
        assert!(server.untrack_subscription("test://a"), "tracked URI drops");
        assert!(
            !server.untrack_subscription("test://a"),
            "second drop is a no-op"
        );
        assert_eq!(server.subscribed_uris(), ["test://b"]);
    }

    #[test]
    fn log_threshold_starts_permissive_and_tightens() {
        let server = EverythingServer::new();
        assert!(server.log_permits(LoggingLevel::Debug));
        *server.log_level.lock().unwrap() = LoggingLevel::Error;
        assert!(!server.log_permits(LoggingLevel::Info));
        assert!(server.log_permits(LoggingLevel::Critical));
    }
}
