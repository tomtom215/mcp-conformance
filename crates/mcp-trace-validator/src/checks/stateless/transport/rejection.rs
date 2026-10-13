// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `TRAN-083`/`TOOL-046` and `TRAN-084`/`TOOL-035`: a Streamable HTTP client
//! rejects a tool definition whose `x-mcp-header` annotations break the
//! constraints, and "rejection means the client MUST exclude the invalid tool
//! from the result of `tools/list`".
//!
//! The rejection itself happens inside the client, and the list it shapes is
//! the one it hands its own consumer, so neither is on the wire — which is why
//! all four clauses were excluded until 2026-10-13. But a rejected, excluded
//! tool has one observable consequence: the client does not call it. A
//! `tools/call` naming a tool whose definition, as last served, the client was
//! bound to reject shows that it kept the tool — offered it onward, and used
//! it. So the clauses are judged by their falsifiable half, and the check is
//! honest about the other: a client that keeps an invalid tool and never calls
//! it is not caught, so a pass means "used no tool it had to reject", nothing
//! stronger.
//!
//! **What "invalid" means here** is exactly what this crate already judges
//! about the server's side of the same definition, so the two cannot disagree:
//! an annotation that breaks `TRAN-080` (`transport.x-mcp-header-name-valid`)
//! or sits where no chain of `properties` keys reaches (`TRAN-082`,
//! `transport.x-mcp-header-statically-reachable`). The integer-range bullet
//! constrains argument values, not definitions, so it rejects no tool.
//!
//! **Only Streamable HTTP.** "Clients using other transports (e.g., stdio) MAY
//! ignore `x-mcp-header` annotations entirely", so a stdio client calling such
//! a tool did nothing wrong. The definition and the call must both have
//! travelled over HTTP.
//!
//! **The subject is the invalid definition**, counted when served: the clause
//! forbids the tool from being used at all after it, so the window it opens is
//! what the trace evidences (ADR-0012's second shape). A later list serving a
//! valid definition under the same name closes the window.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::{Direction, TransportKind};
use serde_json::Value;

use super::designations;
use super::headers::annotation_faults;
use super::placement::annotations;
use crate::checks::FindingSink;
use crate::context::TraceContext;

/// Why `tool`'s definition must be rejected, when it must.
fn rejection(tool: &Value) -> Option<String> {
    let schema = tool.get("inputSchema")?;
    if let Some((pointer, _)) = annotations(schema)
        .into_iter()
        .find(|(_, reachable)| !reachable)
    {
        return Some(format!(
            "its `x-mcp-header` annotation at inputSchema {pointer} is not statically reachable"
        ));
    }
    let mut seen = BTreeSet::new();
    designations(schema).into_iter().find_map(|designation| {
        let duplicate = !seen.insert(designation.name.to_ascii_lowercase());
        annotation_faults(&designation, duplicate)
            .into_iter()
            .next()
            .map(|fault| {
                format!(
                    "its `x-mcp-header` {:?} on `{}` {fault}",
                    designation.name,
                    designation.path.join(".")
                )
            })
    })
}

/// A Streamable HTTP client never calls a tool it was bound to reject.
pub(in crate::checks) fn invalid_tool_not_called(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    // Tool name → (seq of the list that served it, why it must be rejected),
    // as most recently served.
    let mut served: BTreeMap<&str, (u64, Option<String>)> = BTreeMap::new();
    for (event, _, _) in context.messages() {
        if event.transport != TransportKind::StreamableHttp {
            continue;
        }
        let Some(payload) = event.message_payload() else {
            continue;
        };
        match event.direction {
            Direction::ServerToClient => {
                let tools = payload
                    .get("result")
                    .and_then(|result| result.get("tools"))
                    .and_then(Value::as_array);
                for tool in tools.into_iter().flatten() {
                    let Some(name) = tool.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    let reason = rejection(tool);
                    if reason.is_some() {
                        sink.examined();
                    }
                    served.insert(name, (event.seq, reason));
                }
            }
            Direction::ClientToServer => {
                if payload.get("method").and_then(Value::as_str) != Some("tools/call") {
                    continue;
                }
                let Some(name) = payload
                    .get("params")
                    .and_then(|params| params.get("name"))
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                if let Some((listed, Some(reason))) = served.get(name) {
                    sink.push(
                        Some(event.seq),
                        format!(
                            "client calls tool {name:?}, whose definition served at seq {listed} \
                             it had to reject and exclude from `tools/list`: {reason}"
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
