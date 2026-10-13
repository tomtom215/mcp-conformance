// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the client-owned request-header clauses: which headers the
//! encoding rules reach, and which defect each of the three encoding checks
//! claims.

use crate::checks::stateless::testkit::{META, client, findings_for, post, server, trace};

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
    assert_eq!(findings_for(check, &document), Vec::<String>::new());
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
    assert_eq!(findings_for(case, &document), Vec::<String>::new());
    assert_eq!(findings_for(pattern, &document), Vec::<String>::new());

    // Miscased markers: the case check's. The encoding check must skip it
    // rather than double-report a value it can see is meant to be encoded.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"=?BASE64?Y2Fmw6k=?="}"#,
        r#""café""#,
    );
    assert_eq!(findings_for(encoding, &document), Vec::<String>::new());
    assert_eq!(findings_for(case, &document).len(), 1);
    assert_eq!(findings_for(pattern, &document), Vec::<String>::new());

    // A plain value shaped like the sentinel, repeated verbatim: the pattern
    // check's. It is header-safe, so the encoding check has nothing to say.
    let document = read(
        r#"{"mcp-method":"resources/read","mcp-name":"=?base64?literal?="}"#,
        r#""=?base64?literal?=""#,
    );
    assert_eq!(findings_for(encoding, &document), Vec::<String>::new());
    assert_eq!(findings_for(case, &document), Vec::<String>::new());
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

/// A `tools/list` designating `region` (string) and `limit` (integer), then a
/// `tools/call` carrying `headers` against `arguments`.
fn call(headers: &str, arguments: &str) -> String {
    trace(&[
        client(
            0,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{META}}}}}"#),
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","tools":[{"name":"q","inputSchema":{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Region"},"limit":{"type":"integer","x-mcp-header":"Limit"}}}}]}}"#,
        ),
        post(2, headers),
        client(
            3,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"q","arguments":{arguments},{META}}}}}"#
            ),
        ),
    ])
}

#[test]
fn a_mirrored_header_must_carry_the_designated_value() {
    let check = "transport.x-mcp-header-value-mirrored";
    let base = r#""mcp-method":"tools/call","mcp-name":"q""#;
    let judge = |headers: &str, arguments: &str| {
        findings_for(check, &call(&format!("{{{base},{headers}}}"), arguments)).len()
    };
    // The value carried, plainly, Base64-encoded, or as the same integer.
    assert_eq!(judge(r#""mcp-param-region":"eu""#, r#"{"region":"eu"}"#), 0);
    assert_eq!(
        judge(
            r#""mcp-param-region":"=?base64?ZXU=?=""#,
            r#"{"region":"eu"}"#
        ),
        0
    );
    assert_eq!(judge(r#""mcp-param-limit":"42.0""#, r#"{"limit":42}"#), 0);
    // Another value, or another number: not a mirror.
    assert_eq!(judge(r#""mcp-param-region":"us""#, r#"{"region":"eu"}"#), 1);
    assert_eq!(judge(r#""mcp-param-limit":"41""#, r#"{"limit":42}"#), 1);
    // Absence is the presence check's finding, and miscased markers TRAN-089's.
    assert_eq!(judge(r#""x-other":"1""#, r#"{"region":"eu"}"#), 0);
    assert_eq!(
        judge(
            r#""mcp-param-region":"=?BASE64?ZXU=?=""#,
            r#"{"region":"eu"}"#
        ),
        0
    );
    // Without the tool list there is no designation to hold the header to.
    let undesignated = trace(&[
        post(0, &format!(r#"{{{base},"mcp-param-region":"us"}}"#)),
        client(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"q","arguments":{{"region":"eu"}},{META}}}}}"#
            ),
        ),
    ]);
    assert_eq!(findings_for(check, &undesignated), Vec::<String>::new());
}
