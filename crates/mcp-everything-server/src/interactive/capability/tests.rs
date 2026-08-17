// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the capability gate.
//!
//! The refusal is tested directly rather than through a tool call: building a
//! `RequestContext` needs a live peer, and what these pin is the *shape of the
//! error*, which is what a trace is judged on. The read path — that the
//! declaration comes from the request rather than from a handshake — is pinned
//! on the wire instead, by `tests/stateless_stdio.rs`.

use super::*;

/// The `data.requiredCapabilities` of a refusal, as JSON.
fn required_capabilities(revision: ServedRevision, required: Required) -> serde_json::Value {
    let error = refusal(revision, required);
    error
        .data
        .and_then(|data| data.get("requiredCapabilities").cloned())
        .unwrap_or(serde_json::Value::Null)
}

#[test]
fn the_legacy_refusal_is_unchanged() {
    // The suite's scenarios expect this shape at `2025-11-25`, and `-32021`
    // does not exist there.
    let error = refusal(ServedRevision::V2025_11_25, Required::Sampling);
    assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_REQUEST);
    assert!(error.message.contains("sampling"), "{}", error.message);
    assert!(error.data.is_none());
}

#[test]
fn the_stateless_refusal_names_the_capability_in_the_schema_shape() {
    // BASE-035, and the shape the `2026-07-28` schema types the field as: a
    // `ClientCapabilities` object, not a list of names.
    let error = refusal(ServedRevision::V2026_07_28, Required::Elicitation);
    assert_eq!(
        error.code,
        rmcp::model::ErrorCode::MISSING_REQUIRED_CLIENT_CAPABILITY
    );
    assert_eq!(
        required_capabilities(ServedRevision::V2026_07_28, Required::Elicitation),
        serde_json::json!({ "elicitation": {} })
    );
    assert_eq!(
        required_capabilities(ServedRevision::V2026_07_28, Required::Sampling),
        serde_json::json!({ "sampling": {} })
    );
}

#[test]
fn a_refusal_names_only_the_capability_it_is_about() {
    // An object carrying every capability would be true but useless: the
    // client cannot tell which one this call needed.
    for (required, other) in [
        (Required::Sampling, "elicitation"),
        (Required::Elicitation, "sampling"),
    ] {
        let named = required_capabilities(ServedRevision::V2026_07_28, required);
        assert!(named.get(required.name()).is_some(), "{named}");
        assert!(named.get(other).is_none(), "{named}");
    }
}

#[test]
fn a_declaration_is_recognized_only_for_the_capability_it_declares() {
    let mut sampling_only = ClientCapabilities::default();
    sampling_only.sampling = Some(SamplingCapability::default());
    assert!(Required::Sampling.declared_in(&sampling_only));
    assert!(!Required::Elicitation.declared_in(&sampling_only));
    assert!(!Required::Sampling.declared_in(&ClientCapabilities::default()));
}
