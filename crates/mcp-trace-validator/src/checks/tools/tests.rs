// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the tools checks.

#![allow(clippy::unwrap_used)]

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

fn findings_for(check: &str, trace: &str) -> Vec<String> {
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let context = TraceContext::new(&events);
    checks::find(check)
        .unwrap()
        .run(&context)
        .findings
        .into_iter()
        .map(|finding| finding.detail)
        .collect()
}

fn session(server_capabilities: &str, body: &[&str]) -> String {
    let mut lines = vec![
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#.to_owned(),
            format!(
                r#"{{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-11-25","capabilities":{server_capabilities},"serverInfo":{{"name":"s","version":"0"}}}}}}}}"#
            ),
            r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#.to_owned(),
        ];
    for (offset, payload) in body.iter().enumerate() {
        let seq = 3 + offset as u64;
        let direction = if offset % 2 == 0 {
            "client-to-server"
        } else {
            "server-to-client"
        };
        lines.push(format!(
                r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
            ));
    }
    lines.join("\n")
}

#[test]
fn name_length_boundaries_are_inclusive() {
    let ok_128 = "a".repeat(128);
    let bad_129 = "a".repeat(129);
    let trace = session(
        r#"{"tools":{}}"#,
        &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"result":{{"tools":[{{"name":"{ok_128}","inputSchema":{{"type":"object"}}}},{{"name":"{bad_129}","inputSchema":{{"type":"object"}}}},{{"name":"","inputSchema":{{"type":"object"}}}}]}}}}"#
            ),
        ],
    );
    let findings = findings_for("tools.name-length", &trace);
    assert_eq!(findings.len(), 2, "{findings:?}");
    assert!(findings[0].contains("129 characters"), "{findings:?}");
    assert!(findings[1].contains("0 characters"), "{findings:?}");
}

#[test]
fn charset_findings_name_the_offending_characters() {
    let trace = session(
        r#"{"tools":{}}"#,
        &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"weather lookup,v2!","inputSchema":{"type":"object"}},{"name":"admin.tools.list-v2_X","inputSchema":{"type":"object"}}]}}"#,
        ],
    );
    let findings = findings_for("tools.name-charset", &trace);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains(r#"" ,!""#), "{findings:?}");
}

#[test]
fn capability_check_abstains_without_an_initialize_result() {
    // Truncated trace: tools traffic but no initialize result at all — the
    // declaration surface is missing, so the check must abstain, not flag.
    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list"}}
{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}}"#;
    assert!(findings_for("tools.capability-declared", trace).is_empty());
}

#[test]
fn null_and_false_capability_values_are_not_declarations() {
    // `{"tools": null}` and `{"tools": false}` resolve the path but declare
    // nothing — the ADR-0006 truthiness rule, pinned here at the check layer.
    for capabilities in [r#"{"tools":null}"#, r#"{"tools":false}"#] {
        let trace = session(
            capabilities,
            &[
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#,
            ],
        );
        let findings = findings_for("tools.capability-declared", &trace);
        assert_eq!(findings.len(), 1, "{capabilities}: {findings:?}");
    }
}

#[test]
fn capability_check_ignores_error_answers() {
    // A server *rejecting* tools traffic is not evidence it supports tools.
    let trace = session(
        "{}",
        &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32601,"message":"Method not found"}}"#,
        ],
    );
    assert!(findings_for("tools.capability-declared", &trace).is_empty());
}

#[test]
fn output_schema_check_skips_execution_errors_and_unknown_tools() {
    let trace = session(
        r#"{"tools":{}}"#,
        &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"w","inputSchema":{"type":"object"},"outputSchema":{"type":"object"}}]}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"w","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"boom"}],"isError":true}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"other","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"result":{"content":[{"type":"text","text":"ok"}]}}"#,
        ],
    );
    assert!(
        findings_for("tools.output-schema-structured-result", &trace).is_empty(),
        "execution errors and tools without schemas are not findings"
    );
}

#[test]
fn interim_and_task_creation_results_are_not_the_tools_result() {
    // A 2026-07-28 multi-round-trip interim result asks for input; the retry
    // carries the tool's result. A 2025-11-25 task-augmented call answers with a
    // CreateTaskResult; `tasks/result` carries the tool's result.
    let trace = session(
        r#"{"tools":{},"tasks":{"requests":{"tools":{"call":{}}}}}"#,
        &[
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"w","inputSchema":{"type":"object"},"outputSchema":{"type":"object"}}]}}"#,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"w","arguments":{}}}"#,
            r#"{"jsonrpc":"2.0","id":3,"result":{"resultType":"input_required","inputRequests":{"a":{"method":"elicitation/create","params":{}}},"requestState":"s"}}"#,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"w","arguments":{},"task":{"ttl":60000}}}"#,
            r#"{"jsonrpc":"2.0","id":4,"result":{"task":{"taskId":"t1","status":"working","createdAt":"2025-11-25T10:30:00Z","lastUpdatedAt":"2025-11-25T10:30:00Z","ttl":60000}}}"#,
            r#"{"jsonrpc":"2.0","id":5,"method":"tasks/result","params":{"taskId":"t1"}}"#,
            r#"{"jsonrpc":"2.0","id":5,"result":{"content":[{"type":"text","text":"{}"}],"structuredContent":{}}}"#,
        ],
    );
    assert!(findings_for("tools.output-schema-structured-result", &trace).is_empty());
    assert!(findings_for("tools.structured-content-text", &trace).is_empty());

    // The tasks/result answer is judged as the tool's result: without
    // structuredContent it is a finding, at the tasks/result response.
    let missing = trace.replace(r#","structuredContent":{}"#, "");
    let findings = findings_for("tools.output-schema-structured-result", &missing);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn structured_content_shape_follows_the_schema_root_type() {
    let call = |schema: &str, content: &str| {
        session(
            r#"{"tools":{}}"#,
            &[
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":2,"result":{{"tools":[{{"name":"w","inputSchema":{{"type":"object"}},"outputSchema":{schema}}}]}}}}"#
                ),
                r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"w","arguments":{}}}"#,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":3,"result":{{"content":[{{"type":"text","text":"x"}}],"structuredContent":{content}}}}}"#
                ),
            ],
        )
    };
    let check = "tools.output-schema-structured-result";
    // 2026-07-28 admits any JSON value; an array schema with an array result passes.
    assert!(
        findings_for(
            check,
            &call(r#"{"type":"array","items":{"type":"string"}}"#, r#"["a"]"#)
        )
        .is_empty()
    );
    // An object schema with an object result passes; with an array it does not.
    assert!(findings_for(check, &call(r#"{"type":"object"}"#, r#"{"n":1}"#)).is_empty());
    let findings = findings_for(check, &call(r#"{"type":"object"}"#, r#"["a"]"#));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("not an object"), "{findings:?}");
}
