// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tool surface of the everything server.
//!
//! Mirrors the TypeScript everything server's tool semantics (register 2.10:
//! parity with its surface is the M2 coverage bar) starting with the
//! foundational pair every conformance scenario can call: `echo` and `add`.
//! Each tool documents the upstream behavior it mirrors so divergence is a
//! reviewable decision, never an accident.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content};
use rmcp::{ErrorData, tool, tool_router};

use crate::server::EverythingServer;

/// Arguments for [`EverythingServer::echo`].
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct EchoArgs {
    /// Message to echo back unchanged.
    pub message: String,
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
}
