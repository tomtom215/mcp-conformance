// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Test fixtures shared across the renderers' tests.

#![allow(clippy::expect_used)]
// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a private module; this follows the rustc
// lint and quiets the clippy one.
#![allow(clippy::redundant_pub_crate)]

use mcp_conformance_core::trace::TraceEvent;
use serde_json::{Value, json};

use crate::judge::{self, Judgment};
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

pub(crate) const DRAFT_07: &str = "http://json-schema.org/draft-07/schema#";
pub(crate) const DRAFT_2020: &str = "https://json-schema.org/draft/2020-12/schema";

/// One JSON Lines event.
pub(crate) fn event(seq: u64, direction: &str, payload: &Value) -> String {
    json!({
        "seq": seq,
        "direction": direction,
        "transport": "stdio",
        "kind": "message",
        "payload": payload,
    })
    .to_string()
}

/// A `2025-11-25` session of a server listing `tools`, each with the `$schema`
/// its input schema declares: the shape of the TypeScript SDK's.
pub(crate) fn session(tools: &[(&str, &str)]) -> String {
    let listed: Vec<Value> = tools
        .iter()
        .map(|(name, dialect)| {
            json!({ "name": name, "inputSchema": { "$schema": dialect, "type": "object" } })
        })
        .collect();
    session_answering(&json!({ "tools": listed }))
}

/// The same session, the server answering `tools/list` with `result`.
pub(crate) fn session_answering(result: &Value) -> String {
    [
        event(
            0,
            "client-to-server",
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "c", "version": "1"}}}),
        ),
        event(
            1,
            "server-to-client",
            &json!({"jsonrpc": "2.0", "id": 1, "result": {
                "protocolVersion": "2025-11-25", "capabilities": {"tools": {}},
                "serverInfo": {"name": "s", "version": "1"}}}),
        ),
        event(
            2,
            "client-to-server",
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        ),
        event(
            3,
            "client-to-server",
            &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        ),
        event(
            4,
            "server-to-client",
            &json!({"jsonrpc": "2.0", "id": 2, "result": result}),
        ),
    ]
    .join("\n")
}

pub(crate) fn judged(tools: &[(&str, &str)]) -> Judgment {
    judge::judge(&session(tools)).expect("the fixture session judges")
}

/// The three-tool session the tests start from: two draft-07 tools, one not.
pub(crate) fn sdk_session() -> Judgment {
    judged(&[("a", DRAFT_07), ("b", DRAFT_07), ("c", DRAFT_2020)])
}

/// [`sdk_session`], with a baseline accepting both its findings for `reason`
/// applied: the report a server author with known findings reads.
pub(crate) fn baselined_sdk_session(reason: &str) -> Judgment {
    use crate::baseline::{Baseline, Entry, TraceReports, record};
    let mut judgment = sdk_session();
    let recorded = record(&[(Some("t.jsonl"), &judgment.reports)], None, None);
    let baseline = Baseline::new(
        recorded
            .baseline
            .entries()
            .iter()
            .map(|entry| Entry::new(entry.fingerprint.clone(), reason))
            .collect(),
    );
    let mut traces = [TraceReports {
        name: Some("t.jsonl"),
        reports: &mut judgment.reports,
    }];
    baseline
        .apply(&mut traces, None)
        .expect("no entry has a date");
    judgment
}
