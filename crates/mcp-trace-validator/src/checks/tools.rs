// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the `2025-11-25` tools requirements (`TOOL-*`).
//!
//! List-shaped evidence comes from `tools/list` results; call-shaped evidence from
//! `tools/call` exchanges. Checks abstain (no finding) when the trace lacks the
//! evidence a judgment needs — a missing `initialize` result, an error response, a
//! tool object without a `name` — because those gaps are other requirements'
//! findings, not these.

use serde_json::Value;

use super::FindingSink;
use super::support::server_capability;
use crate::context::TraceContext;

/// Every tool object across all `tools/list` results, with the result event's `seq`.
fn listed_tools<'a>(context: &TraceContext<'a>) -> impl Iterator<Item = (u64, &'a Value)> {
    context.exchanges_for("tools/list").flat_map(|exchange| {
        let seq = exchange.response.seq;
        exchange
            .result
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(move |tool| (seq, tool))
    })
}

/// Successful `tools/call` results, with the called tool's name when stated.
fn call_results<'a>(
    context: &TraceContext<'a>,
) -> impl Iterator<Item = (u64, Option<&'a str>, &'a Value)> {
    context.exchanges_for("tools/call").filter_map(|exchange| {
        let result = exchange.result?;
        let name = exchange
            .params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str);
        Some((exchange.response.seq, name, result))
    })
}

/// `TOOL-001`: "Servers that support tools MUST declare the `tools` capability:" —
/// successfully serving tools traffic, or emitting the tools list-changed
/// notification, is the observable form of supporting tools.
pub(super) fn capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    if server_capability(context, &["tools"]) != Some(false) {
        return;
    }
    for exchange in context.exchanges() {
        if exchange.method.starts_with("tools/") && exchange.result.is_some() {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "server answered {:?} without declaring the tools capability",
                    exchange.method
                ),
            );
        }
    }
}

/// `TOOL-003`: a listed tool's `inputSchema` must be a JSON Schema *object* — never
/// `null`, an array, or any other scalar. Presence is not judged here (the spec's
/// shape lists the member; this clause constrains its type).
pub(super) fn input_schema_object(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, tool) in listed_tools(context) {
        if let Some(schema) = tool.get("inputSchema") {
            if !schema.is_object() {
                sink.push(
                    Some(seq),
                    format!(
                        "tool {} has an inputSchema that is not a JSON Schema object: {schema}",
                        tool_label(tool)
                    ),
                );
            }
        }
    }
}

/// `TOOL-005`: tool names should be 1–128 characters long, inclusive.
pub(super) fn name_length(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, tool) in listed_tools(context) {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            let length = name.chars().count();
            if !(1..=128).contains(&length) {
                sink.push(
                    Some(seq),
                    format!("tool name {name:?} is {length} characters long, expected 1 to 128"),
                );
            }
        }
    }
}

/// `TOOL-006` / `TOOL-007`: tool names should use only ASCII letters, digits,
/// underscore, hyphen, and dot — which also rules out spaces, commas, and other
/// special characters.
pub(super) fn name_charset(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, tool) in listed_tools(context) {
        if let Some(name) = tool.get("name").and_then(Value::as_str) {
            let offenders: String = name
                .chars()
                .filter(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
                .collect();
            if !offenders.is_empty() {
                sink.push(
                    Some(seq),
                    format!(
                        "tool name {name:?} contains characters outside A-Z, a-z, 0-9, underscore, hyphen, and dot: {offenders:?}"
                    ),
                );
            }
        }
    }
}

