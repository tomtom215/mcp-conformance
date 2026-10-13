// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use serde_json::json;

use super::annotations;

#[test]
fn a_chain_of_properties_keys_is_static_at_any_depth() {
    let schema = json!({"type": "object", "properties": {
        "region": {"type": "string", "x-mcp-header": "Region"},
        "target": {"type": "object", "properties": {
            "zone": {"type": "string", "x-mcp-header": "Zone"}}}}});
    assert_eq!(
        annotations(&schema),
        [
            ("/properties/region".to_owned(), true),
            ("/properties/target/properties/zone".to_owned(), true)
        ]
    );
}

/// Each keyword the clause names breaks the chain, and so does `$defs`
/// (reached only through `$ref`) and an annotation on the root itself.
#[test]
fn every_forbidden_step_breaks_the_chain() {
    let annotated = json!({"type": "string", "x-mcp-header": "H"});
    for (schema, at) in [
        (json!({"items": annotated}), "/items"),
        (json!({"prefixItems": [annotated]}), "/prefixItems/0"),
        (json!({"anyOf": [annotated]}), "/anyOf/0"),
        (json!({"oneOf": [annotated]}), "/oneOf/0"),
        (json!({"allOf": [annotated]}), "/allOf/0"),
        (json!({"not": annotated}), "/not"),
        (json!({"if": annotated}), "/if"),
        (json!({"then": annotated}), "/then"),
        (json!({"else": annotated}), "/else"),
        (json!({"$defs": {"h": annotated}}), "/$defs/h"),
        (
            json!({"additionalProperties": annotated}),
            "/additionalProperties",
        ),
        (
            json!({"patternProperties": {"^a": annotated}}),
            "/patternProperties/^a",
        ),
        (
            json!({"properties": {"list": {"items": {"properties": {"x": annotated}}}}}),
            "/properties/list/items/properties/x",
        ),
        (json!({"x-mcp-header": "Root"}), "/"),
    ] {
        assert_eq!(annotations(&schema), [(at.to_owned(), false)], "{schema}");
    }
}

/// Instance data is not a schema: the key inside `default`, `const`, `enum`,
/// `examples` or an unknown keyword is not an annotation.
#[test]
fn instance_data_carrying_the_key_is_not_an_annotation() {
    let data = json!({"x-mcp-header": "Data"});
    let schema = json!({"type": "object", "properties": {"p": {
        "type": "object", "default": data, "const": data, "enum": [data],
        "examples": [data], "x-vendor": data}}});
    assert_eq!(annotations(&schema), []);
}

#[test]
fn reference_tokens_are_escaped() {
    let schema = json!({"properties": {"a/b~c": {"type": "string", "x-mcp-header": "A"}}});
    assert_eq!(
        annotations(&schema),
        [("/properties/a~1b~0c".to_owned(), true)]
    );
}
