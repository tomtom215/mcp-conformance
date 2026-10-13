// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `TRAN-137`, `TRAN-082` and `TOOL-045`: an `x-mcp-header` annotation is
//! valid only on a property *statically reachable* from the schema root —
//! "reachable via a chain consisting solely of `properties` keys", never
//! through an array, composition or conditional keyword, or `$ref`.

use serde_json::Value;

use super::tool_definitions;
use crate::checks::FindingSink;
use crate::context::TraceContext;

/// Keywords whose value is one subschema, in every JSON Schema draft a tool
/// may declare (2020-12 and the drafts before it).
const SCHEMA_KEYWORDS: &[&str] = &[
    "additionalItems",
    "additionalProperties",
    "contains",
    "contentSchema",
    "else",
    "if",
    "items",
    "not",
    "propertyNames",
    "then",
    "unevaluatedItems",
    "unevaluatedProperties",
];

/// Keywords whose value is an array of subschemas (`items` before 2020-12).
const SCHEMA_ARRAY_KEYWORDS: &[&str] = &["allOf", "anyOf", "items", "oneOf", "prefixItems"];

/// Keywords whose value maps names to subschemas. Only `properties` keeps a
/// chain static; reaching a `$defs` entry takes a `$ref`, which breaks it.
const SCHEMA_MAP_KEYWORDS: &[&str] = &[
    "$defs",
    "definitions",
    "dependencies",
    "dependentSchemas",
    "patternProperties",
    "properties",
];

/// The JSON Pointer of every `x-mcp-header` annotation in `schema`, and
/// whether its property is statically reachable.
///
/// Walks subschemas only, by keyword, so `default`, `const`, `enum`,
/// `examples` and unknown keywords — which hold instance data that may carry
/// the same key without being an annotation — are never mistaken for schemas.
/// Recursion is bounded by the reader's nesting limit.
pub(super) fn annotations(schema: &Value) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    walk(schema, "", true, &mut out);
    out
}

fn walk(schema: &Value, pointer: &str, reachable: bool, out: &mut Vec<(String, bool)>) {
    let Some(object) = schema.as_object() else {
        return;
    };
    if object.contains_key("x-mcp-header") {
        let at = if pointer.is_empty() {
            "/".to_owned()
        } else {
            pointer.to_owned()
        };
        // The root is a schema, not a property: an annotation there sits on
        // no parameter at all.
        out.push((at, reachable && !pointer.is_empty()));
    }
    for (keyword, value) in object {
        let here = format!("{pointer}/{}", escape(keyword));
        if SCHEMA_MAP_KEYWORDS.contains(&keyword.as_str()) {
            for (name, subschema) in value.as_object().into_iter().flatten() {
                let static_step = reachable && keyword == "properties";
                walk(
                    subschema,
                    &format!("{here}/{}", escape(name)),
                    static_step,
                    out,
                );
            }
        } else if let Some(items) = value
            .as_array()
            .filter(|_| SCHEMA_ARRAY_KEYWORDS.contains(&keyword.as_str()))
        {
            for (index, subschema) in items.iter().enumerate() {
                walk(subschema, &format!("{here}/{index}"), false, out);
            }
        } else if SCHEMA_KEYWORDS.contains(&keyword.as_str()) {
            walk(value, &here, false, out);
        }
    }
}

/// RFC 6901 escaping of one reference token.
fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

/// Every `x-mcp-header` annotation in a tool's `inputSchema` sits on a
/// statically reachable property.
pub(in crate::checks) fn x_mcp_header_statically_reachable(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (seq, tool) in tool_definitions(context) {
        let Some(schema) = tool.get("inputSchema") else {
            continue;
        };
        let name = tool
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("(unnamed)");
        for (pointer, reachable) in annotations(schema) {
            sink.examined();
            if !reachable {
                sink.push(
                    Some(seq),
                    format!(
                        "tool {name:?} carries an `x-mcp-header` annotation at inputSchema \
                         {pointer}, which no chain of `properties` keys alone reaches from the \
                         schema root"
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
