// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the client-owned request-header clauses: which headers the
//! encoding rules reach, and which defect each of the three encoding checks
//! claims.

use crate::checks::draft::testkit::{META, client, findings_for, post, trace};

/// A `resources/read` POST carrying `headers`, whose `params.uri` is `uri`.
fn read(headers: &str, uri: &str) -> String {
    trace(&[
        post(0, headers),
        client(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"resources/read","params":{{"uri":{uri},{META}}}}}"#
            ),
        ),
    ])
}

#[test]
fn the_encoding_rules_reach_mcp_name_and_mcp_param_only() {
    let check = "transport.header-value-encoding";

    // `Mcp-Name` and any `Mcp-Param-*` are encodable and therefore judged.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"café"}"#,
        r#""café""#,
    );
    assert_eq!(findings_for(check, &document).len(), 1);

    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"a","mcp-param-region":" padded"}"#,
        r#""a""#,
    );
    assert_eq!(findings_for(check, &document).len(), 1);

    // A header outside that set carries whatever it carries; these clauses say
    // nothing about `Mcp-Method`, `Accept` or a vendor header.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"a","x-vendor":"café","accept":" padded"}"#,
        r#""a""#,
    );
    assert!(findings_for(check, &document).is_empty());
}

#[test]
fn each_encoding_defect_is_claimed_by_exactly_one_check() {
    let encoding = "transport.header-value-encoding";
    let case = "transport.sentinel-marker-case";
    let pattern = "transport.sentinel-pattern-encoded";

    // Unencoded non-ASCII: the encoding check's, and only its.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"café"}"#,
        r#""café""#,
    );
    assert_eq!(findings_for(encoding, &document).len(), 1);
    assert!(findings_for(case, &document).is_empty());
    assert!(findings_for(pattern, &document).is_empty());

    // Miscased markers: the case check's. The encoding check must skip it
    // rather than double-report a value it can see is meant to be encoded.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"=?BASE64?Y2Fmw6k=?="}"#,
        r#""café""#,
    );
    assert!(findings_for(encoding, &document).is_empty());
    assert_eq!(findings_for(case, &document).len(), 1);
    assert!(findings_for(pattern, &document).is_empty());

    // A plain value shaped like the sentinel, repeated verbatim: the pattern
    // check's. It is header-safe, so the encoding check has nothing to say.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"=?base64?literal?="}"#,
        r#""=?base64?literal?=""#,
    );
    assert!(findings_for(encoding, &document).is_empty());
    assert!(findings_for(case, &document).is_empty());
    assert_eq!(findings_for(pattern, &document).len(), 1);

    // A sentinel whose payload is not itself safely representable is not an
    // encoded value at all, and the encoding check says so.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"=?base64?café?="}"#,
        r#""café""#,
    );
    assert_eq!(findings_for(encoding, &document).len(), 1);

    // Correctly encoded: none of the three fires.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"=?base64?Y2Fmw6k=?="}"#,
        r#""café""#,
    );
    for check in [encoding, case, pattern] {
        assert!(findings_for(check, &document).is_empty(), "{check}");
    }
}

#[test]
fn notification_posts_are_outside_every_header_clause() {
    // "Header requirements for notification POSTs are not defined by this
    // revision" — so a notification with no headers at all is not a finding.
    let document = trace(&[
        post(0, r#"{"mcp-name":"café"}"#),
        client(
            1,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#,
        ),
    ]);
    for check in [
        "transport.protocol-version-header-present",
        "transport.request-metadata-headers",
        "transport.header-value-encoding",
        "transport.sentinel-marker-case",
    ] {
        assert!(findings_for(check, &document).is_empty(), "{check}");
    }
}
