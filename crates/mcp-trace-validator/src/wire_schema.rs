// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Wire-schema validation: every recorded message against the judged
//! revision's own JSON Schema (ADR-0020).
//!
//! The specification's TypeScript schema is "the source of truth for all
//! protocol messages and structures", and the JSON Schema generated from it is
//! what tooling validates against. Most of what it constrains — a result's
//! required members, a member's type, an enumeration's values, a content
//! block's shape — has no sentence of its own in the prose, so no requirement
//! row can carry it. This module judges it beside the rows: one
//! [`WireSchemaReport`] per report, failing when any message is invalid.
//!
//! Which definition a message is held to follows the official suite's
//! `wire-schema-valid` check, so the two read a message the same way: a
//! request or notification by its `method`, an error by its `code`, a result
//! by the method of the request it answers (paired as every check pairs them),
//! and anything unpaired or unnamed by the JSON-RPC envelope alone. Where that
//! would hold a message to a definition the schema does not have, it falls
//! back to the envelope rather than guessing — a missed finding, never a false
//! one.

use core::fmt::Write as _;
use std::collections::BTreeMap;
use std::sync::OnceLock;

use jsonschema::error::ValidationErrorKind;
use jsonschema::{Draft, ValidationError, Validator, ValidatorMap};
use mcp_conformance_core::revision::ProtocolRevision;
use mcp_conformance_core::wire_schema::{self, WIRE_SCHEMAS, WireSchema};
use serde_json::Value;

use crate::context::TraceContext;
use crate::report::{ClauseSource, Finding, Outcome, Party, WireSchemaReport};

/// The `check` every wire-schema finding names.
pub const CHECK: &str = "wire-schema";

/// Errors listed per message before the rest are counted. The suite lists six;
/// one message's first few errors are what a reader acts on.
const MAX_ERRORS: usize = 6;

/// Characters of one error's text kept before it is cut: an `anyOf` failure
/// repeats the whole offending value, which can be megabytes.
const MAX_ERROR_CHARS: usize = 200;

/// Union definitions that alias several concrete ones and so would claim a
/// `method` they do not define (the suite skips the same eight).
const NON_CANONICAL: &[&str] = &[
    "ClientRequest",
    "ClientNotification",
    "ClientResult",
    "ClientMessage",
    "ServerRequest",
    "ServerNotification",
    "ServerResult",
    "ServerMessage",
];

/// `resultType` values an extension defines, whose result schema is not
/// vendored: such a result is held to the envelope only (the suite's list).
const EXTENSION_RESULT_TYPES: &[&str] = &["task"];

/// One revision's schema, compiled once per process.
struct Compiled {
    validators: ValidatorMap,
    /// Every definition name the schema has.
    definitions: Vec<String>,
    /// JSON-RPC method → its request or notification definition.
    methods: BTreeMap<String, String>,
    /// JSON-RPC method → its result definition, where the schema names one.
    results: BTreeMap<String, String>,
    /// `error.code` constant → its typed error-response definition.
    errors: BTreeMap<i64, String>,
}

impl Compiled {
    fn has(&self, definition: &str) -> bool {
        self.definitions.iter().any(|name| name == definition)
    }

    /// The first of `names` the schema defines, else the last.
    fn first_of<'n>(&self, names: &[&'n str]) -> &'n str {
        names
            .iter()
            .copied()
            .find(|name| self.has(name))
            .unwrap_or(names[names.len() - 1])
    }

    /// The errors `instance` raises against `definition`, rendered, sorted and
    /// de-duplicated so the order cannot depend on the engine's traversal.
    fn errors(&self, definition: &str, instance: &Value, prefix: &str) -> Vec<String> {
        let Some(validator) = self.validators.get(&format!("#/$defs/{definition}")) else {
            return Vec::new();
        };
        instance_errors(validator, instance, prefix)
    }
}