/// `TOOL-008`: tool names should be unique within a server. Judged within each
/// `tools/list` result: re-listing the same page is not a duplication, so cross-result
/// repeats are out of scope (and pagination cursor flows are PAGE-002's business).
pub(super) fn name_unique(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for exchange in context.exchanges_for("tools/list") {
        let Some(tools) = exchange
            .result
            .and_then(|result| result.get("tools"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        let mut seen = std::collections::BTreeSet::new();
        for tool in tools {
            if let Some(name) = tool.get("name").and_then(Value::as_str) {
                if !seen.insert(name) {
                    sink.push(
                        Some(exchange.response.seq),
                        format!(
                            "tool name {name:?} appears more than once in this tools/list result"
                        ),
                    );
                }
            }
        }
    }
}

/// `TOOL-009`: servers returning embedded resources in tool results should declare
/// the `resources` capability.
pub(super) fn embedded_resource_capability(context: &TraceContext<'_>, sink: &mut FindingSink) {
    if server_capability(context, &["resources"]) != Some(false) {
        return;
    }
    for (seq, name, result) in call_results(context) {
        let embedded = content_items(result)
            .any(|item| item.get("type").and_then(Value::as_str) == Some("resource"));
        if embedded {
            sink.push(
                Some(seq),
                format!(
                    "tool {} returned an embedded resource, but the server did not declare the resources capability",
                    name.map_or_else(|| "(unnamed)".to_owned(), |name| format!("{name:?}"))
                ),
            );
        }
    }
}

/// `TOOL-010`: a result carrying `structuredContent` should also carry the serialized
/// JSON in a `TextContent` block, for backwards compatibility.
pub(super) fn structured_content_text(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, name, result) in call_results(context) {
        if result.get("structuredContent").is_none() {
            continue;
        }
        let has_text = content_items(result)
            .any(|item| item.get("type").and_then(Value::as_str) == Some("text"));
        if !has_text {
            sink.push(
                Some(seq),
                format!(
                    "tool {} returned structuredContent without a TextContent fallback block",
                    name.map_or_else(|| "(unnamed)".to_owned(), |name| format!("{name:?}"))
                ),
            );
        }
    }
}

/// `TOOL-011`: when a tool declared an `outputSchema` in `tools/list`, its successful,
/// non-`isError` call results must provide `structuredContent`. Conformance of that
/// content *to* the schema needs a JSON Schema engine and is exercised through the
/// official-suite agreement check (roadmap M2); presence is what a trace judges.
pub(super) fn output_schema_structured_result(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let with_output_schema: std::collections::BTreeSet<&str> = listed_tools(context)
        .filter(|(_, tool)| tool.get("outputSchema").is_some_and(Value::is_object))
        .filter_map(|(_, tool)| tool.get("name").and_then(Value::as_str))
        .collect();
    if with_output_schema.is_empty() {
        return;
    }
    for (seq, name, result) in call_results(context) {
        let Some(name) = name else { continue };
        if !with_output_schema.contains(name) {
            continue;
        }
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            continue; // Execution errors legitimately carry no structured result.
        }
        if !result
            .get("structuredContent")
            .is_some_and(Value::is_object)
        {
            sink.push(
                Some(seq),
                format!(
                    "tool {name:?} declares an outputSchema but this result carries no structuredContent object"
                ),
            );
        }
    }
}

/// The `content` array items of a tool result, if any.
fn content_items(result: &Value) -> impl Iterator<Item = &Value> {
    result
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// A short identifier for a tool object in findings: its name when present.
fn tool_label(tool: &Value) -> String {
    tool.get("name")
        .and_then(Value::as_str)
        .map_or_else(|| "(unnamed)".to_owned(), |name| format!("{name:?}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::checks;
    use crate::context::TraceContext;
    use crate::reader::{Limits, parse_trace};

    fn findings_for(check: &str, trace: &str) -> Vec<String> {
        let events = parse_trace(trace, &Limits::default()).unwrap();
        let context = TraceContext::new(&events);
        checks::find(check)
            .unwrap()
            .run(&context)
            .into_iter()
            .map(|finding| finding.detail)
            .collect()
    }

    fn session(server_capabilities: &str, body: &[&str]) -> String {
        let mut lines = vec![
            format!(
                r#"{{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"t","version":"0"}}}}}}}}"#
            ),
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
}
