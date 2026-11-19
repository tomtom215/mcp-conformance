// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use proptest::prelude::*;
use serde_json::{Value, json};

use super::build;

#[test]
fn only_required_members_are_built() {
    let schema = json!({"type": "object", "required": ["a"], "properties": {
        "a": {"type": "string"}, "b": {"type": "integer"}}});
    assert_eq!(build(&schema).unwrap(), json!({"a": "example"}));
}

#[test]
fn a_schema_requiring_nothing_gives_an_empty_object() {
    assert_eq!(build(&json!({"type": "object"})).unwrap(), json!({}));
    assert_eq!(build(&json!({})).unwrap(), json!({}));
}

#[test]
fn a_stated_value_is_used_before_a_type_is_consulted() {
    let schema = json!({"type": "object", "required": ["c", "d", "e"], "properties": {
        "c": {"const": "fixed", "type": "string"},
        "d": {"default": 7, "type": "integer"},
        "e": {"enum": ["first", "second"]}}});
    assert_eq!(
        build(&schema).unwrap(),
        json!({"c": "fixed", "d": 7, "e": "first"})
    );
}

#[test]
fn numbers_take_the_value_nearest_zero_within_their_bounds() {
    let cases = [
        (json!({"type": "integer"}), json!(0)),
        (json!({"type": "integer", "minimum": 3}), json!(3)),
        (json!({"type": "integer", "exclusiveMinimum": 3}), json!(4)),
        (json!({"type": "number", "maximum": -2}), json!(-2)),
        (
            json!({"type": "integer", "exclusiveMaximum": -2}),
            json!(-3),
        ),
        (
            json!({"type": "integer", "minimum": 5, "multipleOf": 4}),
            json!(8),
        ),
        (
            json!({"type": "integer", "minimum": 8, "multipleOf": 4}),
            json!(8),
        ),
        (
            json!({"type": "integer", "maximum": -5, "multipleOf": 4}),
            json!(-8),
        ),
    ];
    for (member, expected) in cases {
        let schema = json!({"type": "object", "required": ["n"], "properties": {"n": member}});
        assert_eq!(build(&schema).unwrap(), json!({"n": expected}), "{member}");
    }
}

#[test]
fn bounds_that_admit_nothing_are_refused() {
    for member in [
        json!({"type": "integer", "minimum": 5, "maximum": 4}),
        json!({"type": "integer", "minimum": 5, "maximum": 7, "multipleOf": 4}),
        json!({"type": "integer", "multipleOf": 0}),
        json!({"type": "integer", "multipleOf": 0.5}),
    ] {
        let schema = json!({"type": "object", "required": ["n"], "properties": {"n": member}});
        assert!(build(&schema).is_err(), "{member}");
    }
}

#[test]
fn strings_fit_their_length_bounds_and_known_formats() {
    let text = |member: Value| {
        let schema = json!({"type": "object", "required": ["s"], "properties": {"s": member}});
        build(&schema).unwrap()["s"].as_str().unwrap().to_owned()
    };
    assert_eq!(text(json!({"type": "string"})), "example");
    assert_eq!(text(json!({"type": "string", "maxLength": 3})), "exa");
    assert_eq!(text(json!({"type": "string", "minLength": 9})), "exampleex");
    assert_eq!(text(json!({"type": "string", "maxLength": 0})), "");
    assert_eq!(
        text(json!({"type": "string", "format": "uri"})),
        "https://example.com/"
    );
    assert_eq!(
        text(json!({"type": "string", "format": "date"})),
        "2026-01-01"
    );
}

