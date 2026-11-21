// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Arguments for a tool call, built from the tool's own `inputSchema` and
//! from nothing else (ADR-0024, decision 3).
//!
//! Only the members a schema requires are built, each from what the schema
//! itself says: `const`, `default`, the first `enum` value, or the smallest
//! value its type and bounds allow. Where the schema does not say enough — a
//! `pattern`, an unknown `format`, an untyped member — nothing is built and the
//! tool is not called: a guessed argument would exercise the guess, not the
//! server. Whatever is built is then checked against the schema with the JSON
//! Schema engine the validator uses, so a call is never made with arguments the
//! server's own schema refuses.

use serde_json::{Map, Number, Value, json};

/// How deep a schema is followed (through `properties`, `items` and `$ref`).
const MAX_DEPTH: usize = 16;

/// The arguments object `schema` (a tool's `inputSchema`) requires.
///
/// # Errors
///
/// Why none can be built, in the words the report gives for the skipped tool.
pub(crate) fn build(schema: &Value) -> Result<Value, String> {
    let arguments = object(schema, schema, 0)?;
    let validator = jsonschema::validator_for(schema)
        .map_err(|error| format!("its inputSchema is not a valid JSON Schema ({error})"))?;
    if let Some(error) = validator.iter_errors(&arguments).next() {
        return Err(format!(
            "the arguments its inputSchema describes do not satisfy it ({error})"
        ));
    }
    Ok(arguments)
}

/// An object holding `schema`'s required members.
fn object(schema: &Value, root: &Value, depth: usize) -> Result<Value, String> {
    let empty = Map::new();
    let properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut built = Map::new();
    for name in schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let member = properties
            .get(name)
            .ok_or_else(|| format!("required argument `{name}` has no schema"))?;
        let value = value(member, root, depth + 1)
            .map_err(|reason| format!("required argument `{name}`: {reason}"))?;
        built.insert(name.to_owned(), value);
    }
    Ok(Value::Object(built))
}

/// A value `schema` accepts, from what the schema states.
fn value(schema: &Value, root: &Value, depth: usize) -> Result<Value, String> {
    if depth > MAX_DEPTH {
        return Err(format!("its schema nests deeper than {MAX_DEPTH} levels"));
    }
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
        let target = reference
            .strip_prefix('#')
            .and_then(|pointer| root.pointer(pointer))
            .ok_or_else(|| format!("`$ref` {reference:?} does not resolve inside the schema"))?;
        return value(target, root, depth + 1);
    }
    for stated in ["const", "default"] {
        if let Some(value) = schema.get(stated) {
            return Ok(value.clone());
        }
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|values| values.first())
    {
        return Ok(first.clone());
    }
    for keyword in ["anyOf", "oneOf"] {
        if let Some(branches) = schema.get(keyword).and_then(Value::as_array) {
            return branches
                .iter()
                .find_map(|branch| value(branch, root, depth + 1).ok())
                .ok_or_else(|| format!("no `{keyword}` branch states a value that can be built"));
        }
    }
    if schema.get("allOf").is_some() {
        return Err("`allOf` is not combined".to_owned());
    }
    let types: Vec<&str> = match schema.get("type") {
        Some(Value::String(one)) => vec![one.as_str()],
        Some(Value::Array(several)) => several.iter().filter_map(Value::as_str).collect(),
        _ if schema.get("properties").is_some() => vec!["object"],
        _ => return Err("its schema states no type".to_owned()),
    };
    let mut refusal = "its schema states no type".to_owned();
    for name in types {
        match typed(name, schema, root, depth) {
            Ok(value) => return Ok(value),
            Err(reason) => refusal = reason,
        }
    }
    Err(refusal)
}

/// A value of the JSON Schema type `name` within `schema`'s bounds.
fn typed(name: &str, schema: &Value, root: &Value, depth: usize) -> Result<Value, String> {
    match name {
        "string" => string(schema),
        "integer" | "number" => number(schema),
        "boolean" => Ok(Value::Bool(false)),
        "null" => Ok(Value::Null),
        "object" => object(schema, root, depth),
        "array" => array(schema, root, depth),
        other => Err(format!("type {other:?} is not a JSON Schema type")),
    }
}

