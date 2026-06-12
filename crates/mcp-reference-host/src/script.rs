// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Scripted interaction: every behavior a model or user would supply, as data.
//!
//! The host's client handlers ([`crate::handler`]) consult an
//! [`InteractionScript`] instead of a model, a UI, or any network — CI runs are
//! reproducible by construction (ADR-0009). The SEP-1034 defaults policy is the
//! behavior the official suite's `elicitation-sep1034-client-defaults` scenario
//! requires: a client accepting a form fills every omitted field that carries a
//! schema default before responding.

use rmcp::model::{
    ElicitationSchema, EnumSchema, MultiSelectEnumSchema, PrimitiveSchema, Root,
    SingleSelectEnumSchema,
};
use serde_json::{Map, Value};

/// Deterministic answers for everything a server may ask of this host.
#[derive(Debug, Clone)]
pub struct InteractionScript {
    /// Text every `sampling/createMessage` answers with.
    pub sampling_reply: String,
    /// Model name reported in sampling results.
    pub sampling_model: String,
    /// How form-mode `elicitation/create` requests are answered.
    pub elicitation: ElicitationPolicy,
    /// How URL-mode `elicitation/create` requests are answered.
    pub url_elicitation: UrlElicitationPolicy,
    /// The roots this host exposes to `roots/list`.
    pub roots: Vec<Root>,
}

impl Default for InteractionScript {
    /// The conformance-run script: accept forms by applying SEP-1034 schema
    /// defaults, consent to URL elicitations, answer sampling with a fixed
    /// line, and expose one synthetic project root.
    fn default() -> Self {
        Self {
            sampling_reply: "Scripted response".to_owned(),
            sampling_model: "scripted-model".to_owned(),
            elicitation: ElicitationPolicy::AcceptWithDefaults,
            url_elicitation: UrlElicitationPolicy::AcceptConsent,
            roots: vec![Root::new("file:///workspace/project").with_name("project")],
        }
    }
}

/// Answer policy for form-mode elicitation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElicitationPolicy {
    /// Accept, filling every field that carries a schema default (SEP-1034)
    /// and omitting the rest.
    AcceptWithDefaults,
    /// Accept with exactly this content object.
    AcceptWith(Map<String, Value>),
    /// Decline the request (the user said no).
    Decline,
    /// Cancel the request (the user dismissed it).
    Cancel,
}

/// Answer policy for URL-mode elicitation. Accepting records consent to
/// navigate; the interaction itself is out of band and its completion arrives
/// (if ever) as `notifications/elicitation/complete`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlElicitationPolicy {
    /// Consent: respond `accept` (no content — URL mode carries none).
    AcceptConsent,
    /// Refuse: respond `decline`.
    Decline,
}

/// The SEP-1034 defaults of `schema`, as elicitation response content.
///
/// Every property carrying a `default` appears with that value; properties
/// without one are omitted (they are optional, and omission is the correct
/// answer).
#[must_use]
pub fn defaults_from_schema(schema: &ElicitationSchema) -> Map<String, Value> {
    let mut content = Map::new();
    for (name, property) in &schema.properties {
        if let Some(value) = default_of(property) {
            content.insert(name.clone(), value);
        }
    }
    content
}

/// The default value of one primitive property, when it declares one.
fn default_of(property: &PrimitiveSchema) -> Option<Value> {
    match property {
        PrimitiveSchema::String(string) => string.default.clone().map(Value::String),
        PrimitiveSchema::Number(number) => number
            .default
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number),
        PrimitiveSchema::Integer(integer) => {
            integer.default.map(|value| Value::Number(value.into()))
        }
        PrimitiveSchema::Boolean(boolean) => boolean.default.map(Value::Bool),
        PrimitiveSchema::Enum(EnumSchema::Single(single)) => match single {
            SingleSelectEnumSchema::Untitled(schema) => schema.default.clone().map(Value::String),
            SingleSelectEnumSchema::Titled(schema) => schema.default.clone().map(Value::String),
        },
        PrimitiveSchema::Enum(EnumSchema::Multi(multi)) => {
            let default = match multi {
                MultiSelectEnumSchema::Untitled(schema) => schema.default.clone(),
                MultiSelectEnumSchema::Titled(schema) => schema.default.clone(),
            };
            default.map(|values| Value::Array(values.into_iter().map(Value::String).collect()))
        }
        // The legacy enum form (SEP-1330's predecessor) defines no default.
        PrimitiveSchema::Enum(EnumSchema::Legacy(_)) => None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The exact five-field schema the suite's SEP-1034 scenario sends, by
    /// type and default: string "John Doe", integer 30, number 95.5, enum
    /// "active", boolean true — all optional.
    fn suite_schema() -> ElicitationSchema {
        serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "default": "John Doe" },
                "age": { "type": "integer", "default": 30 },
                "score": { "type": "number", "default": 95.5 },
                "status": { "type": "string", "enum": ["active", "inactive"], "default": "active" },
                "subscribe": { "type": "boolean", "default": true }
            },
            "required": []
        }))
        .unwrap()
    }

    #[test]
    fn suite_scenario_defaults_are_extracted_exactly() {
        let content = defaults_from_schema(&suite_schema());
        assert_eq!(content.len(), 5, "{content:?}");
        assert_eq!(content["name"], "John Doe");
        assert_eq!(content["age"], 30);
        assert_eq!(content["score"], 95.5);
        assert_eq!(content["status"], "active");
        assert_eq!(content["subscribe"], true);
    }

    #[test]
    fn properties_without_defaults_are_omitted_not_invented() {
        let schema: ElicitationSchema = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "with": { "type": "string", "default": "x" },
                "without": { "type": "string" }
            },
            "required": []
        }))
        .unwrap();
        let content = defaults_from_schema(&schema);
        assert_eq!(content.len(), 1);
        assert!(!content.contains_key("without"), "{content:?}");
    }

    #[test]
    fn multi_select_defaults_become_string_arrays() {
        let schema: ElicitationSchema = serde_json::from_value(serde_json::json!({
            "type": "object",
            "properties": {
                "tags": {
                    "type": "array",
                    "items": { "type": "string", "enum": ["a", "b", "c"] },
                    "default": ["a", "c"]
                }
            },
            "required": []
        }))
        .unwrap();
        let content = defaults_from_schema(&schema);
        assert_eq!(content["tags"], serde_json::json!(["a", "c"]));
    }

    #[test]
    fn default_script_is_the_conformance_posture() {
        let script = InteractionScript::default();
        assert_eq!(script.elicitation, ElicitationPolicy::AcceptWithDefaults);
        assert_eq!(script.url_elicitation, UrlElicitationPolicy::AcceptConsent);
        assert!(!script.roots.is_empty(), "roots/list must have an answer");
    }
}
