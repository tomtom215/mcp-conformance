// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The content an accepted form submits, held to the form's own schema
//! ([`accepted_content`]).

use rmcp::model::{
    ElicitationSchema, EnumSchema, IntegerSchema, MultiSelectEnumSchema, NumberSchema,
    PrimitiveSchemaDefinition, SingleSelectEnumSchema, StringFormat, StringSchema,
};
use serde_json::{Map, Value};

/// The plain string placeholder, before `minLength`/`maxLength` are applied.
const PLACEHOLDER: &str = "scripted";

/// The content an `accept` of `schema` submits, or `None` when a required
/// property cannot be satisfied and the form must be declined.
///
/// `2026-07-28` (`client/elicitation#form-mode-security`, ELIC-095): "Clients
/// SHOULD validate all responses against the provided schema". A scripted
/// host has no user to type the missing fields, so validating is not enough
/// on its own — the answer has to be *made* valid, or not sent. The rule, in
/// the order it is applied:
///
/// 1. Every property carrying a `default` gets it (SEP-1034), required or not,
///    exactly as [`defaults_from_schema`](crate::script::defaults_from_schema) fills
///    them.
/// 2. Every **required** property still missing gets a deterministic value of
///    its type, chosen to satisfy its constraints:
///
///    | Type | Value |
///    |---|---|
///    | `string`, no `format` | `"scripted"`, padded with `x` to `minLength`, cut to `maxLength` |
///    | `string`, `format: email` | `"user@example.com"` |
///    | `string`, `format: uri` | `"https://example.com/"` |
///    | `string`, `format: date` | `"1970-01-01"` |
///    | `string`, `format: date-time` | `"1970-01-01T00:00:00Z"` |
///    | `integer`, `number` | `0`, moved to `minimum` or `maximum` when outside them |
///    | `boolean` | `false` |
///    | single-select enum | its first value (a legacy enum's `default` first, when it has one) |
///    | multi-select enum | its first `minItems` values (none when `minItems` is absent) |
///
///    The values are placeholders and look it: `example.com` is reserved for
///    exactly this (RFC 2606), and the epoch is nobody's real date.
/// 3. If any required property cannot be satisfied — an enum with no values,
///    bounds that exclude each other, a formatted placeholder longer than
///    `maxLength`, a name `required` lists that `properties` does not define,
///    or a schema form this host does not recognise — the answer is **decline**.
///    Accepting with content the schema rejects is the defect ELIC-095 names;
///    declining is the answer a user would give a form they could not fill.
///
/// Optional properties without a default stay omitted: omission is the
/// correct answer for them, and inventing one would put words in the user's
/// mouth the server did not ask for.
///
/// ```
/// use mcp_reference_host::script::accepted_content;
///
/// let form = serde_json::from_value(serde_json::json!({
///     "type": "object",
///     "properties": {
///         "email": { "type": "string", "format": "email" },
///         "plan": { "type": "string", "enum": [] }
///     },
///     "required": ["email"]
/// }))?;
/// let content = accepted_content(&form).expect("satisfiable");
/// assert_eq!(content["email"], "user@example.com");
/// assert!(!content.contains_key("plan"), "optional, and left out");
/// # Ok::<(), serde_json::Error>(())
/// ```
#[must_use]
pub fn accepted_content(schema: &ElicitationSchema) -> Option<Map<String, Value>> {
    let mut content = super::defaults_from_schema(schema);
    for name in schema.required.iter().flatten() {
        if content.contains_key(name) {
            continue;
        }
        let value = placeholder(schema.properties.get(name)?)?;
        content.insert(name.clone(), value);
    }
    Some(content)
}

/// A deterministic value satisfying `property`, if one exists.
fn placeholder(property: &PrimitiveSchemaDefinition) -> Option<Value> {
    match property {
        PrimitiveSchemaDefinition::String(string) => string_placeholder(string).map(Value::String),
        PrimitiveSchemaDefinition::Integer(integer) => integer_placeholder(integer),
        PrimitiveSchemaDefinition::Number(number) => number_placeholder(number),
        PrimitiveSchemaDefinition::Boolean(_) => Some(Value::Bool(false)),
        PrimitiveSchemaDefinition::Enum(schema) => enum_placeholder(schema),
        // `#[non_exhaustive]`: a type this host cannot read is a field it
        // cannot fill, and filling it with a guess is what step 3 refuses.
        _ => None,
    }
}

