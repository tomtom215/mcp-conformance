// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tools that emit notifications mid-execution.
//!
//! `tools-call-with-logging` and `tools-call-with-progress` exist to prove a
//! client can receive `notifications/message` and `notifications/progress`
//! *during* a tool call, so the inter-notification delays the scenarios ask
//! for (~50 ms) are part of the contract, not an implementation detail.

use std::time::Duration;

use rmcp::model::{
    CallToolResult, Content, LoggingLevel, LoggingMessageNotificationParam,
    ProgressNotificationParam,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, tool, tool_router};

use crate::server::EverythingServer;

/// Delay between the staged notifications; the scenarios specify "~50ms" so
/// clients demonstrably observe distinct messages.
const STAGE_DELAY: Duration = Duration::from_millis(50);

#[tool_router(router = tool_router_notifying, vis = "pub(crate)")]
impl EverythingServer {
    /// `tools-call-with-logging`: three info-level log notifications spaced
    /// ~50 ms apart, then a confirming text result.
    ///
    /// # Errors
    ///
    /// Never fails: log delivery is best-effort by design — a client that
    /// ignores notifications must still get the tool result.
    #[tool(description = "Sends log notifications during execution for conformance testing")]
    pub async fn test_tool_with_logging(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // logging/setLevel filtering happens at the emission site: a
        // threshold above info silences these (the scenario's "filter
        // subsequent log notifications" requirement).
        let permitted = self.log_permits(LoggingLevel::Info);
        let log = |message: &'static str| {
            let peer = context.peer.clone();
            async move {
                if permitted {
                    let _ = peer
                        .notify_logging_message(LoggingMessageNotificationParam {
                            level: LoggingLevel::Info,
                            logger: Some("test_tool_with_logging".into()),
                            data: serde_json::Value::String(message.into()),
                        })
                        .await;
                }
            }
        };
        log("Tool execution started").await;
        tokio::time::sleep(STAGE_DELAY).await;
        log("Tool processing data").await;
        tokio::time::sleep(STAGE_DELAY).await;
        log("Tool execution completed").await;
        Ok(CallToolResult::success(vec![Content::text(
            "Tool with logging executed successfully.",
        )]))
    }

    /// `tools-call-with-progress`: progress 0 → 50 → 100 (total 100) against
    /// the request's `progressToken`; without a token, just the delays.
    ///
    /// # Errors
    ///
    /// Never fails: progress delivery is best-effort by design.
    #[tool(description = "Reports progress notifications during execution for conformance testing")]
    pub async fn test_tool_with_progress(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let token = context.meta.get_progress_token();
        let notify = |progress: u32| {
            let token = token.clone();
            let peer = context.peer.clone();
            async move {
                if let Some(token) = token {
                    let _ = peer
                        .notify_progress(ProgressNotificationParam {
                            progress_token: token,
                            progress: progress.into(),
                            total: Some(100_u32.into()),
                            message: None,
                        })
                        .await;
                }
            }
        };
        // Unrolled rather than looped: the only conditional this function
        // needs is token presence — a "sleep between stages" branch would be
        // observable solely through wall-clock timing, which deterministic
        // tests must not assert.
        notify(0).await;
        tokio::time::sleep(STAGE_DELAY).await;
        notify(50).await;
        tokio::time::sleep(STAGE_DELAY).await;
        notify(100).await;
        Ok(CallToolResult::success(vec![Content::text(
            "Tool with progress executed successfully.",
        )]))
    }
}
