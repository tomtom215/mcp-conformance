// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::{Designated, designated, encode, mirrored};

/// The specification's own table of encoding examples
/// (basic/transports/streamable-http, "Value Encoding").
#[test]
fn the_specifications_encoding_examples() {
    assert_eq!(encode("us-west1"), "us-west1");
    assert_eq!(encode("Hello, 世界"), "=?base64?SGVsbG8sIOS4lueVjA==?=");
    assert_eq!(encode(" padded "), "=?base64?IHBhZGRlZCA=?=");
    assert_eq!(encode("line1\nline2"), "=?base64?bGluZTEKbGluZTI=?=");
    assert_eq!(
        encode("=?base64?literal?="),
        "=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?="
    );
}

#[test]
fn only_values_a_header_cannot_carry_unchanged_are_encoded() {
    // Interior spaces and tabs are field content; only the edges are trimmed
    // by HTTP, so only there do they force the sentinel form.
    assert_eq!(encode("a b\tc"), "a b\tc");
    assert_eq!(encode("~!"), "~!");
    assert_eq!(encode(""), "");
    for forced in ["trailing ", "\tleading", "del\u{7f}", "nul\u{0}", "é"] {
        assert!(encode(forced).starts_with("=?base64?"), "{forced:?}");
    }
    // Half a sentinel is plain text.
    assert_eq!(encode("=?base64?open"), "=?base64?open");
    assert_eq!(encode("close?="), "close?=");
}

#[test]
fn annotations_are_found_through_properties_chains_only() {
    let schema = json!({"type": "object", "properties": {
        "region": {"type": "string", "x-mcp-header": "Region"},
        "plain": {"type": "string"},
        "nested": {"type": "object", "properties": {
            "limit": {"type": "integer", "x-mcp-header": "Limit"}}}}});
    let mut found = designated(&schema).unwrap();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        found,
        [
            Designated {
                name: "Limit".to_owned(),
                path: vec!["nested".to_owned(), "limit".to_owned()]
            },
            Designated {
                name: "Region".to_owned(),
                path: vec!["region".to_owned()]
            },
        ]
    );
    assert_eq!(designated(&json!({"type": "object"})).unwrap(), []);
}

#[test]
fn a_parameter_merely_named_x_mcp_header_is_not_an_annotation() {
    let schema = json!({"type": "object", "properties": {
        "x-mcp-header": {"type": "string"}}});
    assert_eq!(designated(&schema).unwrap(), []);
}

#[test]
fn invalid_definitions_are_rejected_with_the_reason() {
    let cases = [
        (
            json!({"properties": {"a": {"type": "string", "x-mcp-header": ""}}}),
            "field-name token",
        ),
        (
            json!({"properties": {"a": {"type": "string", "x-mcp-header": "Two Words"}}}),
            "field-name token",
        ),
        (
            json!({"properties": {"a": {"type": "string", "x-mcp-header": 5}}}),
            "field-name token",
        ),
        (
            json!({"properties": {"a": {"type": "number", "x-mcp-header": "N"}}}),
            "not a string, integer or boolean",
        ),
        (
            json!({"properties": {"a": {"x-mcp-header": "N"}}}),
            "not a string, integer or boolean",
        ),
        (
            json!({"properties": {
            "a": {"type": "string", "x-mcp-header": "Same"},
            "b": {"type": "string", "x-mcp-header": "sAME"}}}),
            "more than once",
        ),
        (
            json!({"properties": {"a": {"type": "array", "items": {"type": "string", "x-mcp-header": "N"}}}}),
            "no chain",
        ),
        (
            json!({"anyOf": [{"properties": {"a": {"type": "string", "x-mcp-header": "N"}}}]}),
            "no chain",
        ),
        (
            json!({"properties": {"a": {"$ref": "#/$defs/d"}}, "$defs": {"d": {"type": "string", "x-mcp-header": "N"}}}),
            "no chain",
        ),
    ];
    for (schema, word) in cases {
        let reason = designated(&schema).unwrap_err();
        assert!(reason.contains(word), "{schema}: {reason}");
    }
}

#[test]
fn present_values_are_mirrored_as_text_and_absent_ones_omitted() {
    let schema = json!({"properties": {
        "region": {"type": "string", "x-mcp-header": "Region"},
        "limit": {"type": "integer", "x-mcp-header": "Limit"},
        "dry": {"type": "boolean", "x-mcp-header": "Dry-Run"},
        "absent": {"type": "string", "x-mcp-header": "Absent"},
        "nested": {"properties": {"city": {"type": "string", "x-mcp-header": "City"}}}}});
    let designated = designated(&schema).unwrap();
    let arguments = json!({"region": "us-west1", "limit": -7, "dry": true,
        "nested": {"city": "Zürich"}});
    let mut headers = mirrored(&designated, &arguments).unwrap();
    headers.sort();
    assert_eq!(
        headers,
        [
            (
                "Mcp-Param-City".to_owned(),
                "=?base64?WsO8cmljaA==?=".to_owned()
            ),
            ("Mcp-Param-Dry-Run".to_owned(), "true".to_owned()),
            ("Mcp-Param-Limit".to_owned(), "-7".to_owned()),
            ("Mcp-Param-Region".to_owned(), "us-west1".to_owned()),
        ]
    );
}

#[test]
fn an_integer_outside_the_safe_range_is_not_sent() {
    let designated = [Designated {
        name: "N".to_owned(),
        path: vec!["n".to_owned()],
    }];
    let edge = 9_007_199_254_740_991_i64;
    assert_eq!(
        mirrored(&designated, &json!({"n": edge})).unwrap(),
        [("Mcp-Param-N".to_owned(), edge.to_string())]
    );
    assert_eq!(
        mirrored(&designated, &json!({"n": -edge})).unwrap(),
        [("Mcp-Param-N".to_owned(), (-edge).to_string())]
    );
    for outside in [
        json!({"n": edge + 1}),
        json!({"n": -edge - 1}),
        json!({"n": 1.5}),
    ] {
        assert!(mirrored(&designated, &outside).is_err(), "{outside}");
    }
    // A value of a type the annotation cannot designate has no text form.
    assert_eq!(mirrored(&designated, &json!({"n": null})).unwrap(), []);
    assert_eq!(mirrored(&designated, &json!({"n": [1]})).unwrap(), []);
}
