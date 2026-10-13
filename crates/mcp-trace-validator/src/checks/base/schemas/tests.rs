// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

fn findings(messages: &[(&str, Value)]) -> (Vec<String>, u32) {
    let text = messages
        .iter()
        .enumerate()
        .map(|(seq, (direction, payload))| {
            json!({"seq": seq, "direction": direction, "transport": "stdio",
                   "kind": "message", "payload": payload})
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let events = parse_trace(&text, &Limits::default()).unwrap();
    let outcome = checks::find("base.schemas-valid-for-dialect")
        .unwrap()
        .run(&TraceContext::new(&events));
    (
        outcome
            .findings
            .into_iter()
            .map(|finding| finding.detail)
            .collect(),
        outcome.subjects,
    )
}

fn tools_list(input_schema: &Value) -> Vec<(&'static str, Value)> {
    vec![
        (
            "client-to-server",
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        ),
        (
            "server-to-client",
            json!({"jsonrpc": "2.0", "id": 1, "result": {"tools": [
            {"name": "t", "inputSchema": input_schema, "outputSchema": {"type": "object"}}]}}),
        ),
    ]
}

#[test]
fn a_valid_schema_is_a_subject_and_passes() {
    let (found, subjects) = findings(&tools_list(&json!({"type": "object",
        "properties": {"q": {"type": "string", "minLength": 1}}, "required": ["q"]})));
    assert_eq!(found, Vec::<String>::new());
    assert_eq!(subjects, 2);
}

#[test]
fn an_invalid_schema_names_the_tool_member_and_keyword() {
    let (found, _) = findings(&tools_list(&json!({"type": "object",
        "properties": {"q": {"type": "strng"}}, "required": true})));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with(
            "the server sent tool \"t\"'s inputSchema, which is not a valid schema in the default dialect, 2020-12: "
        ),
        "{found:?}"
    );
    assert!(found[0].contains("/required"), "{found:?}");
    assert!(found[0].contains("/properties/q/type"), "{found:?}");
}

/// A declared dialect is honoured: `"exclusiveMinimum": true` is draft-04's
/// boolean form, valid there and invalid in 2020-12.
#[test]
fn the_declared_dialect_decides() {
    let draft4 = json!({"$schema": "http://json-schema.org/draft-04/schema#",
        "type": "number", "minimum": 0, "exclusiveMinimum": true});
    assert_eq!(findings(&tools_list(&draft4)).0, Vec::<String>::new());
    let default = json!({"type": "number", "minimum": 0, "exclusiveMinimum": true});
    assert_eq!(findings(&tools_list(&default)).0.len(), 1);
}

#[test]
fn an_unknown_dialect_is_not_examined() {
    let custom = json!({"$schema": "https://example.test/dialect", "type": 7});
    let (found, subjects) = findings(&tools_list(&custom));
    assert_eq!(found, Vec::<String>::new());
    assert_eq!(subjects, 1, "only the outputSchema");
}

#[test]
fn elicitation_schemas_are_examined_at_both_revisions() {
    let bad = json!({"type": "object", "properties": {"age": {"type": "integr"}}});
    let request = [(
        "server-to-client",
        json!({"jsonrpc": "2.0", "id": 5,
        "method": "elicitation/create", "params": {"mode": "form", "message": "?",
        "requestedSchema": bad}}),
    )];
    let (found, _) = findings(&request);
    assert!(
        found[0].contains("the elicitation's requestedSchema"),
        "{found:?}"
    );
    let input_required = [(
        "server-to-client",
        json!({"jsonrpc": "2.0", "id": 5,
        "result": {"resultType": "input_required", "inputRequests": {"who": {
            "method": "elicitation/create", "params": {"mode": "form", "message": "?",
            "requestedSchema": bad}}}}}),
    )];
    let (found, _) = findings(&input_required);
    assert!(
        found[0].contains("input request \"who\"'s requestedSchema"),
        "{found:?}"
    );
}
