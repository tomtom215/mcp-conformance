// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Schemas an implementation declares — a tool's `inputSchema` and
//! `outputSchema`, an elicitation's `requestedSchema` — read in their own
//! dialect, for the clauses about them (`TOOL-011`/`TOOL-040`,
//! `BASE-017`/`BASE-073`).

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a private module; this follows the rustc
// lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use jsonschema::{Draft, Validator};
use serde_json::Value;

use super::explain;

/// The dialect `schema` is written in: its `$schema`, or JSON Schema 2020-12 —
/// every vendored revision's default for a schema without one. `None` for a
/// dialect the engine does not know, which no check here can judge (whether a
/// peer supports it is BASE-015/BASE-071's question, and excluded).
fn declared_dialect(schema: &Value) -> Option<Draft> {
    match schema.get("$schema") {
        None => Some(Draft::Draft202012),
        Some(Value::String(uri)) => match Draft::from_schema_uri(uri) {
            Draft::Unknown => None,
            draft => Some(draft),
        },
        Some(_) => None,
    }
}

/// `schema` compiled in its own dialect; `None` when the dialect is unknown or
/// the schema does not compile — invalid, or referring to a document outside
/// itself, which this engine never fetches. A schema that cannot be compiled
/// cannot be applied; whether it is valid is [`schema_errors`]' question.
pub(crate) fn compile_declared(schema: &Value) -> Option<Validator> {
    jsonschema::options()
        .with_draft(declared_dialect(schema)?)
        .should_validate_formats(false)
        .build(schema)
        .ok()
}

/// What makes `schema` invalid in its declared dialect, by that dialect's
/// metaschema, as sorted report lines; `None` for a dialect the engine does
/// not know.
pub(crate) fn schema_errors(schema: &Value) -> Option<Vec<String>> {
    let metaschema = match declared_dialect(schema)? {
        Draft::Draft4 => jsonschema::draft4::meta::validator(),
        Draft::Draft6 => jsonschema::draft6::meta::validator(),
        Draft::Draft7 => jsonschema::draft7::meta::validator(),
        Draft::Draft201909 => jsonschema::draft201909::meta::validator(),
        _ => jsonschema::draft202012::meta::validator(),
    };
    let mut errors: Vec<String> = metaschema
        .iter_errors(schema)
        .flat_map(|error| explain(&error, ""))
        .collect();
    errors.sort();
    errors.dedup();
    Some(errors)
}
