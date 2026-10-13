// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `BASE-017` (`2025-11-25`) and `BASE-073` (`2026-07-28`): "Schemas MUST be
//! valid according to their declared or default dialect".

use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use crate::checks::FindingSink;
use crate::context::TraceContext;

/// Every schema a message carries, with what it is: a tool's `inputSchema`
/// and `outputSchema` in a `tools/list` result, and an elicitation's
/// `requestedSchema` — a `2025-11-25` `elicitation/create` request, or one of
/// a `2026-07-28` `input_required` result's input requests.
fn schemas(payload: &Value) -> Vec<(String, &Value)> {
    let mut out = Vec::new();
    if let Some(result) = payload.get("result") {
        for tool in result
            .get("tools")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let name = tool
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("(unnamed)");
            for member in ["inputSchema", "outputSchema"] {
                if let Some(schema) = tool.get(member) {
                    out.push((format!("tool {name:?}'s {member}"), schema));
                }
            }
        }
        let requests = result.get("inputRequests").and_then(Value::as_object);
        for (key, request) in requests.into_iter().flatten() {
            if request.get("method").and_then(Value::as_str) == Some("elicitation/create")
                && let Some(schema) = request.pointer("/params/requestedSchema")
            {
                out.push((format!("input request {key:?}'s requestedSchema"), schema));
            }
        }
    }
    if payload.get("method").and_then(Value::as_str) == Some("elicitation/create")
        && let Some(schema) = payload.pointer("/params/requestedSchema")
    {
        out.push(("the elicitation's requestedSchema".to_owned(), schema));
    }
    out
}

/// Each schema a session carries against the metaschema of its declared
/// dialect (2020-12 without a `$schema`). A dialect the engine does not know
/// is not examined: it may be valid in a dialect no check here can read.
pub(in crate::checks) fn schemas_valid_for_dialect(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        for (what, schema) in schemas(payload) {
            let Some(errors) = crate::wire_schema::schema_errors(schema) else {
                continue;
            };
            sink.examined();
            if errors.is_empty() {
                continue;
            }
            let sender = match event.direction {
                Direction::ClientToServer => "client",
                Direction::ServerToClient => "server",
            };
            let dialect = schema.get("$schema").and_then(Value::as_str).map_or_else(
                || "the default dialect, 2020-12".to_owned(),
                |uri| format!("its declared dialect, {uri}"),
            );
            sink.push(
                Some(event.seq),
                format!(
                    "the {sender} sent {what}, which is not a valid schema in {dialect}: {}",
                    crate::wire_schema::summarize(&errors)
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
