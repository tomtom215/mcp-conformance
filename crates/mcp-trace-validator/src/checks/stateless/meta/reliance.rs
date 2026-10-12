// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `BASE-034`, `MRTR-012` and `MRTR-013`: a server asks for input only of the
//! kinds the request it is answering declared.
//!
//! Split from [`super`] because "declared" reaches below the top-level
//! capability name. A client declares *which* elicitation modes it handles and
//! whether its sampling takes tools, and an `input_required` result can ask for
//! a mode or a tool-enabled sampling the client left out while still naming a
//! capability it has.

use serde_json::Value;

use super::super::super::FindingSink;
use super::params_meta;
use crate::context::TraceContext;
use mcp_conformance_core::trace::Direction;

#[cfg(test)]
mod tests;

/// What an input request needs the client to have declared: the capability its
/// method belongs to, and the member of that capability its parameters select
/// when the specification names one. `None` for a method no capability governs.
fn needs(method: &str, params: Option<&Value>) -> Option<(&'static str, Option<&'static str>)> {
    let param = |name: &str| params.and_then(|params| params.get(name));
    match method {
        // `client/elicitation`: "Servers MUST NOT send elicitation requests
        // with modes that are not supported by the client", and "Clients MUST
        // treat requests without a `mode` field as form mode". A mode this
        // revision does not define selects nothing to compare against.
        "elicitation/create" => {
            let mode = match param("mode").map(Value::as_str) {
                None | Some(Some("form")) => Some("form"),
                Some(Some("url")) => Some("url"),
                _ => None,
            };
            Some(("elicitation", mode))
        }
        // `client/sampling`: "Servers MUST NOT send tool-enabled sampling
        // requests to Clients that have not declared support for tool use via
        // the `sampling.tools` capability". The schema says which requests
        // those are: of both `tools` and `toolChoice`, "The client MUST return
        // an error if this field is provided but
        // `ClientCapabilities.sampling.tools` is not declared."
        "sampling/createMessage" => {
            let tools = param("tools").is_some() || param("toolChoice").is_some();
            Some(("sampling", tools.then_some("tools")))
        }
        "roots/list" => Some(("roots", None)),
        _ => None,
    }
}

/// Whether a declared `capability` covers its `member`.
///
/// A capability declared as something other than an object is taken to cover
/// every member: its shape is the client's defect, and nothing in it says a
/// member is missing. For elicitation, "an empty capabilities object is
/// equivalent to declaring support for `form` mode only" (`client/elicitation`,
/// "For backwards compatibility"); an object naming neither mode is read the
/// same way, since it, too, declares no mode explicitly.
fn covers(capability: &Value, member: &str) -> bool {
    let Some(declared) = capability.as_object() else {
        return true;
    };
    declared.contains_key(member) || (member == "form" && !declared.contains_key("url"))
}

/// `BASE-034`: a server must not rely on capabilities the client did not declare.
///
/// Reliance is internal, so this reports its one wire-visible form: the server
/// asking the client for input (`resultType: "input_required"`, SEP-2322) of a
/// kind the request's own `clientCapabilities` never advertised.
///
/// Each result is judged against the request the context pairs it with, not
/// the last request bearing its id. `2026-07-28` permits reusing an id once its
/// response has arrived (BASE-045), and keyed by id text an `input_required`
/// answering a request that declared `elicitation` was held against a later,
/// unrelated request reusing the id without it.
pub(in crate::checks) fn no_undeclared_capability_reliance(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for exchange in context.exchanges() {
        if exchange.request.direction != Direction::ClientToServer {
            continue;
        }
        let Some(result) = exchange.result else {
            continue;
        };
        if result.get("resultType").and_then(Value::as_str) != Some("input_required") {
            continue;
        }
        // There is no session-wide declaration to fall back on at this
        // revision: the request's own `_meta` is all the client declared.
        let declared = exchange
            .request
            .message_payload()
            .and_then(params_meta)
            .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
            .and_then(Value::as_object);
        let requests = result
            .get("inputRequests")
            .and_then(Value::as_object)
            .map(|map| map.values().collect::<Vec<_>>())
            .unwrap_or_default();
        for request in requests {
            let Some(method) = request.get("method").and_then(Value::as_str) else {
                continue;
            };
            let Some((needed, member)) = needs(method, request.get("params")) else {
                continue;
            };
            // The subject is an input request of a kind a capability governs;
            // the revision's other input kinds need none.
            sink.examined();
            let Some(capability) = declared.and_then(|declared| declared.get(needed)) else {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "server asked for `{method}`, which needs the `{needed}` capability, \
                         but the request's `clientCapabilities` did not declare it"
                    ),
                );
                continue;
            };
            if let Some(member) = member
                && !covers(capability, member)
            {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "server asked for `{method}` needing `{needed}.{member}`, but the \
                         request's `clientCapabilities` declared `{needed}` without it"
                    ),
                );
            }
        }
    }
}
