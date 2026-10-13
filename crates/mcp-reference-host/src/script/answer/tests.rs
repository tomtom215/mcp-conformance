// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Each case is judged twice: by what the rule promises, and by the
//! validator's own `elicitation.response-matches-schema` check (ELIC-095),
//! run over a one-round `2026-07-28` trace carrying the answer — the same
//! engine that judges the committed captures, so "validates against the
//! requested schema" means what the corpus means by it. The engine is a
//! dev-dependency only; the host's runtime carries no schema engine.

use mcp_conformance_core::requirement::RegistrySet;
use mcp_conformance_core::revision::REVISION_2026_07_28;
use mcp_trace_validator::report::Outcome;
use serde_json::{Value, json};

use super::*;

fn schema(value: &Value) -> ElicitationSchema {
    serde_json::from_value(value.clone()).unwrap()
}

/// ELIC-095's outcome for an `accept` of `requested` carrying `content`.
fn judged(requested: &Value, content: &Map<String, Value>) -> Outcome {
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": { "elicitation": { "form": {} } },
        "io.modelcontextprotocol/clientInfo": { "name": "test", "version": "0" }
    });
    let server = json!({ "io.modelcontextprotocol/serverInfo": { "name": "s", "version": "0" } });
    let messages = [
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
               "params":{"name":"ask","arguments":{},"_meta":meta}}),
        json!({"jsonrpc":"2.0","id":1,"result":{"_meta":server,"resultType":"input_required",
               "inputRequests":{"form":{"method":"elicitation/create","params":{
                   "mode":"form","message":"m","requestedSchema":requested}}},
               "requestState":"1"}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
               "params":{"name":"ask","arguments":{},"requestState":"1",
                         "inputResponses":{"form":{"action":"accept","content":content}},
                         "_meta":meta}}),
        json!({"jsonrpc":"2.0","id":2,"result":{"_meta":server,"resultType":"complete",
               "content":[]}}),
    ];
    let trace = messages
        .iter()
        .enumerate()
        .map(|(seq, payload)| {
            let direction = if seq % 2 == 0 {
                "client-to-server"
            } else {
                "server-to-client"
            };
            json!({"seq":seq,"direction":direction,"transport":"stdio",
                   "kind":"message","payload":payload})
            .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let events = mcp_trace_validator::reader::parse_trace(
        &trace,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .unwrap();
    let registry = RegistrySet::builtin()
        .unwrap()
        .registry(REVISION_2026_07_28)
        .unwrap();
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    report
        .requirements
        .iter()
        .find(|row| row.id == "ELIC-095")
        .unwrap()
        .outcome
}

/// The accepted content for `requested`, which must also pass ELIC-095.
fn accepted_and_valid(requested: &Value) -> Map<String, Value> {
    let content = accepted_content(&schema(requested))
        .unwrap_or_else(|| panic!("declined a satisfiable form: {requested}"));
    assert_eq!(
        judged(requested, &content),
        Outcome::Pass,
        "{content:?} against {requested}"
    );
    content
}

#[test]
fn the_judge_is_live() {
    // The control: the answer this host used to give the probe form warns.
    // Without it every `Pass` below could be the check abstaining.
    let probe = json!({
        "type": "object",
        "properties": { "username": { "type": "string" } },
        "required": ["username"]
    });
    assert_eq!(judged(&probe, &Map::new()), Outcome::Warn);
}

#[test]
fn the_servers_probe_form_is_filled_by_type_and_format() {
    // The everything server's `test_elicitation` form, as captured.
    let content = accepted_and_valid(&json!({
        "type": "object",
        "properties": {
            "username": { "type": "string", "description": "User's response" },
            "email": { "type": "string", "format": "email", "description": "User's email address" }
        },
        "required": ["username", "email"]
    }));
    assert_eq!(content["username"], "scripted");
    assert_eq!(content["email"], "user@example.com");
}

#[test]
fn a_required_default_is_used_rather_than_a_placeholder() {
    let content = accepted_and_valid(&json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "default": "John Doe" },
            "age": { "type": "integer", "default": 30 }
        },
        "required": ["name", "age"]
    }));
    assert_eq!(content["name"], "John Doe");
    assert_eq!(content["age"], 30);
}