fn bound(schema: &Value, keyword: &str) -> Option<u64> {
    schema.get(keyword).and_then(Value::as_u64)
}

fn string(schema: &Value) -> Result<Value, String> {
    if schema.get("pattern").is_some() {
        return Err("a `pattern` constrains it, and no value is stated".to_owned());
    }
    if let Some(format) = schema.get("format").and_then(Value::as_str) {
        let text = match format {
            "date-time" => "2026-01-01T00:00:00Z",
            "date" => "2026-01-01",
            "time" => "00:00:00Z",
            "email" => "user@example.com",
            "uri" | "iri" | "uri-reference" | "iri-reference" => "https://example.com/",
            "uuid" => "00000000-0000-4000-8000-000000000000",
            "hostname" => "example.com",
            "ipv4" => "192.0.2.1",
            "ipv6" => "2001:db8::1",
            other => return Err(format!("format {other:?} has no value this tool builds")),
        };
        return Ok(Value::String(text.to_owned()));
    }
    let shortest = bound(schema, "minLength").unwrap_or(0);
    let longest = bound(schema, "maxLength").unwrap_or(u64::MAX);
    if shortest > 1024 {
        return Err(format!("it must be at least {shortest} characters"));
    }
    let length = 7.clamp(shortest, longest.max(shortest));
    let text: String = "example"
        .chars()
        .cycle()
        .take(usize::try_from(length).unwrap_or(7))
        .collect();
    Ok(Value::String(text))
}

/// The value closest to zero within the schema's bounds; a multiple of
/// `multipleOf` when one is asked for.
fn number(schema: &Value) -> Result<Value, String> {
    let lowest = schema
        .get("minimum")
        .and_then(Value::as_i64)
        .or_else(|| {
            schema
                .get("exclusiveMinimum")
                .and_then(Value::as_i64)
                .map(|bound| bound.saturating_add(1))
        })
        .unwrap_or(i64::MIN);
    let highest = schema
        .get("maximum")
        .and_then(Value::as_i64)
        .or_else(|| {
            schema
                .get("exclusiveMaximum")
                .and_then(Value::as_i64)
                .map(|bound| bound.saturating_sub(1))
        })
        .unwrap_or(i64::MAX);
    if lowest > highest {
        return Err("its bounds admit no integer".to_owned());
    }
    let mut chosen = 0.clamp(lowest, highest);
    if let Some(step) = schema.get("multipleOf") {
        let step = step
            .as_i64()
            .filter(|step| *step > 0)
            .ok_or_else(|| "its `multipleOf` is not a positive integer".to_owned())?;
        // The nearest multiple at or above the chosen value, or failing that
        // the nearest at or below it: each is within one step.
        let over = chosen.rem_euclid(step);
        chosen = chosen
            .checked_add((step - over) % step)
            .filter(|multiple| *multiple <= highest)
            .or_else(|| {
                chosen
                    .checked_sub(over)
                    .filter(|multiple| *multiple >= lowest)
            })
            .ok_or_else(|| "its bounds admit no multiple of `multipleOf`".to_owned())?;
    }
    Ok(Value::Number(Number::from(chosen)))
}

fn array(schema: &Value, root: &Value, depth: usize) -> Result<Value, String> {
    let count = bound(schema, "minItems").unwrap_or(0);
    if count == 0 {
        return Ok(json!([]));
    }
    if count > 16 {
        return Err(format!("it must hold at least {count} items"));
    }
    if count > 1 && schema.get("uniqueItems") == Some(&Value::Bool(true)) {
        return Err("it must hold several distinct items".to_owned());
    }
    let items = schema
        .get("items")
        .filter(|items| items.is_object())
        .ok_or_else(|| "it must hold items, and no `items` schema says of what".to_owned())?;
    let item = value(items, root, depth + 1)?;
    Ok(Value::Array(vec![
        item;
        usize::try_from(count).unwrap_or(1)
    ]))
}

#[cfg(test)]
mod tests;
