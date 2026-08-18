// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the caching-hint clauses.
//!
//! The page-scope check follows a cursor chain, so the case that matters is two
//! *independent* list calls: the clause binds the pages of one request, and a
//! check that grouped by method alone would report a server for answering two
//! separate `tools/list` calls with different scopes, which is permitted.

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const HINTS: &str = "caching.hints-on-cacheable-results";
const TTL: &str = "caching.ttl-non-negative";
const SCOPE: &str = "caching.page-scope-consistent";

/// A client request `id` for `method`, with `extra` params.
fn request(seq: u64, id: u64, method: &str, extra: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"_meta":{{}}{extra}}}}}"#
        ),
    )
}

/// A server result for `id` carrying `body` alongside `resultType: complete`.
fn complete(seq: u64, id: u64, body: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete"{body}}}}}"#),
    )
}

#[test]
fn every_cacheable_operation_must_carry_a_ttl() {
    for method in [
        "server/discover",
        "tools/list",
        "prompts/list",
        "resources/list",
        "resources/templates/list",
        "resources/read",
    ] {
        let bare = trace(&[request(0, 1, method, ""), complete(1, 1, "")]);
        assert_eq!(findings_for(HINTS, &bare).len(), 1, "{method} needs a hint");

        let hinted = trace(&[
            request(0, 1, method, ""),
            complete(1, 1, r#","ttlMs":1000"#),
        ]);
        assert!(findings_for(HINTS, &hinted).is_empty(), "{method}");
    }
}

#[test]
fn a_cache_scope_is_not_required_by_any_clause() {
    // `ttlMs` alone satisfies the clause: nothing on the page makes `cacheScope`
    // mandatory, and demanding it would be inventing a rule.
    let session = trace(&[
        request(0, 1, "tools/list", ""),
        complete(1, 1, r#","ttlMs":0"#),
    ]);
    assert!(findings_for(HINTS, &session).is_empty());
}

#[test]
fn operations_outside_the_cacheable_six_need_no_hint() {
    let session = trace(&[request(0, 1, "tools/call", ""), complete(1, 1, "")]);
    assert!(findings_for(HINTS, &session).is_empty());
}

#[test]
fn an_interim_result_carries_no_hints_and_is_not_reported() {
    let session = trace(&[
        request(0, 1, "resources/read", ""),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"input_required","requestState":"s"}}"#,
        ),
    ]);
    assert!(findings_for(HINTS, &session).is_empty());
}

#[test]
fn a_result_produced_by_a_retry_needs_no_hint() {
    // CACH-003 forbids caching it, and the page's own words for interim results
    // — "not cacheable and carry no caching hints" — are the principle.
    for marker in [r#","requestState":"blob""#, r#","inputResponses":{"k":{}}"#] {
        let session = trace(&[request(0, 1, "resources/read", marker), complete(1, 1, "")]);
        assert!(
            findings_for(HINTS, &session).is_empty(),
            "retry marker {marker}"
        );
    }
}

#[test]
fn a_negative_or_non_numeric_ttl_is_reported() {
    for (ttl, needle) in [
        ("-1", "-1"),
        ("-3600000", "-3600000"),
        (r#""600""#, "not an integer"),
    ] {
        let session = trace(&[
            request(0, 1, "tools/list", ""),
            complete(1, 1, &format!(r#","ttlMs":{ttl}"#)),
        ]);
        let findings = findings_for(TTL, &session);
        assert_eq!(findings.len(), 1, "ttl {ttl}: {findings:?}");
        assert!(findings[0].contains(needle), "{findings:?}");
    }
}

#[test]
fn zero_and_positive_ttls_conform() {
    for ttl in ["0", "1", "3600000"] {
        let session = trace(&[
            request(0, 1, "tools/list", ""),
            complete(1, 1, &format!(r#","ttlMs":{ttl}"#)),
        ]);
        assert!(findings_for(TTL, &session).is_empty(), "ttl {ttl}");
    }
}

#[test]
fn a_paginated_list_keeps_one_scope_across_its_pages() {
    let paged = |second_scope: &str| {
        trace(&[
            request(0, 1, "tools/list", ""),
            complete(
                1,
                1,
                r#","ttlMs":1000,"cacheScope":"private","nextCursor":"page2""#,
            ),
            request(2, 2, "tools/list", r#","cursor":"page2""#),
            complete(
                3,
                2,
                &format!(r#","ttlMs":1000,"cacheScope":{second_scope}"#),
            ),
        ])
    };
    assert!(findings_for(SCOPE, &paged(r#""private""#)).is_empty());

    let switched = findings_for(SCOPE, &paged(r#""public""#));
    assert_eq!(switched.len(), 1, "{switched:?}");
    assert!(switched[0].contains("public"), "{switched:?}");

    // Dropping the field is also not "the same scope".
    let dropped = trace(&[
        request(0, 1, "tools/list", ""),
        complete(
            1,
            1,
            r#","ttlMs":1000,"cacheScope":"private","nextCursor":"page2""#,
        ),
        request(2, 2, "tools/list", r#","cursor":"page2""#),
        complete(3, 2, r#","ttlMs":1000"#),
    ]);
    assert_eq!(findings_for(SCOPE, &dropped).len(), 1);
}

#[test]
fn two_independent_list_calls_may_differ() {
    // Neither carries a cursor, so neither continues the other: the clause binds
    // the pages of *one* request.
    let session = trace(&[
        request(0, 1, "tools/list", ""),
        complete(1, 1, r#","ttlMs":1000,"cacheScope":"private""#),
        request(2, 2, "tools/list", ""),
        complete(3, 2, r#","ttlMs":1000,"cacheScope":"public""#),
    ]);
    assert!(findings_for(SCOPE, &session).is_empty());
}

#[test]
fn a_cursor_from_another_method_does_not_join_the_chain() {
    let session = trace(&[
        request(0, 1, "tools/list", ""),
        complete(
            1,
            1,
            r#","ttlMs":1000,"cacheScope":"private","nextCursor":"page2""#,
        ),
        request(2, 2, "prompts/list", r#","cursor":"page2""#),
        complete(3, 2, r#","ttlMs":1000,"cacheScope":"public""#),
    ]);
    assert!(findings_for(SCOPE, &session).is_empty());
}
