// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Checks for the base JSON-RPC message requirements (`BASE-*`).
//!
//! These operate per message and rely on [`classify`]'s leniency: malformed messages
//! are reported precisely rather than aborting the run.
//!
//! Two groups live in submodules because they carry machinery of their own:
//! [`meta`] holds the `_meta` key grammar, and [`correlation`] the shared walk
//! that answers `BASE-004` and `BASE-009` together.

use std::collections::HashMap;

use mcp_conformance_core::canonical::to_canonical_string;
use mcp_conformance_core::message::{MessageKind, is_notification_method};
use mcp_conformance_core::trace::Direction;
use serde_json::Value;

use super::FindingSink;
use crate::context::TraceContext;

mod correlation;
mod meta;
mod schemas;

pub(super) use correlation::{error_id_matches, result_id_matches};
pub(super) use meta::meta_key_format;
pub(super) use schemas::{schemas_use_2020_12, schemas_valid_for_dialect};
// The `_meta` key grammar is unchanged at `2026-07-28`, where VERS-004 reuses it
// for extension identifiers — the function, not `base.meta-key-format`, which
// reads envelope keys and would inspect no identifier at all.
pub(super) use meta::validate_meta_key;

/// Human name of a JSON value's type, for finding details.
fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                "an integer"
            } else {
                "a non-integer number"
            }
        }
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn id_is_string_or_integer(id: &Value) -> bool {
    match id {
        Value::String(_) => true,
        Value::Number(number) => number.is_i64() || number.is_u64(),
        _ => false,
    }
}

/// `BASE-001`: "Requests MUST include a string or integer ID."
pub(super) fn request_id_type(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        let MessageKind::Request { method, id } = kind else {
            continue;
        };
        sink.examined();
        if !id_is_string_or_integer(id) {
            sink.push(
                    Some(event.seq),
                    format!(
                        "request {method:?} carries {} as its id; the ID must be a string or an integer",
                    type_name(id)
                ),
            );
        }
    }
}

/// `BASE-002`: "Unlike base JSON-RPC, the ID MUST NOT be `null`."
pub(super) fn request_id_not_null(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        let MessageKind::Request { method, id } = kind else {
            continue;
        };
        sink.examined();
        if id.is_null() {
            sink.push(
                Some(event.seq),
                format!("request {method:?} carries a null id, which MCP forbids"),
            );
        }
    }
}

/// `BASE-003`: "The request ID MUST NOT have been previously used by the requestor
/// within the same session."
pub(super) fn request_id_unique(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let mut first_use: HashMap<(Direction, String), u64> = HashMap::new();
    for (event, kind, _) in context.messages() {
        if let MessageKind::Request { method, id } = kind {
            if id.is_null() {
                continue; // BASE-002's finding; don't double-report.
            }
            sink.examined();
            let key = (event.direction, to_canonical_string(id));
            match first_use.get(&key) {
                Some(previous) => sink.push(
                    Some(event.seq),
                    format!(
                        "request {method:?} reuses id {}, already used by the same party at seq {previous}",
                        key.1
                    ),
                ),
                None => {
                    first_use.insert(key, event.seq);
                }
            }
        }
    }
}

/// `BASE-005`: "Notifications MUST NOT include an ID."
///
/// A message in the reserved `notifications/` namespace that carries an `id`
/// classifies structurally as a request; this check is what catches it.
pub(super) fn notification_no_id(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        let MessageKind::Request { method, .. } = kind else {
            continue;
        };
        sink.examined();
        if is_notification_method(method) {
            sink.push(
                    Some(event.seq),
                    format!(
                    "{method:?} is a notification method but the message carries an id; notifications must not include one"
                ),
            );
        }
    }
}