/// The errors `instance` raises against `validator`, as report lines: each
/// [`explain`]ed, then sorted and de-duplicated so the order cannot depend on
/// the engine's traversal. `prefix` is prepended to every instance path.
pub(crate) fn instance_errors(
    validator: &Validator,
    instance: &Value,
    prefix: &str,
) -> Vec<String> {
    let mut errors: Vec<String> = validator
        .iter_errors(instance)
        .flat_map(|error| explain(&error, prefix))
        .collect();
    errors.sort();
    errors.dedup();
    errors
}

/// `errors` for a finding's detail: the first [`MAX_ERRORS`], then a count
/// of the rest.
pub(crate) fn summarize(errors: &[String]) -> String {
    let mut summary = errors
        .iter()
        .take(MAX_ERRORS)
        .cloned()
        .collect::<Vec<_>>()
        .join("; ");
    if errors.len() > MAX_ERRORS {
        let _ = write!(summary, "; and {} more", errors.len() - MAX_ERRORS);
    }
    summary
}

/// One engine error as report lines.
///
/// An `anyOf`/`oneOf` that no branch satisfies says only that, which names
/// no defect; its closest branch is reported instead — the one whose errors
/// reach deepest into the value, then the one with fewest, then the first, so
/// the choice is a function of the schema and the message alone. A form-mode
/// elicitation without `requestedSchema` reads "matches none of 3
/// alternatives; closest: …/params: "requestedSchema" is a required property",
/// not "is not valid under any of the schemas".
fn explain(error: &ValidationError<'_>, prefix: &str) -> Vec<String> {
    let path = format!("{prefix}{}", error.instance_path());
    let branches = match error.kind() {
        ValidationErrorKind::AnyOf { context } | ValidationErrorKind::OneOfNotValid { context } => {
            Some(context)
        }
        _ => None,
    };
    if let Some(closest) = branches.and_then(|branches| closest(branches)) {
        let count = branches.map_or(0, Vec::len);
        let inner: Vec<String> = closest
            .iter()
            .flat_map(|error| explain(error, prefix))
            .collect();
        return vec![format!(
            "{}: matches none of {count} alternatives; closest: {}",
            if path.is_empty() { "/" } else { &path },
            inner.join(" and ")
        )];
    }
    let text = truncate(&error.to_string());
    vec![if path.is_empty() {
        text
    } else {
        format!("{path}: {text}")
    }]
}

/// The branch most likely meant: one whose discriminator matched (no `const`
/// failure — a `method` or `type` constant that differs names another variant
/// entirely), then the one whose errors got furthest into the value (deepest
/// instance path), then the one with fewest errors, then the earliest.
fn closest<'e>(
    branches: &'e [Vec<ValidationError<'static>>],
) -> Option<&'e [ValidationError<'static>]> {
    branches
        .iter()
        .filter(|errors| !errors.is_empty())
        .enumerate()
        .min_by_key(|(index, errors)| {
            let depth = errors
                .iter()
                .map(|error| error.instance_path().into_iter().count())
                .max()
                .unwrap_or(0);
            let wrong_variant = errors
                .iter()
                .any(|error| matches!(error.kind(), ValidationErrorKind::Constant { .. }));
            (
                wrong_variant,
                core::cmp::Reverse(depth),
                errors.len(),
                *index,
            )
        })
        .map(|(_, errors)| errors.as_slice())
}

fn truncate(text: &str) -> String {
    match text.char_indices().nth(MAX_ERROR_CHARS) {
        Some((cut, _)) => format!("{}…", &text[..cut]),
        None => text.to_owned(),
    }
}

