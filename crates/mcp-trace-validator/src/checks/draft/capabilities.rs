// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Server capability declarations at `2026-07-28`.
//!
//! Every feature page states the same clause — "Servers that support X MUST
//! declare the X capability" — and the `2025-11-25` registry judges it with
//! `tools.capability-declared` and its four siblings. **Those checks cannot be
//! reused here.** They resolve declarations through
//! `support::server_capability`, which returns "abstain" unless the trace
//! carries an `initialize` *result*, and this revision has no `initialize` at
//! all. Pointed at a `2026-07-28` entry each would inspect nothing and report a
//! vacuous `pass` — the TRAN-071 failure mode, five times over.
//!
//! The declaration surface here is the `server/discover` result, so that is what
//! these read.
//!
//! The abstention is preserved where it is honest: a session that
//! never probed carries no declaration, and silence is not a denial.

use mcp_conformance_core::message::MessageKind;
use mcp_conformance_core::trace::Direction;

use super::super::FindingSink;
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// The probe whose result is this revision's capability declaration.
const DISCOVER: &str = "server/discover";

/// Whether the server declared `capability`, or `None` when the trace carries no
/// `server/discover` result to read a declaration from.
///
/// A declaration is present when the key resolves to something that is neither
/// `false` nor `null` — the same reading ADR-0006 gives the `2025-11-25`
/// capability objects.
fn declares(context: &TraceContext<'_>, capability: &str) -> Option<bool> {
    let capabilities = context
        .exchanges_for(DISCOVER)
        .find_map(|exchange| exchange.result)?
        .get("capabilities");
    let Some(capabilities) = capabilities else {
        return Some(false);
    };
    Some(
        capabilities
            .get(capability)
            .is_some_and(|value| !value.is_null() && value.as_bool() != Some(false)),
    )
}

/// Reports every `methods` request the server *answered* while `capability` was
/// declared absent — answering is the observable form of supporting a feature.
fn answered_undeclared(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    capability: &str,
    methods: &[&str],
) {
    // No `server/discover` result at all: there is no declaration surface, so
    // the session's capability state is unknowable rather than empty.
    let Some(declared) = declares(context, capability) else {
        return;
    };
    for method in methods {
        for exchange in context.exchanges_for(method) {
            if exchange.result.is_none() {
                continue;
            }
            sink.examined();
            if !declared {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "server answered `{method}` while its `{DISCOVER}` result declared no \
                         `{capability}` capability"
                    ),
                );
            }
        }
    }
}

/// `COMP-007`: a server answering completions declares the `completions` capability.
pub(in crate::checks) fn completions_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    answered_undeclared(context, sink, "completions", &["completion/complete"]);
}

/// `LOG-007`: a server emitting log notifications declares the `logging` capability.
///
/// Logging has no request of its own at this revision — `logging/setLevel` was
/// removed and the level rides each request's `_meta` — so the observable form of
/// "emits log message notifications" is the notification itself.
pub(in crate::checks) fn logging_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some(declared) = declares(context, "logging") else {
        return;
    };
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        if matches!(kind, MessageKind::Notification { method } if *method == "notifications/message")
        {
            sink.examined();
            if !declared {
                sink.push(
                    Some(event.seq),
                    format!(
                        "server emitted `notifications/message` while its `{DISCOVER}` result \
                         declared no `logging` capability"
                    ),
                );
            }
        }
    }
}

/// The list operation each capability's declaration obliges the server to answer.
const LIST_METHOD: &[(&str, &str)] = &[
    ("tools", "tools/list"),
    ("resources", "resources/list"),
    ("prompts", "prompts/list"),
];

/// Reports a declared capability whose list operation the server refused as an
/// unimplemented method — the shared body of TOOL-020, RES-013 and PROM-013.
fn declared_list_answered(context: &TraceContext<'_>, sink: &mut FindingSink, capability: &str) {
    if declares(context, capability) != Some(true) {
        return;
    }
    let Some((_, method)) = LIST_METHOD.iter().find(|(name, _)| *name == capability) else {
        return;
    };
    for exchange in context.exchanges_for(method) {
        // The subject is a call of the declared capability's list operation:
        // a capability nobody exercised is not one this session saw served.
        sink.examined();
        let code = exchange
            .response
            .message_payload()
            .and_then(|payload| payload.get("error"))
            .and_then(|error| error.get("code"))
            .and_then(serde_json::Value::as_i64);
        if code == Some(-32601) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "server declared the `{capability}` capability but answered `{method}` \
                     with -32601; a declared capability must be served"
                ),
            );
        }
    }
}

/// `TOOL-016`: a server serving tools declares the `tools` capability.
pub(in crate::checks) fn tools_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    answered_undeclared(context, sink, "tools", &["tools/list", "tools/call"]);
}

/// `RES-012`: a server serving resources declares the `resources` capability.
pub(in crate::checks) fn resources_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    answered_undeclared(
        context,
        sink,
        "resources",
        &[
            "resources/list",
            "resources/templates/list",
            "resources/read",
        ],
    );
}

/// `PROM-012`: a server serving prompts declares the `prompts` capability.
pub(in crate::checks) fn prompts_declared(context: &TraceContext<'_>, sink: &mut FindingSink) {
    answered_undeclared(context, sink, "prompts", &["prompts/list", "prompts/get"]);
}

/// `TOOL-020`: a declared `tools` capability answers `tools/list`.
pub(in crate::checks) fn tools_list_implemented(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_list_answered(context, sink, "tools");
}

/// `RES-013`: a declared `resources` capability answers `resources/list`.
pub(in crate::checks) fn resources_list_implemented(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_list_answered(context, sink, "resources");
}

/// `PROM-013`: a declared `prompts` capability answers `prompts/list`.
pub(in crate::checks) fn prompts_list_implemented(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_list_answered(context, sink, "prompts");
}

/// `TOOL-038`: a server embedding resources in tool results declares `resources`.
///
/// The observable form of "uses embedded resources" is a `resource` content block
/// in a `tools/call` result — the same evidence `tools.embedded-resource-capability`
/// reads at `2025-11-25`, which cannot be reused because it resolves the
/// declaration through the removed handshake.
pub(in crate::checks) fn embedded_resource_declared(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    let Some(declared) = declares(context, "resources") else {
        return;
    };
    for exchange in context.exchanges_for("tools/call") {
        let embedded = exchange
            .result
            .and_then(|result| result.get("content"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    block.get("type").and_then(serde_json::Value::as_str) == Some("resource")
                })
            });
        if !embedded {
            continue;
        }
        sink.examined();
        if !declared {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "a `tools/call` result embeds a resource while the `{DISCOVER}` result \
                     declared no `resources` capability"
                ),
            );
        }
    }
}
