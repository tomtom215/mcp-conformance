// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! server/utilities/completion#error-handling: the code a refused
//! `completion/complete` carries.

use serde_json::Value;

use super::super::FindingSink;
use super::super::stateless::capabilities::declares;
use super::super::support::{Declaration, server_capability};
use super::listing::Listing;
use super::{Answer, INVALID_PARAMS, METHOD_NOT_FOUND};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// Whether the server declared `completions`, on whichever declaration
/// surface the session has: the `initialize` result (`2025-11-25`) or, where
/// there is none, the `server/discover` result (`2026-07-28`). `None` when the
/// trace carries neither, and the session's capabilities are unknowable.
fn completions_declared(context: &TraceContext<'_>) -> Option<bool> {
    match server_capability(context, &["completions"]) {
        Declaration::Declared => Some(true),
        Declaration::Withheld => Some(false),
        Declaration::Unknowable => declares(context, "completions"),
    }
}

/// `COMP-003` / `COMP-008`: "Method not found: `-32601` (Capability not
/// supported)", "Invalid prompt name: `-32602`", "Missing required arguments:
/// `-32602`".
///
/// **Stimuli**, each a `completion/complete` request:
///
/// 1. *Capability not supported.* The server's declaration (the `initialize`
///    result, or the `server/discover` result) is in the trace and withholds
///    `completions`, and the request was **answered with an error**. A result
///    would be the server supporting completions after all — COMP-001's and
///    COMP-007's matter, not this clause's. Conforming: `-32601`.
/// 2. *A missing required argument.* The server declared `completions` and the
///    request carries no `params.argument` or no `params.ref`, both of which
///    the schema requires. Conforming: `-32602`; any other answer, a result
///    included, is reported, because the request is invalid on its face.
/// 3. *An invalid prompt name.* The server declared `completions`, the request
///    is otherwise whole, its `ref` is a `ref/prompt` naming a prompt absent
///    from a complete, unchanged `prompts/list` enumeration, and it was
///    **answered with an error** — a server may complete for a prompt it does
///    not list, so a result is not judged. Conforming: `-32602`.
///
/// Not judged anywhere: `-32603`, the clause's own fourth case, and a session
/// whose declarations the trace does not hold.
pub(in crate::checks) fn error_codes(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let Some(declared) = completions_declared(context) else {
        return;
    };
    let prompts = Listing::of(
        context,
        "prompts/list",
        "prompts",
        "notifications/prompts/list_changed",
    )
    .known("name");
    for exchange in context.exchanges_for("completion/complete") {
        let answer = Answer::of(&exchange);
        if answer.is_another_failure() {
            continue;
        }
        let refused = matches!(answer, Answer::Error(_));
        let (case, expected) = if !declared {
            if !refused {
                continue;
            }
            (
                "the server declared no `completions` capability".to_owned(),
                METHOD_NOT_FOUND,
            )
        } else if let Some(member) = ["argument", "ref"].into_iter().find(|member| {
            exchange
                .params
                .and_then(|params| params.get(*member))
                .is_none()
        }) {
            (
                format!("the request carries no `params.{member}`"),
                INVALID_PARAMS,
            )
        } else if let Some(name) = unknown_prompt(exchange.params, prompts.as_ref())
            && refused
        {
            (
                format!(
                    "the request refers to prompt {name:?}, which the session's complete \
                     `prompts/list` does not list"
                ),
                INVALID_PARAMS,
            )
        } else {
            continue;
        };
        // The subject is a request the trace itself shows to be one of the
        // clause's failure cases.
        sink.examined();
        if !answer.is_error(expected) {
            sink.push(
                Some(exchange.response.seq),
                format!(
                    "`completion/complete` drew {} rather than {expected}, though {case}",
                    answer.describe()
                ),
            );
        }
    }
}

/// The prompt a `ref/prompt` reference names, when `known` is the server's
/// whole prompt listing and does not hold it.
fn unknown_prompt<'a>(
    params: Option<&'a Value>,
    known: Option<&std::collections::BTreeSet<&str>>,
) -> Option<&'a str> {
    let reference = params?.get("ref")?;
    if reference.get("type").and_then(Value::as_str) != Some("ref/prompt") {
        return None;
    }
    let name = reference.get("name").and_then(Value::as_str)?;
    (!known?.contains(name)).then_some(name)
}