/// Compiles `schema`, applying its errata first.
///
/// # Panics
///
/// When the vendored document does not parse, an erratum's pointer resolves to
/// nothing, or the schema does not compile — all properties of data embedded
/// at build time, which `mcp-conformance-core`'s tests and this module's pin.
#[allow(clippy::expect_used)]
fn compile(schema: &WireSchema) -> Compiled {
    let mut document: Value =
        serde_json::from_str(schema.json).expect("the vendored schema is valid JSON");
    for erratum in schema.errata {
        let corrected: Value =
            serde_json::from_str(erratum.corrected).expect("an erratum's correction is JSON");
        *document
            .pointer_mut(erratum.pointer)
            .expect("an erratum's pointer resolves in the vendored schema") = corrected;
    }
    let validators = jsonschema::options()
        .with_draft(Draft::Draft202012)
        // In 2020-12 `format` is an annotation unless asserted, and the schema
        // uses three. `byte` is asserted: the prose says the same thing of
        // every such member ("base64-encoded"), the alphabet has one
        // definition, and no clause judges a tool result's encoding, so this
        // is the only place a malformed image could be caught. `uri` and
        // `uri-template` stay annotations (ADR-0020): asserting RFC 3986 would
        // fail URIs the prose permits. The official suite does the opposite —
        // `ajv-formats` asserts both URI formats and `addFormat("byte", true)`
        // accepts any string — which the agreement check records.
        .should_validate_formats(true)
        .with_format("byte", crate::checks::is_base64)
        .with_format("uri", |_: &str| true)
        .with_format("uri-template", |_: &str| true)
        .build_map(&document)
        .expect("the vendored schema compiles");
    let mut compiled = Compiled {
        validators,
        definitions: Vec::new(),
        methods: BTreeMap::new(),
        results: BTreeMap::new(),
        errors: BTreeMap::new(),
    };
    let definitions = document
        .get("$defs")
        .and_then(Value::as_object)
        .expect("the vendored schema has $defs");
    compiled.definitions = definitions.keys().cloned().collect();
    for (name, definition) in definitions {
        if NON_CANONICAL.contains(&name.as_str()) {
            continue;
        }
        if let Some(method) = definition
            .pointer("/properties/method/const")
            .and_then(Value::as_str)
        {
            compiled.methods.insert(method.to_owned(), name.clone());
            if let Some(stem) = name.strip_suffix("Request") {
                let result = format!("{stem}Result");
                if definitions.contains_key(&result) {
                    compiled.results.insert(method.to_owned(), result);
                }
            }
        }
        let parts = definition
            .pointer("/properties/error/allOf")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for part in parts {
            if let Some(code) = part
                .pointer("/properties/code/const")
                .and_then(Value::as_i64)
            {
                compiled.errors.insert(code, name.clone());
            }
        }
    }
    compiled
}

/// The compiled schema for `revision`, compiled on first use.
fn compiled(revision: ProtocolRevision) -> Option<(&'static WireSchema, &'static Compiled)> {
    static CACHE: [OnceLock<Compiled>; 2] = [OnceLock::new(), OnceLock::new()];
    let index = WIRE_SCHEMAS
        .iter()
        .position(|schema| schema.revision == revision)?;
    let schema = &WIRE_SCHEMAS[index];
    let slot = CACHE.get(index)?;
    Some((schema, slot.get_or_init(|| compile(schema))))
}

/// Validates every message in `context` against `revision`'s schema; `None`
/// when this build vendors no schema for `revision`.
pub(crate) fn check(
    revision: ProtocolRevision,
    context: &TraceContext<'_>,
) -> Option<WireSchemaReport> {
    let (schema, compiled) = compiled(revision)?;
    // Response seq → the request it answers: its method and its params.
    let answered: BTreeMap<u64, (&str, Option<&Value>)> = context
        .exchanges()
        .map(|exchange| (exchange.response.seq, (exchange.method, exchange.params)))
        .collect();
    let mut messages = 0u32;
    let mut findings = Vec::new();
    for event in context.events() {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        if !(payload.is_object() || payload.is_array()) {
            // A raw line the recorder kept as text: not a JSON value to judge.
            continue;
        }
        if context.refused(event.seq) && payload.get("jsonrpc").is_none() {
            // An authorization refusal's own body, not a protocol message
            // (`context::refusal`). A refused *request* is still judged: it
            // was a JSON-RPC message, only an unread one.
            continue;
        }
        messages = messages.saturating_add(1);
        let (definition, errors) = judge(compiled, revision, payload, answered.get(&event.seq));
        if errors.is_empty() {
            continue;
        }
        // The schema binds whoever sent the message, so the sender is the
        // party held to it.
        let party = Party::sender(event.direction);
        let detail = format!(
            "the {party}'s message is not a valid {definition} under the {revision} schema: {}",
            summarize(&errors)
        );
        findings.push(Finding {
            check: CHECK.to_owned(),
            seq: Some(event.seq),
            party: Some(party),
            detail,
        });
    }
    let outcome = if messages == 0 {
        Outcome::NotObserved
    } else if findings.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail
    };
    Some(WireSchemaReport {
        outcome,
        messages,
        findings,
        schema: schema.url(),
        source: ClauseSource::from_source(&schema.source(), revision),
    })
}