#[test]
fn what_the_schema_does_not_state_is_not_guessed() {
    let refused = [
        (json!({"type": "string", "pattern": "^[a-z]+$"}), "pattern"),
        (json!({"type": "string", "format": "regex"}), "format"),
        (json!({"description": "anything"}), "no type"),
        (json!({"allOf": [{"type": "string"}]}), "allOf"),
        (json!({"type": "array", "minItems": 1}), "items"),
        (
            json!({"type": "array", "minItems": 2, "uniqueItems": true, "items": {"type": "boolean"}}),
            "distinct",
        ),
        (json!({"$ref": "#/$defs/missing"}), "does not resolve"),
        (
            json!({"$ref": "https://example.com/schema"}),
            "does not resolve",
        ),
        (json!({"type": "string", "minLength": 5000}), "at least"),
        (json!({"type": "widget"}), "not a JSON Schema type"),
    ];
    for (member, word) in refused {
        let schema = json!({"type": "object", "required": ["x"], "properties": {"x": member}});
        let reason = build(&schema).unwrap_err();
        assert!(
            reason.contains("`x`") && reason.contains(word),
            "{member}: {reason}"
        );
    }
    let unlisted = json!({"type": "object", "required": ["ghost"]});
    assert!(build(&unlisted).unwrap_err().contains("has no schema"));
}

#[test]
fn references_branches_arrays_and_nested_objects_are_followed() {
    let schema = json!({
        "type": "object",
        "required": ["r", "u", "a", "o", "t"],
        "properties": {
            "r": {"$ref": "#/$defs/flag"},
            "u": {"anyOf": [{"type": "string", "pattern": "x"}, {"type": "integer", "minimum": 2}]},
            "a": {"type": "array", "minItems": 2, "items": {"type": "boolean"}},
            "o": {"type": "object", "required": ["inner"], "properties": {"inner": {"type": "null"}}},
            "t": {"type": ["null", "string"]}
        },
        "$defs": {"flag": {"type": "boolean"}}
    });
    assert_eq!(
        build(&schema).unwrap(),
        json!({"r": false, "u": 2, "a": [false, false], "o": {"inner": null}, "t": null})
    );
}

#[test]
fn a_reference_cycle_ends_in_a_refusal_not_a_stack_overflow() {
    let schema = json!({"type": "object", "required": ["x"], "properties": {
        "x": {"$ref": "#/properties/x"}}});
    assert!(build(&schema).unwrap_err().contains("nests deeper"));
}

#[test]
fn arguments_the_schema_itself_refuses_are_never_returned() {
    // A default that breaks its own schema: built from what the schema
    // states, then caught by the schema.
    let schema = json!({"type": "object", "required": ["n"], "properties": {
        "n": {"type": "integer", "default": "seven"}}});
    assert!(build(&schema).unwrap_err().contains("do not satisfy it"));
    let invalid = json!({"type": "object", "required": 5});
    assert!(
        build(&invalid)
            .unwrap_err()
            .contains("not a valid JSON Schema")
    );
}

/// A member schema this module should always be able to satisfy.
fn buildable() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        (0_u64..40, 0_u64..40).prop_map(|(low, extra)| json!({
            "type": "string", "minLength": low, "maxLength": low + extra})),
        (-50_i64..50, 0_i64..50).prop_map(|(low, extra)| json!({
            "type": "integer", "minimum": low, "maximum": low + extra})),
        (-50_i64..50, 1_i64..9).prop_map(|(low, step)| json!({
            "type": "integer", "minimum": low, "multipleOf": step})),
        Just(json!({"type": "boolean"})),
        Just(json!({"type": "null"})),
        Just(json!({"type": "string", "format": "date-time"})),
        Just(json!({"enum": ["a", 1, null]})),
    ];
    leaf.prop_recursive(3, 12, 3, |inner| {
        prop_oneof![
            (inner.clone(), 0_u64..4).prop_map(
                |(items, count)| json!({"type": "array", "minItems": count, "items": items})
            ),
            inner.clone().prop_map(
                |member| json!({"type": "object", "required": ["k"], "properties": {"k": member}})
            ),
            inner
                .prop_map(|member| json!({"anyOf": [{"type": "string", "pattern": "^$"}, member]})),
        ]
    })
}

proptest! {
    /// For every schema in the family this module claims to handle, arguments
    /// are built, and they satisfy the schema under an independent check here
    /// (not only the one inside `build`).
    #[test]
    fn a_buildable_schema_gets_arguments_that_satisfy_it(member in buildable()) {
        let schema = json!({"type": "object", "required": ["x"], "properties": {"x": member}});
        let arguments = build(&schema).unwrap();
        prop_assert!(jsonschema::is_valid(&schema, &arguments), "{schema} / {arguments}");
        prop_assert!(arguments.get("x").is_some());
    }
}
