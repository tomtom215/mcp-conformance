// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `client/elicitation`: what a server may ask for, in which mode, with which
//! parameters — and the client's side, where an answer shows it.
//!
//! Every check here reads [`asks`](super::asks) or [`answers`](super::asks), so
//! the same sentence is judged the same way at `2025-11-25`, where the server
//! sends `elicitation/create`, and at `2026-07-28`, where it puts one inside an
//! `input_required` result.

use serde_json::Value;

use super::super::FindingSink;
use super::asks::{Answer, Ask, Surface, answers_for, asks_for};
use super::uri;
use crate::context::TraceContext;

/// The elicitation method.
const ELICIT: &str = "elicitation/create";

/// An elicitation's mode, as the page defines it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    /// `mode: "form"`, or no `mode` at all: "Clients MUST treat requests
    /// without a `mode` field as form mode."
    Form,
    /// `mode: "url"`.
    Url,
    /// Any other value, which selects neither and is judged only by the clause
    /// that lists the permitted ones.
    Other,
}

impl Mode {
    /// The mode `ask` requests.
    pub(super) fn of(ask: &Ask<'_>) -> Self {
        ask.param("mode")
            .map_or(Self::Form, |mode| match mode.as_str() {
                Some("form") => Self::Form,
                Some("url") => Self::Url,
                _ => Self::Other,
            })
    }

    /// The capability member that declares it.
    const fn member(self) -> &'static str {
        match self {
            Self::Form => "form",
            Self::Url | Self::Other => "url",
        }
    }
}

/// Whether `surface` declares `mode`; `None` when nothing can be known.
///
/// "An empty capabilities object is equivalent to declaring support for `form`
/// mode only", so form mode is declared by an `elicitation` naming `form`, or
/// naming neither mode. An `elicitation` that is not an object names no mode
/// and is read the same way: its shape is the client's defect, not a mode.
pub(super) fn mode_declared(surface: Surface<'_>, mode: Mode) -> Option<bool> {
    if !surface.knowable() {
        return None;
    }
    let Some(capability) = surface.at(&["elicitation"]) else {
        return Some(false);
    };
    let names = |member: &str| capability.get(member).is_some_and(|value| !value.is_null());
    Some(match mode {
        Mode::Form => names("form") || !names("url"),
        Mode::Url => names("url"),
        Mode::Other => false,
    })
}

/// `ELIC` "Clients that support … MUST declare the … capability", for any of
/// the three client features: a client that answers an ask with a result
/// supports the feature, so where it answered it must have declared it.
///
/// An error answer is not support — `2025-11-25` tells a client without roots to
/// answer `-32601` — so only results are subjects.
pub(super) fn declared_where_answered(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    method: &str,
    capability: &str,
) {
    for answer in answers_for(context, method) {
        if answer.result.is_none() {
            continue;
        }
        let Some(declared) = answer.declared.declares(&[capability]) else {
            continue;
        };
        sink.examined();
        if !declared {
            sink.push(
                Some(answer.seq),
                format!(
                    "the client answered {} with a result, so it supports `{capability}`, but \
                     {} did not declare the `{capability}` capability",
                    answer.ask.label(),
                    if answer.ask.is_mrtr() {
                        "the retry's `clientCapabilities`"
                    } else {
                        answer.ask.declaration()
                    }
                ),
            );
        }
    }
}

/// "Clients that support elicitation MUST declare the `elicitation` capability".
pub(in crate::checks) fn client_declares_capability(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    declared_where_answered(context, sink, ELICIT, "elicitation");
}

/// "Servers MUST NOT send elicitation requests with modes that are not
/// supported by the client."
pub(in crate::checks) fn mode_supported(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, ELICIT) {
        let mode = Mode::of(&ask);
        if mode == Mode::Other {
            continue; // Not a mode at all: `request_parameters`' finding.
        }
        let Some(declared) = mode_declared(ask.declared, mode) else {
            continue;
        };
        sink.examined();
        if !declared {
            let missing = if ask.declared.declares(&["elicitation"]) == Some(true) {
                format!("`elicitation.{}`", mode.member())
            } else {
                "`elicitation` at all".to_owned()
            };
            sink.push(
                Some(ask.seq),
                format!(
                    "{} asks for {} mode, but {} does not declare {missing}",
                    ask.label(),
                    mode.member(),
                    ask.declaration(),
                ),
            );
        }
    }
}

/// "All elicitation requests MUST include the following parameters": a string
/// `message`, and a `mode` that is `form` or `url` when it is present — the
/// table makes it "Optional for form mode".
pub(in crate::checks) fn request_parameters(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, ELICIT) {
        sink.examined();
        if !ask.param("message").is_some_and(Value::is_string) {
            sink.push(
                Some(ask.seq),
                format!("{} carries no string `message`", ask.label()),
            );
        }
        if Mode::of(&ask) == Mode::Other {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} has `mode` {}, which is neither `form` nor `url`",
                    ask.label(),
                    ask.param("mode").map_or_else(String::new, Value::to_string)
                ),
            );
        }
    }
}