/// The definition `payload` is held to and the errors it raises — the suite's
/// dispatch (module docs). `answers` is the request a response answers.
fn judge(
    compiled: &Compiled,
    revision: ProtocolRevision,
    payload: &Value,
    answers: Option<&(&str, Option<&Value>)>,
) -> (String, Vec<String>) {
    let envelope = |definition: &str| {
        let errors = compiled.errors(definition, payload, "");
        (definition.to_owned(), errors)
    };
    let Some(message) = payload.as_object() else {
        // An array: a batch, which neither vendored revision admits.
        return envelope("JSONRPCMessage");
    };
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        return match compiled.methods.get(method) {
            Some(definition) => envelope(definition),
            None if message.contains_key("id") => envelope("JSONRPCRequest"),
            None => envelope("JSONRPCNotification"),
        };
    }
    if let Some(error) = message.get("error").filter(|error| !error.is_null()) {
        let typed = error
            .get("code")
            .and_then(Value::as_i64)
            .and_then(|code| compiled.errors.get(&code));
        return envelope(typed.map_or_else(
            || compiled.first_of(&["JSONRPCErrorResponse", "JSONRPCError"]),
            String::as_str,
        ));
    }
    let Some(result) = message.get("result") else {
        return envelope("JSONRPCMessage");
    };
    let generic = compiled.first_of(&["JSONRPCResultResponse", "JSONRPCResponse"]);
    if let Some(typed) = result_definition(compiled, revision, result, answers) {
        let errors = compiled.errors(&typed, result, "/result");
        if !errors.is_empty() {
            return (typed, errors);
        }
    }
    envelope(generic)
}

/// The definition a result is held to, beyond the envelope; `None` for the
/// envelope alone.
fn result_definition(
    compiled: &Compiled,
    revision: ProtocolRevision,
    result: &Value,
    answers: Option<&(&str, Option<&Value>)>,
) -> Option<String> {
    // `2026-07-28` (SEP-2322): any request may be answered with an
    // `InputRequiredResult`; an extension's `resultType` has no vendored schema.
    let result_type = result.get("resultType").and_then(Value::as_str);
    if result_type == Some("input_required") && compiled.has("InputRequiredResult") {
        return Some("InputRequiredResult".to_owned());
    }
    if result_type.is_some_and(|kind| EXTENSION_RESULT_TYPES.contains(&kind)) {
        return None;
    }
    let (method, params) = answers?;
    // `2025-11-25` tasks: a task-augmented request is answered with a
    // `CreateTaskResult`, not its method's result.
    if revision == mcp_conformance_core::revision::REVISION_2025_11_25
        && params.is_some_and(|params| params.get("task").is_some())
        && compiled.has("CreateTaskResult")
    {
        return Some("CreateTaskResult".to_owned());
    }
    compiled.results.get(*method).cloned()
}

/// Whether `revision` has a vendored schema in this build.
#[must_use]
pub fn covers(revision: ProtocolRevision) -> bool {
    wire_schema::for_revision(revision).is_some()
}

mod dialect;

pub(crate) use dialect::{compile_declared, schema_errors};

#[cfg(test)]
mod tests;
