// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the three feature-page clauses with no `2025-11-25` counterpart.
//!
//! Each has a qualifier that decides whether it fires at all — "when the
//! underlying set has not changed", "mirrored into a header", "for a
//! non-existent resource" — so each is pinned on the case where the qualifier
//! is *not* met as well as the case where it is.

use crate::checks::stateless::testkit::{client, findings_for, server, trace};

const ORDER: &str = "tools.deterministic-order";
const RANGE: &str = "tools.x-mcp-header-integer-range";
const CONTENTS: &str = "resources.read-contents-non-empty";

/// A `tools/list` exchange returning `names` in order.
fn listed(seq: u64, id: u64, names: &[&str]) -> Vec<String> {
    let tools = names
        .iter()
        .map(|name| format!(r#"{{"name":"{name}","inputSchema":{{"type":"object"}}}}"#))
        .collect::<Vec<_>>()
        .join(",");
    vec![
        client(
            seq,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{"_meta":{{}}}}}}"#
            ),
        ),
        server(
            seq + 1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete","ttlMs":0,"tools":[{tools}]}}}}"#
            ),
        ),
    ]
}

#[test]
fn the_same_set_in_a_different_order_is_reported() {
    let mut lines = listed(0, 1, &["alpha", "beta"]);
    lines.extend(listed(2, 2, &["beta", "alpha"]));
    let findings = findings_for(ORDER, &trace(&lines));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("seq 1"), "{findings:?}");
}

#[test]
fn a_changed_set_may_be_ordered_freshly() {
    // The clause qualifies itself: it binds only while the set is unchanged.
    let mut lines = listed(0, 1, &["alpha", "beta"]);
    lines.extend(listed(2, 2, &["beta", "gamma"]));
    assert!(findings_for(ORDER, &trace(&lines)).is_empty());

    let mut same = listed(0, 1, &["alpha", "beta"]);
    same.extend(listed(2, 2, &["alpha", "beta"]));
    assert!(findings_for(ORDER, &trace(&same)).is_empty());
}

#[test]
fn one_listing_alone_cannot_be_inconsistent() {
    assert!(findings_for(ORDER, &trace(&listed(0, 1, &["alpha"]))).is_empty());
}

/// A `tools/list` annotating `region` with `x-mcp-header`, then a call with `value`.
fn annotated_call(value: &str) -> String {
    trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{}}}"#,
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","ttlMs":0,"tools":[{"name":"t","inputSchema":{"type":"object","properties":{"region":{"type":"integer","x-mcp-header":"Mcp-Param-Region"},"other":{"type":"integer"}}}}]}}"#,
        ),
        client(
            2,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"t","arguments":{{"region":{value},"other":9007199254740992}},"_meta":{{}}}}}}"#
            ),
        ),
    ])
}

#[test]
fn a_mirrored_integer_beyond_the_safe_range_is_reported() {
    for value in ["9007199254740992", "-9007199254740992"] {
        let findings = findings_for(RANGE, &annotated_call(value));
        assert_eq!(findings.len(), 1, "value {value}: {findings:?}");
        assert!(findings[0].contains("Mcp-Param-Region"), "{findings:?}");
    }
}

#[test]
fn the_boundary_values_are_inside_the_range() {
    for value in ["9007199254740991", "-9007199254740991", "0"] {
        assert!(
            findings_for(RANGE, &annotated_call(value)).is_empty(),
            "value {value}"
        );
    }
}

#[test]
fn an_unannotated_argument_is_the_tools_own_business() {
    // `other` in the fixture is deliberately out of range and unannotated: the
    // clause is about what has to survive a trip through a header.
    let findings = findings_for(RANGE, &annotated_call("1"));
    assert!(findings.is_empty(), "{findings:?}");
}

/// A `resources/read` answering with `contents`.
fn read(contents: &str) -> String {
    trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"file:///a","_meta":{}}}"#,
        ),
        server(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","ttlMs":0,"contents":{contents}}}}}"#
            ),
        ),
    ])
}

/// `read(contents)` preceded by an earlier read of the same URI that the
/// server answered with `code` — the server's own word on whether it exists.
fn read_after_error(code: i64, contents: &str) -> String {
    let mut lines = vec![
        client(
            0,
            r#"{"jsonrpc":"2.0","id":7,"method":"resources/read","params":{"uri":"file:///a","_meta":{}}}"#,
        ),
        server(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":7,"error":{{"code":{code},"message":"no such resource"}}}}"#
            ),
        ),
    ];
    lines.extend(read(contents).lines().enumerate().map(|(index, line)| {
        line.replacen(
            &format!(r#""seq":{index}"#),
            &format!(r#""seq":{}"#, index + 2),
            1,
        )
    }));
    trace(&lines)
}

#[test]
fn an_empty_contents_array_for_a_missing_resource_is_reported() {
    for code in [-32602, -32002] {
        let findings = findings_for(CONTENTS, &read_after_error(code, "[]"));
        assert_eq!(findings.len(), 1, "{code}: {findings:?}");
        assert!(findings[0].contains("not found"), "{findings:?}");
    }
    // Some other error says nothing about existence.
    assert!(findings_for(CONTENTS, &read_after_error(-32603, "[]")).is_empty());
}

#[test]
fn an_empty_contents_array_alone_is_not_evidence_of_a_missing_resource() {
    // "It could mean the resource exists but has no content": the clause binds
    // only a non-existent resource, and nothing here says this one is.
    assert!(findings_for(CONTENTS, &read("[]")).is_empty());
}

#[test]
fn an_empty_resource_still_carries_one_entry() {
    // The conforming shape for a resource with no bytes: one entry whose text is
    // empty, not an absent entry.
    let session = read(r#"[{"uri":"file:///a","mimeType":"text/plain","text":""}]"#);
    assert!(findings_for(CONTENTS, &session).is_empty());
}

#[test]
fn a_read_that_errored_has_no_contents_to_judge() {
    let session = trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{"uri":"file:///a","_meta":{}}}"#,
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"no such resource"}}"#,
        ),
    ]);
    assert!(findings_for(CONTENTS, &session).is_empty());
}
