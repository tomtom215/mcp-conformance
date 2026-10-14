// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The schemas a session carries: `BASE-017` (`2025-11-25`) and `BASE-073`
//! (`2026-07-28`), "Schemas MUST be valid according to their declared or
//! default dialect", and `BASE-086`/`BASE-088`, the recommendation to use JSON
//! Schema 2020-12.

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

/// The JSON Schema 2020-12 metaschema URI, as `$schema` names the dialect.
const DIALECT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

/// `BASE-086` (`2025-11-25`) and `BASE-088` (`2026-07-28`): "Implementors are
/// RECOMMENDED to use JSON Schema 2020-12."
///
/// The schema's own `$schema` says which dialect its author used. A schema
/// without one *is* 2020-12 — the default dialect — so only an explicit
/// declaration of another dialect deviates: `draft-07` is the specification's
/// own example of one, and permitted (implementations may support more), which
/// is why this is the recommendation's check and not the validity check's.
/// Every schema the session carries is a subject, examined or not by
/// [`schemas_valid_for_dialect`]: a dialect the engine cannot read is still a
/// dialect that is not 2020-12.
pub(in crate::checks) fn schemas_use_2020_12(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, _, _) in context.messages() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        for (what, schema) in schemas(payload) {
            sink.examined();
            let Some(declared) = schema.get("$schema") else {
                continue;
            };
            if is_2020_12(declared) {
                continue;
            }
            let sender = match event.direction {
                Direction::ClientToServer => "client",
                Direction::ServerToClient => "server",
            };
            sink.push(
                Some(event.seq),
                format!(
                    "the {sender} sent {what} declaring `$schema` {declared}; JSON Schema \
                     2020-12 ({DIALECT_2020_12}) is the recommended dialect"
                ),
            );
        }
    }
}

/// Whether a `$schema` value names 2020-12: its URI, with or without the empty
/// fragment some authors append.
fn is_2020_12(declared: &Value) -> bool {
    declared
        .as_str()
        .is_some_and(|uri| uri.strip_suffix('#').unwrap_or(uri) == DIALECT_2020_12)
}

#[cfg(test)]
mod tests;
