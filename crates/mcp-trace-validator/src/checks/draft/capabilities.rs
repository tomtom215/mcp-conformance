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
//! these read. `tools`, `resources` and `prompts` join them when their pages are
//! entered; a check no requirement references is dead weight, so each arrives
//! with the clause that names it.
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
    if declares(context, capability) != Some(false) {
        return;
    }
    for method in methods {
        for exchange in context.exchanges_for(method) {
            if exchange.result.is_some() {
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
    if declares(context, "logging") != Some(false) {
        return;
    }
    for (event, kind, _) in context.messages() {
        if event.direction != Direction::ServerToClient {
            continue;
        }
        if matches!(kind, MessageKind::Notification { method } if *method == "notifications/message")
        {
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
