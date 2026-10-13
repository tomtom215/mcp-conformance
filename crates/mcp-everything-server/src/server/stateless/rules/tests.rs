// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The two rules, at the boundary each of them draws.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use serde_json::json;

/// A request for `method` whose `params` are `params`.
fn request(method: &str, params: &Value) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params})
}

#[test]
fn every_rfc_5424_level_is_accepted_and_nothing_else_is() {
    // The eight the specification lists, verbatim. A level this rejected would
    // be a conforming client refused, which is worse than the defect the rule
    // exists to fix.
    for level in [
        "debug",
        "info",
        "notice",
        "warning",
        "error",
        "critical",
        "alert",
        "emergency",
    ] {
        let payload = request("tools/call", &json!({"_meta": {LOG_LEVEL: level}}));
        assert!(fault(&payload).is_none(), "{level} must be accepted");
    }
    // Anything else is the malformed request. `"Debug"` is included because
    // the levels are lowercase on the wire and a case-insensitive decoder
    // would silently widen the set.
    for level in [json!("chatty"), json!("Debug"), json!(3), json!(null)] {
        let payload = request("tools/call", &json!({"_meta": {LOG_LEVEL: level}}));
        let fault = fault(&payload).unwrap_or_else(|| panic!("{level} must be rejected"));
        assert_eq!(fault.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(fault.message.contains("RFC 5424"), "{}", fault.message);
    }
    // Absent is not malformed: it is the request asking for no logs, which the
    // revision requires the server to honour by staying silent.
    assert!(fault(&request("tools/call", &json!({"_meta": {}}))).is_none());
    assert!(fault(&request("tools/call", &json!({}))).is_none());
}

#[test]
fn a_cursor_is_refused_on_the_operations_that_paginate() {
    for method in [
        "tools/list",
        "prompts/list",
        "resources/list",
        "resources/templates/list",
    ] {
        for cursor in [json!("fabricated"), json!(2), json!(null)] {
            let payload = request(method, &json!({ "cursor": cursor }));
            let fault = fault(&payload).unwrap_or_else(|| panic!("{method} must reject {cursor}"));
            assert_eq!(fault.code, rmcp::model::ErrorCode::INVALID_PARAMS);
            assert!(
                fault.message.contains("not issued") || fault.message.contains("never issued"),
                "{}",
                fault.message
            );
        }
        // Without one, the same request is fine: it asks for the first page.
        assert!(fault_of(method, &json!({})).is_none());
    }
}

#[test]
fn a_cursor_this_server_issued_is_admitted() {
    // Every listing has a second page, and its cursor must pass the envelope —
    // otherwise the transport would refuse the very pagination the handler
    // offers.
    for listing in Listing::ALL {
        let cursor = listing.cursor(1).unwrap();
        assert!(fault_of(listing.method(), &json!({ "cursor": cursor })).is_none());
        // And it is valid only where it was issued.
        let elsewhere = Listing::ALL
            .into_iter()
            .find(|other| *other != listing)
            .unwrap();
        assert!(fault_of(elsewhere.method(), &json!({ "cursor": cursor })).is_some());
    }
}

#[test]
fn a_cursor_elsewhere_is_an_argument_and_not_a_token() {
    // A `tools/call` whose arguments happen to include the word is not
    // paginating anything; rejecting it would be this server inventing a rule.
    assert!(fault_of("tools/call", &json!({"cursor": "x"})).is_none());
    assert!(fault_of("resources/read", &json!({"cursor": "x"})).is_none());
}

#[test]
fn what_the_rule_cannot_read_is_not_its_finding() {
    // Malformed at a level the transport rejects first. Answering here too
    // would put two rejections in a race for the same request.
    assert!(fault(&json!({"jsonrpc": "2.0", "id": 1})).is_none());
    assert!(fault(&json!({"method": 7})).is_none());
    assert!(fault(&json!("not an object")).is_none());
    // Params that are not an object read as no params, not as a fault.
    assert!(fault(&request("tools/list", &json!("scalar"))).is_none());
}

/// The fault of a request for `method` with `params`.
fn fault_of(method: &str, params: &Value) -> Option<McpError> {
    fault(&request(method, params))
}
