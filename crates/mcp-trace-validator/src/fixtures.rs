// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Test fixtures shared across the renderers' tests.

#![allow(clippy::expect_used)]
// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a private module; this follows the rustc
// lint and quiets the clippy one.
#![allow(clippy::redundant_pub_crate)]

use mcp_conformance_core::trace::TraceEvent;

use crate::judge;
use crate::report::Report;

/// A `2026-07-28` session that conforms to the core protocol and breaks the
/// tasks extension: a client whose `tools/call` does not declare the extension
/// is answered with a task (TASK-001). The corpus trace
/// `task-001-task-for-undeclaring-client.jsonl`, verbatim.
pub(crate) const UNDECLARED_TASK: &str = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"elicitation":{},"extensions":{"io.modelcontextprotocol/tasks":{}}},"io.modelcontextprotocol/clientInfo":{"name":"corpus-client","version":"0.1.0"}}}}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"_meta":{"io.modelcontextprotocol/serverInfo":{"name":"corpus-server","version":"0.1.0"}},"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{"tools":{},"extensions":{"io.modelcontextprotocol/tasks":{}}},"ttlMs":60000,"cacheScope":"public"}}}
{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"deploy","arguments":{},"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{"elicitation":{}},"io.modelcontextprotocol/clientInfo":{"name":"corpus-client","version":"0.1.0"}}}}}
{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"_meta":{"io.modelcontextprotocol/serverInfo":{"name":"corpus-server","version":"0.1.0"}},"resultType":"task","taskId":"t-1","status":"working","createdAt":"2026-10-13T10:00:00Z","lastUpdatedAt":"2026-10-13T10:00:01Z","ttlMs":60000,"pollIntervalMs":1000}}}
"#;

/// [`UNDECLARED_TASK`], judged.
pub(crate) fn undeclared_task() -> (Vec<Report>, Vec<TraceEvent>) {
    let judgment = judge::judge(UNDECLARED_TASK).expect("the fixture is judgeable");
    (judgment.reports, judgment.events)
}