/// Step 2's string, within `minLength`..=`maxLength`.
fn string_placeholder(schema: &StringSchema) -> Option<String> {
    let min = schema.min_length.map_or(0, |min| min as usize);
    let max = schema.max_length.map_or(usize::MAX, |max| max as usize);
    if min > max {
        return None;
    }
    let formatted = match schema.format {
        None => None,
        Some(StringFormat::Email) => Some("user@example.com"),
        Some(StringFormat::Uri) => Some("https://example.com/"),
        Some(StringFormat::Date) => Some("1970-01-01"),
        Some(StringFormat::DateTime) => Some("1970-01-01T00:00:00Z"),
        // A format this host does not know has no value it can vouch for.
        Some(_) => return None,
    };
    if let Some(value) = formatted {
        // Padding or cutting would break the format, so a placeholder that
        // does not fit as it is cannot be offered at all.
        return (min..=max).contains(&value.len()).then(|| value.to_owned());
    }
    let mut value: String = PLACEHOLDER.chars().take(max).collect();
    while value.len() < min {
        value.push('x');
    }
    Some(value)
}

/// Step 2's integer: zero, or the nearer bound when zero is outside them.
fn integer_placeholder(schema: &IntegerSchema) -> Option<Value> {
    let min = schema.minimum.unwrap_or(i64::MIN);
    let max = schema.maximum.unwrap_or(i64::MAX);
    (min <= max).then(|| Value::from(0_i64.clamp(min, max)))
}

/// Step 2's number: zero, or the nearer bound when zero is outside them.
fn number_placeholder(schema: &NumberSchema) -> Option<Value> {
    let min = schema.minimum.unwrap_or(f64::NEG_INFINITY);
    let max = schema.maximum.unwrap_or(f64::INFINITY);
    // Checked before `clamp`, which panics on either.
    if min.is_nan() || max.is_nan() || min > max {
        return None;
    }
    serde_json::Number::from_f64(0.0_f64.clamp(min, max)).map(Value::Number)
}

/// Step 2's enum choice.
fn enum_placeholder(schema: &EnumSchema) -> Option<Value> {
    match schema {
        EnumSchema::Single(single) => {
            let first = match single {
                SingleSelectEnumSchema::Untitled(schema) => schema.enum_.first(),
                SingleSelectEnumSchema::Titled(schema) => {
                    schema.one_of.first().map(|choice| &choice.const_)
                }
                _ => None,
            };
            first.cloned().map(Value::String)
        }
        EnumSchema::Multi(multi) => {
            let (choices, min_items, max_items): (Vec<&String>, _, _) = match multi {
                MultiSelectEnumSchema::Untitled(schema) => (
                    schema.items.enum_.iter().collect(),
                    schema.min_items,
                    schema.max_items,
                ),
                MultiSelectEnumSchema::Titled(schema) => (
                    schema
                        .items
                        .any_of
                        .iter()
                        .map(|choice| &choice.const_)
                        .collect(),
                    schema.min_items,
                    schema.max_items,
                ),
                _ => return None,
            };
            let wanted = usize::try_from(min_items.unwrap_or(0)).ok()?;
            let fits = max_items.is_none_or(|max| min_items.unwrap_or(0) <= max);
            (fits && wanted <= choices.len()).then(|| {
                Value::Array(
                    choices
                        .into_iter()
                        .take(wanted)
                        .cloned()
                        .map(Value::String)
                        .collect(),
                )
            })
        }
        EnumSchema::Legacy(legacy) => legacy
            .default
            .as_ref()
            .or_else(|| legacy.enum_.first())
            .cloned()
            .map(Value::String),
        _ => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests;
