// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The W3C `traceparent` grammar: four fixed-width fields with two all-zero
//! prohibitions, and a corpus trace can only ever break one of them. Each
//! field is pinned here at both its length and its alphabet.

use super::validate_traceparent;
use crate::checks::draft::testkit::{META, client, findings_for, post, server, trace};
use serde_json::json;

/// The W3C Trace Context specification's own example value.
const VALID: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

#[test]
fn the_traceparent_grammar_is_pinned_field_by_field() {
    assert!(validate_traceparent(&json!(VALID)).is_ok());

    let rejected = [
        // Wrong field count, either way.
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
        // Version: two lowercase hex digits.
        "0-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "0g-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "0A-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        // Trace id: 32 lowercase hex digits, never all zero.
        "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",
        "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e473z-00f067aa0ba902b7-01",
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
        // Parent id: 16 lowercase hex digits, never all zero.
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00F067AA0BA902B7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
        // Flags: two lowercase hex digits.
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-1",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-0g",
    ];
    for value in rejected {
        assert!(
            validate_traceparent(&json!(value)).is_err(),
            "{value:?} should be rejected"
        );
    }

    // A non-string is reported as such rather than parsed.
    assert!(validate_traceparent(&json!(42)).is_err());
    assert!(validate_traceparent(&json!(null)).is_err());
}

#[test]
fn trace_context_is_read_from_both_envelopes_and_only_when_present() {
    let check = "meta.trace-context-format";

    // A malformed `traceparent` in request params.
    let request_side = trace(&[
        post(0, r#"{"mcp-method":"ping"}"#),
        client(
            1,
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"traceparent":"nope"}}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &request_side).len(), 1);

    // And in a result envelope, which is the other half of the clause.
    let result_side = trace(&[
        post(0, r#"{"mcp-method":"ping"}"#),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
        server(
            2,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","_meta":{"traceparent":"nope"}}}"#,
        ),
    ]);
    assert_eq!(findings_for(check, &result_side).len(), 1);

    // A valid one, and an envelope carrying none at all, are both silent.
    let valid = request_side.replace("nope", VALID);
    assert!(findings_for(check, &valid).is_empty());
    let absent = trace(&[
        post(0, r#"{"mcp-method":"ping"}"#),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#),
        ),
    ]);
    assert!(findings_for(check, &absent).is_empty());

    // `tracestate` and `baggage` are vendor-defined lists: only their gross
    // shape is judged, so a string passes and a non-string does not.
    let bad_state = request_side.replace(r#""traceparent":"nope""#, r#""tracestate":42"#);
    assert_eq!(findings_for(check, &bad_state).len(), 1);
    let good_state = request_side.replace(r#""traceparent":"nope""#, r#""baggage":"k=v""#);
    assert!(findings_for(check, &good_state).is_empty());
}