#[test]
fn optional_properties_without_a_default_stay_omitted() {
    let content = accepted_and_valid(&json!({
        "type": "object",
        "properties": {
            "needed": { "type": "boolean" },
            "optional": { "type": "string" }
        },
        "required": ["needed"]
    }));
    assert_eq!(content.len(), 1, "{content:?}");
    assert_eq!(content["needed"], false);
}

#[test]
fn string_lengths_pad_and_cut_the_placeholder() {
    let long = accepted_and_valid(&json!({
        "type": "object",
        "properties": { "s": { "type": "string", "minLength": 12 } },
        "required": ["s"]
    }));
    assert_eq!(long["s"], "scriptedxxxx");
    let short = accepted_and_valid(&json!({
        "type": "object",
        "properties": { "s": { "type": "string", "maxLength": 3 } },
        "required": ["s"]
    }));
    assert_eq!(short["s"], "scr");
}

#[test]
fn every_format_has_its_own_placeholder() {
    let content = accepted_and_valid(&json!({
        "type": "object",
        "properties": {
            "u": { "type": "string", "format": "uri" },
            "d": { "type": "string", "format": "date" },
            "t": { "type": "string", "format": "date-time" }
        },
        "required": ["u", "d", "t"]
    }));
    assert_eq!(content["u"], "https://example.com/");
    assert_eq!(content["d"], "1970-01-01");
    assert_eq!(content["t"], "1970-01-01T00:00:00Z");
}

#[test]
fn numbers_take_zero_or_the_nearer_bound() {
    let content = accepted_and_valid(&json!({
        "type": "object",
        "properties": {
            "free": { "type": "integer" },
            "above": { "type": "integer", "minimum": 5, "maximum": 9 },
            "below": { "type": "number", "maximum": -1.5 },
            "around": { "type": "number", "minimum": -1, "maximum": 1 }
        },
        "required": ["free", "above", "below", "around"]
    }));
    assert_eq!(content["free"], 0);
    assert_eq!(content["above"], 5);
    assert_eq!(content["below"], -1.5);
    assert_eq!(content["around"], 0.0);
}

#[test]
fn enums_take_their_first_values() {
    let content = accepted_and_valid(&json!({
        "type": "object",
        "properties": {
            "plain": { "type": "string", "enum": ["b", "a"] },
            "titled": { "type": "string", "oneOf": [
                { "const": "x", "title": "X" }, { "const": "y", "title": "Y" }
            ] },
            "many": { "type": "array", "minItems": 2,
                      "items": { "type": "string", "enum": ["p", "q", "r"] } },
            "any": { "type": "array",
                     "items": { "anyOf": [{ "const": "m", "title": "M" }] } }
        },
        "required": ["plain", "titled", "many", "any"]
    }));
    assert_eq!(content["plain"], "b");
    assert_eq!(content["titled"], "x");
    assert_eq!(content["many"], json!(["p", "q"]));
    assert_eq!(content["any"], json!([]));
}

#[test]
fn an_unsatisfiable_required_property_declines() {
    for requested in [
        // An enum with no values to choose from.
        json!({ "type": "object", "required": ["e"],
                "properties": { "e": { "type": "string", "enum": [] } } }),
        // Bounds that exclude each other, for each type that has them.
        json!({ "type": "object", "required": ["s"],
                "properties": { "s": { "type": "string", "minLength": 4, "maxLength": 2 } } }),
        json!({ "type": "object", "required": ["i"],
                "properties": { "i": { "type": "integer", "minimum": 3, "maximum": 1 } } }),
        json!({ "type": "object", "required": ["n"],
                "properties": { "n": { "type": "number", "minimum": 3.5, "maximum": 1 } } }),
        // More selections demanded than there are choices.
        json!({ "type": "object", "required": ["m"],
                "properties": { "m": { "type": "array", "minItems": 3,
                                       "items": { "type": "string", "enum": ["a"] } } } }),
        // A formatted placeholder that cannot be shortened without breaking it.
        json!({ "type": "object", "required": ["e"],
                "properties": { "e": { "type": "string", "format": "email", "maxLength": 5 } } }),
        // A required name the schema never defines.
        json!({ "type": "object", "required": ["ghost"], "properties": {} }),
    ] {
        assert_eq!(
            accepted_content(&schema(&requested)),
            None,
            "must decline: {requested}"
        );
    }
}
