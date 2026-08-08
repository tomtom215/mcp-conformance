// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the transport area's shared vocabulary: which events count as a
//! POST, what a header value may carry plainly, and how a body value is
//! rendered into a header and compared back.
//!
//! These are pure functions every clause in the area leans on, so an error here
//! would be invisible in one check and wrong in all of them.

use serde_json::json;

use super::{Match, compare, header_safe, header_text, mirrors, posts, sentinel_payload};
use crate::checks::draft::testkit::{META, client, events, post, trace};
use crate::context::TraceContext;

#[test]
fn header_safety_is_visible_ascii_with_no_edge_whitespace() {
    // RFC 9110's field-value set, plus the revision's leading/trailing rule.
    // An interior tab is a legal field-value character; an edge one is not.
    for safe in ["us-west1", "a b", "tabbed\tinside", "", "~", "!"] {
        assert!(header_safe(safe), "{safe:?} should be carried plainly");
    }
    for unsafe_value in [
        " padded",       // leading space
        "padded ",       // trailing space
        "\tpadded",      // leading tab
        "line1\nline2",  // control character
        "caf\u{e9}",     // non-ASCII
        "bell\u{7}here", // C0 control
        "del\u{7f}",     // DEL is outside 0x20..0x7e
    ] {
        assert!(
            !header_safe(unsafe_value),
            "{unsafe_value:?} needs the sentinel"
        );
    }
}

#[test]
fn the_sentinel_is_recognised_only_when_spelled_exactly() {
    assert_eq!(sentinel_payload("=?base64?YWJj?="), Some("YWJj"));
    assert_eq!(sentinel_payload("=?base64??="), Some(""));
    // Wrong case, missing marker, or too short to hold both markers.
    assert_eq!(sentinel_payload("=?BASE64?YWJj?="), None);
    assert_eq!(sentinel_payload("=?base64?YWJj"), None);
    assert_eq!(sentinel_payload("=?base64?="), None);
    assert_eq!(sentinel_payload("plain"), None);
}

#[test]
fn a_value_is_rendered_into_a_header_by_its_json_type() {
    assert_eq!(header_text(&json!("us-west1")), Some("us-west1".to_owned()));
    // Booleans are lowercase, which `true`/`false` already are in Rust.
    assert_eq!(header_text(&json!(true)), Some("true".to_owned()));
    assert_eq!(header_text(&json!(false)), Some("false".to_owned()));
    // Integers of either signedness render as decimal.
    assert_eq!(header_text(&json!(42)), Some("42".to_owned()));
    assert_eq!(header_text(&json!(-7)), Some("-7".to_owned()));
    assert_eq!(
        header_text(&json!(u64::from(u32::MAX) * 8)),
        Some("34359738360".to_owned())
    );
    // Everything else has no defined header form, so comparison abstains.
    assert_eq!(header_text(&json!(1.5)), None);
    assert_eq!(header_text(&json!(null)), None);
    assert_eq!(header_text(&json!(["a"])), None);
    assert_eq!(header_text(&json!({"a": 1})), None);
}

#[test]
fn comparison_decodes_the_sentinel_before_judging() {
    // Plain, and Base64 of the same value, both carry it.
    assert_eq!(compare("us-west1", "us-west1"), Match::Carried);
    assert_eq!(
        compare("=?base64?dXMtd2VzdDE=?=", "us-west1"),
        Match::Carried
    );
    // A sentinel that decodes to something else is a mismatch, not a pass.
    assert_eq!(
        compare("=?base64?dXMtZWFzdDE=?=", "us-west1"),
        Match::Mismatch
    );
    assert_eq!(compare("us-east1", "us-west1"), Match::Mismatch);
    // A body value shaped like the sentinel, repeated verbatim, is TRAN-092's.
    assert_eq!(
        compare("=?base64?literal?=", "=?base64?literal?="),
        Match::UnencodedSentinel
    );
    // The same value properly encoded is carried, not flagged.
    assert_eq!(
        compare("=?base64?PT9iYXNlNjQ/bGl0ZXJhbD89?=", "=?base64?literal?="),
        Match::Carried
    );
}

/// A POST framing `payload`, plus a stdio POST and a server `http` event that
/// must both be ignored.
fn mixed_transports(payload: &str) -> String {
    trace(&[
        r#"{"seq":0,"direction":"server-to-client","transport":"streamable-http","kind":"http","headers":{"content-type":"application/json"}}"#.to_owned(),
        r#"{"seq":1,"direction":"client-to-server","transport":"stdio","kind":"http","headers":{"mcp-method":"ping"}}"#.to_owned(),
        post(2, r#"{"mcp-method":"ping"}"#),
        client(3, payload),
    ])
}

#[test]
fn only_client_posts_over_streamable_http_are_posts() {
    let document = mixed_transports(&format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{{META}}}}}"#
    ));
    let parsed = events(&document);
    let context = TraceContext::new(&parsed);
    let found = posts(&context);

    assert_eq!(found.len(), 1, "the server and stdio events are not POSTs");
    assert_eq!(found[0].seq, 2);
    assert_eq!(found[0].message_seq, 3);
    assert_eq!(found[0].method(), Some("ping"));
}

#[test]
fn a_post_is_a_request_only_with_both_a_method_and_an_id() {
    let cases = [
        (r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#, true),
        // A notification: method, no id. The revision leaves its headers undefined.
        (r#"{"jsonrpc":"2.0","method":"notifications/x"}"#, false),
        // A response: id, no method. TRAN-060 forbids it outright.
        (r#"{"jsonrpc":"2.0","id":1,"result":{}}"#, false),
        // A null id is not an id.
        (r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#, false),
    ];
    for (payload, expected) in cases {
        let document = mixed_transports(payload);
        let parsed = events(&document);
        let context = TraceContext::new(&parsed);
        let found = posts(&context);
        assert_eq!(
            found[0].is_request(),
            expected,
            "{payload} should{} be a request",
            if expected { "" } else { " not" }
        );
    }
}

#[test]
fn mirrors_are_sourced_from_the_body_and_never_invented() {
    let document = trace(&[
        post(0, r#"{"mcp-method":"resources/read"}"#),
        client(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{{"uri":"file:///a.txt",{META}}}}}"#
            ),
        ),
    ]);
    let parsed = events(&document);
    let context = TraceContext::new(&parsed);
    let found = posts(&context);
    let resolved = mirrors(&found[0], &std::collections::BTreeMap::new());

    let named: Vec<(&str, &str)> = resolved
        .iter()
        .map(|mirror| (mirror.header.as_str(), mirror.value.as_str()))
        .collect();
    assert_eq!(
        named,
        [
            ("mcp-method", "resources/read"),
            ("mcp-name", "file:///a.txt"),
        ],
        "resources/read sources Mcp-Name from params.uri"
    );

    // A method outside the table sources no name, and a body missing the source
    // field yields no mirror rather than a phantom requirement.
    let bare = trace(&[
        post(0, r#"{"mcp-method":"tools/call"}"#),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{{META}}}}}"#),
        ),
    ]);
    let parsed = events(&bare);
    let context = TraceContext::new(&parsed);
    let found = posts(&context);
    let resolved = mirrors(&found[0], &std::collections::BTreeMap::new());
    assert_eq!(resolved.len(), 1, "only Mcp-Method: params.name is absent");
    assert!(
        !resolved[0].encodable,
        "Mcp-Method never takes the sentinel"
    );
}
