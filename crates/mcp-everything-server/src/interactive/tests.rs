// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Unit tests for the interactive tools' pure parts: the schema builders and
//! the error they raise when one fails.
//!
//! The interactions themselves are round trips, so they are tested where a
//! round trip exists — `tests/agent_loop.rs` against a live host for the
//! `2025-11-25` form, and `tests/stateless_stdio.rs` on the wire for MRTR.

use super::*;

#[test]
fn invalid_schema_carries_the_failure_payload() {
    let error = invalid_schema("boom");
    assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    assert_eq!(error.message, "elicitation schema construction failed");
    assert_eq!(error.data, Some(serde_json::json!({ "message": "boom" })));
}

/// The true wire shape, asserted at serialization: the duplex round-trip
/// cannot check `enumNames` because rmcp's client-side untagged
/// `EnumSchema` deserialization matches the legacy form as `Untitled`
/// first and silently drops the field (upstream-filing candidate).
#[test]
fn sep1330_serializes_all_five_variants_to_the_wire() {
    let schema = serde_json::to_value(sep1330_schema()).unwrap();
    let props = &schema["properties"];
    assert_eq!(props["untitledSingle"]["type"], "string");
    assert_eq!(
        props["untitledSingle"]["enum"],
        serde_json::json!(["option1", "option2", "option3"])
    );
    assert_eq!(
        props["titledSingle"]["oneOf"][0],
        serde_json::json!({"const": "value1", "title": "First Option"})
    );
    assert_eq!(
        props["legacyEnum"]["enum"],
        serde_json::json!(["opt1", "opt2", "opt3"])
    );
    assert_eq!(
        props["legacyEnum"]["enumNames"],
        serde_json::json!(["Option One", "Option Two", "Option Three"])
    );
    assert_eq!(props["untitledMulti"]["type"], "array");
    assert_eq!(
        props["untitledMulti"]["items"]["enum"],
        serde_json::json!(["option1", "option2", "option3"])
    );
    assert_eq!(props["titledMulti"]["type"], "array");
    assert_eq!(
        props["titledMulti"]["items"]["anyOf"][0],
        serde_json::json!({"const": "value1", "title": "First Choice"})
    );
}
