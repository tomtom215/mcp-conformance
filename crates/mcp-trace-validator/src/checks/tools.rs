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
use super::support::{Declaration, server_capability};
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

/// Final `tools/call` results, with the called tool's name when stated.
///
/// Two kinds of successful response are not the tool's result and are skipped:
/// a `2026-07-28` multi-round-trip interim result (`resultType:
/// "input_required"`, which asks the client for input and is retried), and a
/// `2025-11-25` task-augmented call's `CreateTaskResult` (the request carried
/// `task`; the tool's result is fetched later with `tasks/result`). For the
/// latter, the `tasks/result` answer is yielded in its place, attributed to the
/// tool through the task id the `CreateTaskResult` issued.
fn call_results<'a>(
    context: &TraceContext<'a>,
) -> impl Iterator<Item = (u64, Option<&'a str>, &'a Value)> {
    let mut task_tools: std::collections::BTreeMap<&'a str, Option<&'a str>> =
        std::collections::BTreeMap::new();
    let mut results = Vec::new();
    for exchange in context.exchanges() {
        let Some(result) = exchange.result else {
            continue;
        };
        let param = |key: &str| exchange.params.and_then(|params| params.get(key));
        match exchange.method {
            "tools/call" => {
                let name = param("name").and_then(Value::as_str);
                if param("task").is_some() {
                    if let Some(task_id) = result
                        .get("task")
                        .and_then(|task| task.get("taskId"))
                        .and_then(Value::as_str)
                    {
                        task_tools.insert(task_id, name);
                    }
                    continue;
                }
                if result.get("resultType").and_then(Value::as_str) == Some("input_required") {
                    continue;
                }
                results.push((exchange.response.seq, name, result));
            }
            "tasks/result" => {
                let Some(name) = param("taskId")
                    .and_then(Value::as_str)
                    .and_then(|task_id| task_tools.get(task_id))
                else {
                    continue; // Not a task this trace saw a tool create.
                };
                results.push((exchange.response.seq, *name, result));
            }
            _ => {}
        }
    }
    results.into_iter()
}

/// `TOOL-001`: "Servers that support tools MUST declare the `tools` capability:" —
/// successfully serving tools traffic, or emitting the tools list-changed
/// notification, is the observable form of supporting tools.
pub(super) fn capability_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // The subject is an answered `tools/*` exchange — the observable form of
    // supporting tools — whether or not the declaration is there. A session
    // that never exercised tools has nothing to judge and says so.
    let declared = match server_capability(context, &["tools"]) {
        Declaration::Declared => true,
        Declaration::Withheld => false,
        // Nothing in this trace could have declared anything, so it shows
        // neither compliance nor violation: abstain before counting a subject.
        Declaration::Unknowable => return,
    };
    for exchange in context.exchanges() {
        if exchange.method.starts_with("tools/") && exchange.result.is_some() {
            sink.examined();
            if !declared {
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
}

/// `TOOL-003`: a listed tool's `inputSchema` must be a JSON Schema *object* — never
/// `null`, an array, or any other scalar. Presence is not judged here (the spec's
/// shape lists the member; this clause constrains its type).
pub(super) fn input_schema_object(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, tool) in listed_tools(context) {
        let Some(schema) = tool.get("inputSchema") else {
            continue;
        };
        sink.examined();
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

/// `TOOL-005`: tool names should be 1–128 characters long, inclusive.
pub(super) fn name_length(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, tool) in listed_tools(context) {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue; // A tool object without a string name is BASE's finding.
        };
        sink.examined();
        let length = name.chars().count();
        if !(1..=128).contains(&length) {
            sink.push(
                Some(seq),
                format!("tool name {name:?} is {length} characters long, expected 1 to 128"),
            );
        }
    }
}

/// `TOOL-006` / `TOOL-007`: tool names should use only ASCII letters, digits,
/// underscore, hyphen, and dot — which also rules out spaces, commas, and other
/// special characters.
pub(super) fn name_charset(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (seq, tool) in listed_tools(context) {
        let Some(name) = tool.get("name").and_then(Value::as_str) else {
            continue;
        };
        sink.examined();
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
            let Some(name) = tool.get("name").and_then(Value::as_str) else {
                continue;
            };
            sink.examined();
            if !seen.insert(name) {
                sink.push(
                    Some(exchange.response.seq),
                    format!("tool name {name:?} appears more than once in this tools/list result"),
                );
            }
        }
    }
}

/// `TOOL-009`: servers returning embedded resources in tool results should declare
/// the `resources` capability.
pub(super) fn embedded_resource_capability(context: &TraceContext<'_>, sink: &mut FindingSink) {
    // The subject is a result that actually embedded a resource; a session
    // whose tools never returned one leaves this clause untested.
    let declared = match server_capability(context, &["resources"]) {
        Declaration::Declared => true,
        Declaration::Withheld => false,
        // Nothing in this trace could have declared anything, so it shows
        // neither compliance nor violation: abstain before counting a subject.
        Declaration::Unknowable => return,
    };
    for (seq, name, result) in call_results(context) {
        let embedded = content_items(result)
            .any(|item| item.get("type").and_then(Value::as_str) == Some("resource"));
        if !embedded {
            continue;
        }
        sink.examined();
        if !declared {
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
        sink.examined();
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

/// `TOOL-011`/`TOOL-040`: when a tool declared an `outputSchema` in `tools/list`,
/// its successful, non-`isError` call results must provide `structuredContent`.
/// Conformance of that content *to* the schema needs a JSON Schema engine and is
/// exercised through the official-suite agreement check; what a trace judges is
/// presence, and the one piece of shape the schema states at its root: a schema
/// of `"type": "object"` (the only kind `2025-11-25` allows) needs an object.
/// `2026-07-28` admits any JSON value, an array schema included.
pub(super) fn output_schema_structured_result(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let with_output_schema: std::collections::BTreeMap<&str, bool> = listed_tools(context)
        .filter_map(|(_, tool)| {
            let schema = tool
                .get("outputSchema")
                .filter(|schema| schema.is_object())?;
            let wants_object = schema.get("type").and_then(Value::as_str) == Some("object");
            Some((tool.get("name").and_then(Value::as_str)?, wants_object))
        })
        .collect();
    if with_output_schema.is_empty() {
        return;
    }
    for (seq, name, result) in call_results(context) {
        let Some(name) = name else { continue };
        let Some(&wants_object) = with_output_schema.get(name) else {
            continue;
        };
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            continue; // Execution errors legitimately carry no structured result.
        }
        sink.examined();
        match result.get("structuredContent") {
            None => sink.push(
                Some(seq),
                format!(
                    "tool {name:?} declares an outputSchema but this result carries no structuredContent"
                ),
            ),
            Some(structured) if wants_object && !structured.is_object() => sink.push(
                Some(seq),
                format!(
                    "tool {name:?} declares an object outputSchema but this result's structuredContent is not an object"
                ),
            ),
            Some(_) => {}
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
mod tests;
