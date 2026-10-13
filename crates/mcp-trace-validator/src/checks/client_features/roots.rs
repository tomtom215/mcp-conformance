// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `client/roots`: what a `roots/list` answer may contain, whether the server
//! checked the capability before asking, and the client's error when it has no
//! roots to give.

use serde_json::Value;

use super::super::FindingSink;
use super::asks::{answers_for, asks_for};
use super::elicitation::declared_where_answered;
use crate::context::TraceContext;

/// The roots method.
const ROOTS: &str = "roots/list";

/// "Clients that support roots MUST declare the `roots` capability".
pub(in crate::checks) fn client_declares_capability(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_where_answered(context, sink, ROOTS, "roots");
}

/// Whether `uri` is a `file://` URI; the scheme compared without case, as RFC
/// 3986 §3.1 compares schemes.
fn is_file_uri(uri: &str) -> bool {
    uri.get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file://"))
}

/// "`uri`: Unique identifier for the root. This MUST be a `file://` URI in the
/// current specification."
pub(in crate::checks) fn uri_file_scheme(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for answer in answers_for(context, ROOTS) {
        let roots = answer
            .result
            .and_then(|result| result.get("roots"))
            .and_then(Value::as_array);
        for (index, root) in roots.into_iter().flatten().enumerate() {
            sink.examined();
            let uri = root.get("uri").and_then(Value::as_str);
            if !uri.is_some_and(is_file_uri) {
                sink.push(
                    Some(answer.seq),
                    format!(
                        "the client's answer to {} lists `roots[{index}]` with {}, not a \
                         `file://` URI",
                        answer.ask.label(),
                        uri.map_or_else(|| "no string `uri`".to_owned(), |uri| format!("{uri:?}"))
                    ),
                );
            }
        }
    }
}

/// "Servers SHOULD: Check for roots capability before usage" — its
/// wire-visible failure is asking a client that did not declare `roots`.
pub(in crate::checks) fn capability_checked(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, ROOTS) {
        let Some(declared) = ask.declared.declares(&["roots"]) else {
            continue;
        };
        sink.examined();
        if !declared {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} asks for roots, but {} does not declare `roots`",
                    ask.label(),
                    ask.declaration()
                ),
            );
        }
    }
}

/// `2025-11-25` "Clients SHOULD return standard JSON-RPC errors for common
/// failure cases: Client does not support roots: `-32601` (Method not found)".
///
/// A client that did not declare `roots` is taken at its word: answering such a
/// request with roots would be [`client_declares_capability`]'s finding, and
/// any other error is not the one the row names.
pub(in crate::checks) fn unsupported_rejected(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for answer in answers_for(context, ROOTS) {
        if answer.ask.is_mrtr() || answer.ask.declared.declares(&["roots"]) != Some(false) {
            continue;
        }
        sink.examined();
        if answer.error_code != Some(-32601) {
            sink.push(
                Some(answer.seq),
                format!(
                    "the client, which did not declare `roots`, answered {} with {} rather than \
                     the `-32601` error",
                    answer.ask.label(),
                    answer
                        .error_code
                        .map_or_else(|| "a result".to_owned(), |code| format!("error {code}"))
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests;
