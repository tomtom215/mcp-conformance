// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for `PAGE-011`, the invalid-cursor rejection.
//!
//! The check treats "never issued in this session" as the witness for "invalid",
//! so the two cases that keep it honest are a cursor issued *earlier* (valid, not
//! reported) and one issued for a different method (not this method's cursor).

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const CHECK: &str = "pagination.invalid-cursor-rejected";

/// A list request for `method`, presenting `cursor` when given.
fn list(seq: u64, id: u64, method: &str, cursor: Option<&str>) -> String {
    let params = cursor.map_or_else(String::new, |cursor| format!(r#","cursor":"{cursor}""#));
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"_meta":{{}}{params}}}}}"#
        ),
    )
}

/// A page result, issuing `next` when given.
fn page(seq: u64, id: u64, next: Option<&str>) -> String {
    let cursor = next.map_or_else(String::new, |next| format!(r#","nextCursor":"{next}""#));
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete","ttlMs":0{cursor}}}}}"#
        ),
    )
}

#[test]
fn an_unissued_cursor_answered_with_a_result_is_reported() {
    let session = trace(&[list(0, 1, "tools/list", Some("made-up")), page(1, 1, None)]);
    let findings = findings_for(CHECK, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("made-up"), "{findings:?}");
}

#[test]
fn rejecting_it_with_invalid_params_conforms() {
    let session = trace(&[
        list(0, 1, "tools/list", Some("made-up")),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"invalid cursor"}}"#,
        ),
    ]);
    assert!(findings_for(CHECK, &session).is_empty());
}

#[test]
fn some_other_error_is_not_the_required_rejection() {
    let session = trace(&[
        list(0, 1, "tools/list", Some("made-up")),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"boom"}}"#,
        ),
    ]);
    let findings = findings_for(CHECK, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("-32603"), "{findings:?}");
}

#[test]
fn a_cursor_the_server_issued_is_valid() {
    let session = trace(&[
        list(0, 1, "tools/list", None),
        page(1, 1, Some("page2")),
        list(2, 2, "tools/list", Some("page2")),
        page(3, 2, None),
    ]);
    assert!(findings_for(CHECK, &session).is_empty());
}

#[test]
fn a_cursor_issued_for_another_method_is_not_this_ones() {
    let session = trace(&[
        list(0, 1, "prompts/list", None),
        page(1, 1, Some("page2")),
        list(2, 2, "tools/list", Some("page2")),
        page(3, 2, None),
    ]);
    assert_eq!(findings_for(CHECK, &session).len(), 1);
}

#[test]
fn a_cursor_used_before_it_was_issued_is_still_unissued() {
    // Order matters: the server cannot be excused by a cursor it handed out
    // afterwards.
    let session = trace(&[
        list(0, 1, "tools/list", Some("page2")),
        page(1, 1, Some("page2")),
    ]);
    assert_eq!(findings_for(CHECK, &session).len(), 1);
}

#[test]
fn a_list_request_with_no_cursor_is_not_judged() {
    let session = trace(&[list(0, 1, "tools/list", None), page(1, 1, None)]);
    assert!(findings_for(CHECK, &session).is_empty());
}