/// `BASE-006`: "Error responses MUST include an `error` field with a `code` and
/// `message`."
pub(super) fn error_shape(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        if let MessageKind::Error { error, .. } = kind {
            sink.examined();
            let Some(object) = error.as_object() else {
                sink.push(
                    Some(event.seq),
                    format!("error member is {}, expected an object", type_name(error)),
                );
                continue;
            };
            if !object.contains_key("code") {
                sink.push(
                    Some(event.seq),
                    "error object lacks a code member".to_owned(),
                );
            }
            match object.get("message") {
                None => sink.push(
                    Some(event.seq),
                    "error object lacks a message member".to_owned(),
                ),
                Some(message) if !message.is_string() => sink.push(
                    Some(event.seq),
                    format!(
                        "error message member is {}, expected a string",
                        type_name(message)
                    ),
                ),
                Some(_) => {}
            }
        }
    }
}

/// `BASE-007`: "Error codes MUST be integers."
pub(super) fn error_code_integer(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        let MessageKind::Error { error, .. } = kind else {
            continue;
        };
        // The subject is an error *carrying a code*: an error with none is
        // BASE-006's finding, and this clause has nothing to judge there.
        let Some(code) = error.get("code") else {
            continue;
        };
        sink.examined();
        if !code.is_i64() && !code.is_u64() {
            sink.push(
                Some(event.seq),
                format!("error code is {}, expected an integer", type_name(code)),
            );
        }
    }
}

/// `BASE-010`: "Result responses MUST include a `result` field." A message carrying
/// an `id` and no `method` is response-shaped; if it then carries neither `result`
/// nor `error`, it is a result response missing its `result` member (an error
/// response would carry `error` instead).
///
/// `Result`-classified messages are subjects too, and counting them is the point:
/// they carry the member, so they are this clause *complied with*. Examining only
/// the `Invalid` ones left the check unable to report a pass at all — a session
/// full of well-formed results reported `not observed`, which says the trace
/// carried nothing this clause binds to and was plainly untrue. An outcome a
/// check can never reach is a check nothing proves accepts conforming input.
pub(super) fn result_field(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        // An `Error` response is deliberately not a subject: the clause binds
        // *result* responses, and an error legitimately carries no `result`.
        if !matches!(
            kind,
            MessageKind::Invalid { .. } | MessageKind::Result { .. }
        ) {
            continue;
        }
        let Some(object) = event.message_payload().and_then(Value::as_object) else {
            continue;
        };
        if !object.contains_key("id") || object.contains_key("method") {
            continue;
        }
        sink.examined();
        // One member, not both, and the classifier is why. A message reaching
        // here is `Invalid` with an `id` and no `method`, and `classify`'s own
        // table leaves exactly two ways for that to happen: it carries *both*
        // `result` and `error` (ambiguous) or *neither* (this clause's
        // finding). The two tests can therefore never disagree, so asking both
        // is a condition no trace can vary independently — dead weight that
        // reads as thoroughness. The mutation gate found it.
        if !object.contains_key("result") {
            sink.push(
                Some(event.seq),
                "response-shaped message (id present, no method) carries no result field"
                    .to_owned(),
            );
        }
    }
}

/// `BASE-008`: "All messages between MCP clients and servers MUST follow the JSON-RPC
/// 2.0 specification." — verified here as: the message classifies as a JSON-RPC shape
/// and carries `"jsonrpc": "2.0"`.
pub(super) fn jsonrpc_version(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for (event, kind, _) in context.messages() {
        sink.examined();
        if let MessageKind::Invalid { reason } = kind {
            sink.push(
                Some(event.seq),
                format!("message is not a JSON-RPC request, notification, or response: {reason}"),
            );
            continue;
        }
        let version = event
            .message_payload()
            .and_then(|payload| payload.get("jsonrpc"));
        match version {
            Some(Value::String(version)) if version == "2.0" => {}
            Some(other) => sink.push(
                Some(event.seq),
                format!("jsonrpc member is {other}, expected the string \"2.0\""),
            ),
            None => sink.push(
                Some(event.seq),
                "message lacks the jsonrpc member; JSON-RPC 2.0 requires \"jsonrpc\": \"2.0\""
                    .to_owned(),
            ),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
