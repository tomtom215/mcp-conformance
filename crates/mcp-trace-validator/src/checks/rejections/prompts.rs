// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! server/prompts#error-handling: the code a refused `prompts/get` carries.

use serde_json::Value;

use super::super::FindingSink;
use super::listing::Listing;
use super::{Answer, INVALID_PARAMS};
use crate::context::TraceContext;

#[cfg(test)]
mod tests;

/// `PROM-007` / `PROM-019`: "Invalid prompt name: `-32602`", "Missing required
/// arguments: `-32602`".
///
/// **Stimuli**, either of which a trace may carry:
///
/// 1. *A missing required argument.* A `prompts/get` naming a prompt the
///    session's `prompts/list` results declare, and omitting an argument every
///    one of those declarations marks `required: true`. The declaration and the
///    call are both recorded, so the failure case is the trace's own. Not
///    judged once a `notifications/prompts/list_changed` was seen (the
///    declaration may be stale), nor for a retry carrying `inputResponses`
///    (its arguments may have arrived that way).
/// 2. *An unknown name.* A `prompts/get` naming a prompt absent from a
///    *complete, unchanged* `prompts/list` enumeration (see
///    [`Listing`]), **answered with an error**. A server may serve a prompt it
///    does not list, so a result here is the server's word that the name is
///    valid and is not judged; a refusal of a name it never listed is its word
///    that the name is not.
///
/// **Conforming answers:** error `-32602`; and for the missing argument, a
/// `resultType: "input_required"` result (`2026-07-28`), which is the server
/// asking for what it lacks.
///
/// **Reported:** any other error code, and — for the missing argument, whose
/// antecedent needs no word from the server — any other result. `-32603` is
/// the clause's own third case and is never judged.
pub(in crate::checks) fn get_error_codes(context: &TraceContext<'_>, sink: &mut FindingSink) {
    let listing = Listing::of(
        context,
        "prompts/list",
        "prompts",
        "notifications/prompts/list_changed",
    );
    let known = listing.known("name");
    let required = listing.required_arguments();
    for exchange in context.exchanges_for("prompts/get") {
        let Some(params) = exchange.params else {
            continue;
        };
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            continue;
        };
        let answer = Answer::of(&exchange);
        if answer.is_another_failure() {
            continue;
        }
        let missing = omitted(required.get(name), params);
        if !missing.is_empty() {
            if params.get("inputResponses").is_some() {
                continue;
            }
            // The subject is a call the server's own declaration makes invalid.
            sink.examined();
            if !asks_for_input(answer) && !answer.is_error(INVALID_PARAMS) {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "`prompts/get` {name:?} omitted the required argument(s) {missing:?} \
                         the server declared in `prompts/list`, and drew {} rather than -32602",
                        answer.describe()
                    ),
                );
            }
        } else if matches!(answer, Answer::Error(_))
            && known.as_ref().is_some_and(|known| !known.contains(name))
        {
            // The subject is a refusal of a name the server never listed.
            sink.examined();
            if !answer.is_error(INVALID_PARAMS) {
                sink.push(
                    Some(exchange.response.seq),
                    format!(
                        "`prompts/get` named {name:?}, which the session's complete \
                         `prompts/list` does not list, and the server refused it with {} \
                         rather than -32602",
                        answer.describe()
                    ),
                );
            }
        }
    }
}

/// The `required` arguments a `prompts/get` with `params` leaves out.
fn omitted<'a>(
    required: Option<&std::collections::BTreeSet<&'a str>>,
    params: &Value,
) -> Vec<&'a str> {
    required
        .into_iter()
        .flatten()
        .filter(|argument| {
            params
                .get("arguments")
                .and_then(|arguments| arguments.get(**argument))
                .is_none()
        })
        .copied()
        .collect()
}

/// Whether `answer` is the `2026-07-28` result that asks the client for input
/// rather than serving the prompt.
fn asks_for_input(answer: Answer<'_>) -> bool {
    matches!(
        answer,
        Answer::Result(result)
            if result.get("resultType").and_then(Value::as_str) == Some("input_required")
    )
}
