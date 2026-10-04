// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Unit proofs for the loop's pure parts: the `_meta` a call carries, how an
//! outcome is judged, and schema-derived argument synthesis.

#![allow(clippy::unwrap_used)]

use super::*;

fn plan(log_level: Option<LoggingLevel>, trace_parent: Option<&str>) -> RunPlan {
    RunPlan {
        turn_limit: Some(1),
        error_budget: 0,
        calls: CallPolicy::EachDiscoveredToolOnce,
        log_level,
        trace_parent: trace_parent.map(str::to_owned),
    }
}

/// Each field alone must still produce a `_meta`, which is the arm the
/// capture cannot exercise: it sets both, so an `and` here would behave
/// identically on every recording and on every test that drives one.
#[test]
fn either_field_alone_is_enough_to_carry_a_meta() {
    assert!(call_meta(&plan(None, None)).is_none(), "neither asked for");

    let logging = call_meta(&plan(Some(LoggingLevel::Debug), None)).unwrap();
    assert!(logging.get_traceparent().is_none(), "{logging:?}");
    assert!(
        serde_json::to_string(&logging)
            .unwrap()
            .contains("logLevel"),
        "{logging:?}"
    );

    let traced = call_meta(&plan(
        None,
        Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
    ))
    .unwrap();
    assert_eq!(
        traced.get_traceparent(),
        Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01")
    );
    assert!(
        !serde_json::to_string(&traced).unwrap().contains("logLevel"),
        "{traced:?}"
    );

    let both = call_meta(&plan(
        Some(LoggingLevel::Debug),
        Some("00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01"),
    ))
    .unwrap();
    assert!(both.get_traceparent().is_some(), "{both:?}");
    assert!(
        serde_json::to_string(&both).unwrap().contains("logLevel"),
        "{both:?}"
    );
}

fn text_result(is_error: bool) -> rmcp::model::CallToolResult {
    let content = vec![rmcp::model::ContentBlock::text("said so")];
    if is_error {
        rmcp::model::CallToolResult::error(content)
    } else {
        rmcp::model::CallToolResult::success(content)
    }
}

/// The four ways a call can end, for an ordinary tool and for one documented
/// to fail. Only one cell is excused — the documented error result — and the
/// rest are exactly as countable as they were before the expectation existed.
#[test]
fn only_the_documented_error_result_is_an_expected_failure() {
    // An ordinary tool: success is fine, both error shapes are errors.
    assert_eq!(
        judge_outcome(Ok(text_result(false)), false),
        (Ok("said so".to_owned()), false)
    );
    assert_eq!(
        judge_outcome(Ok(text_result(true)), false),
        (Err("tool error: said so".to_owned()), false)
    );

    // A tool documented to fail: its error result is the expected answer...
    assert_eq!(
        judge_outcome(Ok(text_result(true)), true),
        (Err("tool error: said so".to_owned()), true)
    );
    // ...a success breaks the documentation and is an error, not a pass...
    let (succeeded, expected) = judge_outcome(Ok(text_result(false)), true);
    assert!(!expected);
    assert!(
        succeeded.unwrap_err().contains("documented to fail"),
        "the line says why a success is wrong here"
    );
    // ...and a protocol error is not the in-band result it promised.
    let protocol = Err(rmcp::ServiceError::McpError(
        rmcp::model::ErrorData::invalid_params("nope", None),
    ));
    let (failed, expected) = judge_outcome(protocol, true);
    assert!(failed.is_err());
    assert!(
        !expected,
        "a protocol error is never the documented failure"
    );
}

#[test]
fn the_suite_error_tool_is_recognised_by_exact_name_only() {
    assert!(fails_by_design("test_error_handling"));
    assert!(!fails_by_design("test_error_handling_v2"));
    assert!(!fails_by_design("Test_Error_Handling"));
    assert!(!fails_by_design("echo"));
}

#[test]
fn synthesized_arguments_cover_required_properties_only() {
    let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "a": { "type": "number" },
            "b": { "type": "number" },
            "note": { "type": "string" }
        },
        "required": ["a", "b"]
    }))
    .unwrap();
    let arguments = synthesize_arguments(&schema);
    assert_eq!(arguments.len(), 2, "{arguments:?}");
    assert_eq!(arguments["a"], 7);
    assert_eq!(arguments["b"], 7);
}

#[test]
fn synthesized_arguments_respect_types_and_enums() {
    let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": {
            "city": { "type": "string", "enum": ["New York", "Chicago"] },
            "flag": { "type": "boolean" },
            "items": { "type": "array" },
            "config": { "type": "object" },
            "message": { "type": "string" }
        },
        "required": ["city", "flag", "items", "config", "message"]
    }))
    .unwrap();
    let arguments = synthesize_arguments(&schema);
    assert_eq!(arguments["city"], "New York", "first enum value wins");
    assert_eq!(arguments["flag"], true);
    assert_eq!(arguments["items"], serde_json::json!([]));
    assert_eq!(
        arguments["config"],
        serde_json::json!({}),
        "object-typed requirements get an empty object, not a string"
    );
    assert_eq!(arguments["message"], "probe");
}

#[test]
fn schemars_ref_enums_resolve_to_their_first_const() {
    // The exact shape `#[derive(JsonSchema)]` emits for a Rust enum:
    // the property is a `$ref` into `$defs`, and the definition is a
    // `oneOf` of `const` variants (get-structured-content's Location).
    let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "$defs": {
            "Location": {
                "oneOf": [
                    { "const": "New York", "type": "string" },
                    { "const": "Chicago", "type": "string" }
                ]
            }
        },
        "type": "object",
        "properties": { "location": { "$ref": "#/$defs/Location" } },
        "required": ["location"]
    }))
    .unwrap();
    assert_eq!(synthesize_arguments(&schema)["location"], "New York");
}

#[test]
fn unresolvable_and_cyclic_refs_degrade_to_the_string_probe() {
    // A dangling ref and a two-node cycle: the resolver must stay
    // bounded and total, never loop or panic.
    let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "$defs": {
            "A": { "$ref": "#/$defs/B" },
            "B": { "$ref": "#/$defs/A" }
        },
        "type": "object",
        "properties": {
            "dangling": { "$ref": "#/$defs/Missing" },
            "cyclic": { "$ref": "#/$defs/A" }
        },
        "required": ["dangling", "cyclic"]
    }))
    .unwrap();
    let arguments = synthesize_arguments(&schema);
    assert_eq!(arguments["dangling"], "probe");
    assert_eq!(arguments["cyclic"], "probe");
}

#[test]
fn no_required_block_synthesizes_the_empty_call() {
    let schema: Map<String, Value> = serde_json::from_value(serde_json::json!({
        "type": "object",
        "properties": { "opt": { "type": "string" } }
    }))
    .unwrap();
    assert!(synthesize_arguments(&schema).is_empty());
}