/// "Form mode elicitation requests MUST … include these additional
/// parameters": a `requestedSchema` object.
pub(in crate::checks) fn form_schema_present(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, ELICIT) {
        if Mode::of(&ask) != Mode::Form {
            continue;
        }
        sink.examined();
        if !ask.param("requestedSchema").is_some_and(Value::is_object) {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} is a form mode elicitation without a `requestedSchema` object",
                    ask.label()
                ),
            );
        }
    }
}

/// The URL mode parameters an ask leaves out, of `required`.
fn missing_strings(ask: &Ask<'_>, required: &[&'static str]) -> Vec<&'static str> {
    required
        .iter()
        .copied()
        .filter(|name| !ask.param(name).is_some_and(Value::is_string))
        .collect()
}

/// Reports a URL mode ask missing any of `required`, each a string parameter.
fn url_parameters_named(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
    required: &[&'static str],
) {
    for ask in asks_for(context, ELICIT) {
        if Mode::of(&ask) != Mode::Url {
            continue;
        }
        sink.examined();
        let missing = missing_strings(&ask, required);
        if !missing.is_empty() {
            sink.push(
                Some(ask.seq),
                format!(
                    "{} is a URL mode elicitation without the string parameter(s) {}",
                    ask.label(),
                    missing
                        .iter()
                        .map(|name| format!("`{name}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }
}

/// `2026-07-28`: "URL mode elicitation requests MUST specify `mode: "url"`, a
/// `message`, and include these additional parameters" — `url`.
pub(in crate::checks) fn url_parameters(context: &TraceContext<'_>, sink: &mut FindingSink) {
    url_parameters_named(context, sink, &["message", "url"]);
}

/// `2025-11-25`: the same sentence, whose table adds `elicitationId`.
pub(in crate::checks) fn url_parameters_with_id(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    url_parameters_named(context, sink, &["message", "url", "elicitationId"]);
}

/// "The `url` parameter MUST contain a valid URL": RFC 3986 syntax, scheme
/// required (see [`uri`]).
pub(in crate::checks) fn url_valid(context: &TraceContext<'_>, sink: &mut FindingSink) {
    for ask in asks_for(context, ELICIT) {
        if Mode::of(&ask) != Mode::Url {
            continue;
        }
        // A missing or non-string `url` is the parameter clause's finding.
        let Some(url) = ask.param("url").and_then(Value::as_str) else {
            continue;
        };
        sink.examined();
        if let Err(why) = uri::check(url) {
            sink.push(
                Some(ask.seq),
                format!("{} has the `url` {url:?}, which {why}", ask.label()),
            );
        }
    }
}

/// The schema a form mode answer is held to, when it asked with one.
fn requested_schema<'a>(answer: &Answer<'a>) -> Option<&'a Value> {
    (Mode::of(&answer.ask) == Mode::Form)
        .then(|| answer.ask.param("requestedSchema"))
        .flatten()
        .filter(|schema| schema.is_object())
}

/// "Clients SHOULD validate all responses against the provided schema".
///
/// Validation is internal; its wire-visible outcome is an `accept` whose
/// `content` the schema rejects, which a client that validated would not have
/// sent. Judged in the dialect the schema declares, with `format` not asserted
/// (an annotation in 2020-12, ADR-0020); a schema this engine cannot compile
/// judges nothing.
pub(in crate::checks) fn response_matches_schema(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for answer in answers_for(context, ELICIT) {
        let Some(result) = answer.result else {
            continue;
        };
        if result.get("action").and_then(Value::as_str) != Some("accept") {
            continue; // Only an accepted form submits content.
        }
        let (Some(schema), Some(submitted)) = (requested_schema(&answer), result.get("content"))
        else {
            continue;
        };
        let Some(validator) = crate::wire_schema::compile_declared(schema) else {
            continue;
        };
        sink.examined();
        let errors = crate::wire_schema::instance_errors(&validator, submitted, "");
        if !errors.is_empty() {
            sink.push(
                Some(answer.seq),
                format!(
                    "the client accepted {} with `content` its `requestedSchema` rejects: {}",
                    answer.ask.label(),
                    crate::wire_schema::summarize(&errors)
                ),
            );
        }
    }
}

/// `2025-11-25` "Clients MUST return standard JSON-RPC errors for common failure
/// cases: Server sends an `elicitation/create` request with a mode not declared
/// in client capabilities: `-32602` (Invalid params)".
///
/// The subject is a direct request in a mode the client left out of an
/// `elicitation` capability it did declare — without the capability the
/// request is for an unsupported method, a different failure. An `inputRequests`
/// entry has no JSON-RPC response to carry an error, so it is not one.
pub(in crate::checks) fn undeclared_mode_rejected(
    context: &TraceContext<'_>,
    sink: &mut FindingSink,
) {
    for answer in answers_for(context, ELICIT) {
        if answer.ask.is_mrtr() {
            continue;
        }
        let mode = Mode::of(&answer.ask);
        if mode == Mode::Other || answer.ask.declared.declares(&["elicitation"]) != Some(true) {
            continue;
        }
        if mode_declared(answer.ask.declared, mode) != Some(false) {
            continue;
        }
        sink.examined();
        if answer.error_code != Some(-32602) {
            sink.push(
                Some(answer.seq),
                format!(
                    "the client answered {} mode, which it did not declare, with {} rather than \
                     the `-32602` error",
                    mode.member(),
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
