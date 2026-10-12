// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `x-mcp-header` behaviour table's omission rows.

use crate::checks::stateless::testkit::{META, client, findings_for, post, server, status, trace};

const NULL_OMITTED: &str = "transport.x-mcp-header-null-omitted";
const ABSENT_OMITTED: &str = "transport.x-mcp-header-absent-omitted";
const NULL_NOT_EXPECTED: &str = "transport.null-param-header-not-expected";
const ABSENT_NOT_EXPECTED: &str = "transport.absent-param-header-not-expected";
const REJECTED: &str = "transport.missing-param-header-rejected";

/// `region` set to `null`, and left out of the arguments: each row's argument
/// shape with the check judging it and how a finding names it.
const SCENARIOS: [(&str, &str, &str, &str); 2] = [
    (
        r#"{"region":null,"query":"q"}"#,
        NULL_OMITTED,
        NULL_NOT_EXPECTED,
        "`null`",
    ),
    (
        r#"{"query":"q"}"#,
        ABSENT_OMITTED,
        ABSENT_NOT_EXPECTED,
        "absent from the arguments",
    ),
];

/// A `tools/list` result designating `region` for `Mcp-Param-Region`.
fn tool_list() -> String {
    server(
        0,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","tools":[{"name":"sql","inputSchema":{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Region"},"query":{"type":"string"}}}}]}}"#,
    )
}

/// A `tools/call` of `sql` whose `arguments` are `arguments`, sent as a POST with the
/// standard headers plus `extra` (`,"name":"value"` JSON members), answered by
/// `answer` under HTTP `code`.
fn call(extra: &str, arguments: &str, code: u16, answer: &str) -> String {
    trace(&[
        tool_list(),
        post(
            1,
            &format!(
                r#"{{"mcp-protocol-version":"2026-07-28","mcp-method":"tools/call","mcp-name":"sql"{extra}}}"#
            ),
        ),
        client(
            2,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"sql","arguments":{arguments},{META}}}}}"#
            ),
        ),
        status(3, code),
        server(4, answer),
    ])
}

const RESULT: &str = r#"{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","content":[]}}"#;
const MISMATCH: &str = r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32020,"message":"x"}}"#;

#[test]
fn a_valueless_parameter_carries_no_header_each_row_judged_alone() {
    for (arguments, check, _, named) in SCENARIOS {
        let conforming = call("", arguments, 200, RESULT);
        assert_eq!(
            findings_for(check, &conforming),
            Vec::<String>::new(),
            "{arguments}"
        );
        let sent = call(r#","mcp-param-region":"us""#, arguments, 200, RESULT);
        let findings = findings_for(check, &sent);
        assert_eq!(findings.len(), 1, "{arguments}: {findings:?}");
        assert!(
            findings[0].contains(&format!("`params.arguments.region` is {named}")),
            "{findings:?}"
        );
    }
    // Each row is its own scenario: the other row's check says nothing.
    let null_sent = call(r#","mcp-param-region":"us""#, SCENARIOS[0].0, 200, RESULT);
    assert_eq!(
        findings_for(ABSENT_OMITTED, &null_sent),
        Vec::<String>::new()
    );
    let absent_sent = call(r#","mcp-param-region":"us""#, SCENARIOS[1].0, 200, RESULT);
    assert_eq!(
        findings_for(NULL_OMITTED, &absent_sent),
        Vec::<String>::new()
    );
    // A value present is TRAN-079's business, not this rule's.
    let valued = call(
        r#","mcp-param-region":"us""#,
        r#"{"region":"us"}"#,
        200,
        RESULT,
    );
    for check in [NULL_OMITTED, ABSENT_OMITTED] {
        assert_eq!(
            findings_for(check, &valued),
            Vec::<String>::new(),
            "{check}"
        );
    }
}

#[test]
fn rejecting_a_conforming_omission_is_expecting_the_header() {
    for (arguments, _, check, named) in SCENARIOS {
        let findings = findings_for(check, &call("", arguments, 400, MISMATCH));
        assert_eq!(findings.len(), 1, "{arguments}: {findings:?}");
        assert!(
            findings[0].contains(&format!(
                "omitted `Mcp-Param-Region` for a parameter {named}"
            )),
            "{findings:?}"
        );
        // Accepting it, or failing it for another reason, is not expecting it.
        for answer in [
            RESULT,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"x"}}"#,
        ] {
            let findings = findings_for(check, &call("", arguments, 200, answer));
            assert_eq!(findings, Vec::<String>::new(), "{answer}");
        }
    }
    let null_rejected = call("", SCENARIOS[0].0, 400, MISMATCH);
    assert_eq!(
        findings_for(ABSENT_NOT_EXPECTED, &null_rejected),
        Vec::<String>::new()
    );
}

#[test]
fn a_rejection_with_another_reason_on_the_post_is_not_judged() {
    let absent = SCENARIOS[1].0;
    // The client also sent the header it had to omit, so the server had cause.
    let sent = call(r#","mcp-param-region":"us""#, absent, 400, MISMATCH);
    assert_eq!(
        findings_for(ABSENT_NOT_EXPECTED, &sent),
        Vec::<String>::new()
    );
    // A standard header disagreeing with the body is cause too.
    let wrong_name =
        call("", absent, 400, MISMATCH).replace(r#""mcp-name":"sql""#, r#""mcp-name":"other""#);
    assert_eq!(
        findings_for(ABSENT_NOT_EXPECTED, &wrong_name),
        Vec::<String>::new()
    );
    // So is a protocol-version header disagreeing with `_meta`.
    let wrong_version = call("", absent, 400, MISMATCH).replace(
        r#""mcp-protocol-version":"2026-07-28""#,
        r#""mcp-protocol-version":"2025-11-25""#,
    );
    assert_eq!(
        findings_for(ABSENT_NOT_EXPECTED, &wrong_version),
        Vec::<String>::new()
    );
    // And an unrelated `Mcp-Param-*` header carrying a raw control character.
    let invalid = call(r#","mcp-param-other":"a\u0001b""#, absent, 400, MISMATCH);
    assert_eq!(
        findings_for(ABSENT_NOT_EXPECTED, &invalid),
        Vec::<String>::new()
    );
}

#[test]
fn a_value_in_the_body_without_its_header_must_be_rejected() {
    let accepted = call("", r#"{"region":"us-west1","query":"q"}"#, 200, RESULT);
    let findings = findings_for(REJECTED, &accepted);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("`params.arguments.region` = \"us-west1\" with no `Mcp-Param-Region`"),
        "{findings:?}"
    );
    // Rejected: conforming, whichever code it chose (TRAN-098 judges the code).
    let rejected = call("", r#"{"region":"us-west1"}"#, 400, MISMATCH);
    assert_eq!(findings_for(REJECTED, &rejected), Vec::<String>::new());
    // Header present, or no value to mirror: nothing for this rule.
    for (extra, arguments) in [
        (
            r#","mcp-param-region":"us-west1""#,
            r#"{"region":"us-west1"}"#,
        ),
        ("", r#"{"region":null}"#),
        ("", "{}"),
    ] {
        let findings = findings_for(REJECTED, &call(extra, arguments, 200, RESULT));
        assert_eq!(findings, Vec::<String>::new(), "{extra} {arguments}");
    }
}

#[test]
fn only_a_tools_call_is_held_to_the_table() {
    // A `prompts/get` naming a prompt that shares the tool's name has no
    // designations: the table is about `tools/call` arguments.
    let prompt = call(r#","mcp-param-region":"us""#, "{}", 200, RESULT)
        .replace(
            r#""mcp-method":"tools/call""#,
            r#""mcp-method":"prompts/get""#,
        )
        .replace(r#""method":"tools/call""#, r#""method":"prompts/get""#);
    for check in [NULL_OMITTED, ABSENT_OMITTED] {
        assert_eq!(
            findings_for(check, &prompt),
            Vec::<String>::new(),
            "{check}"
        );
    }
}

#[test]
fn a_correctly_mirrored_second_parameter_is_no_reason_to_reject() {
    // `limit` is designated too, has a value and carries its header exactly;
    // only the valueless `region` is left out, so the rejection is still
    // expecting a header the table says the server must not.
    let trace = trace(&[
        server(
            0,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","tools":[{"name":"sql","inputSchema":{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Region"},"limit":{"type":"integer","x-mcp-header":"Limit"}}}}]}}"#,
        ),
        post(
            1,
            r#"{"mcp-protocol-version":"2026-07-28","mcp-method":"tools/call","mcp-name":"sql","mcp-param-limit":"42"}"#,
        ),
        client(
            2,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"sql","arguments":{{"limit":42}},{META}}}}}"#
            ),
        ),
        status(3, 400),
        server(4, MISMATCH),
    ]);
    assert_eq!(findings_for(ABSENT_NOT_EXPECTED, &trace).len(), 1);
}

#[test]
fn nothing_is_judged_without_a_designating_tool_list() {
    // Each of these fails its check while the trace carries the tool list.
    let cases = [
        (
            NULL_OMITTED,
            call(r#","mcp-param-region":"us""#, SCENARIOS[0].0, 200, RESULT),
        ),
        (
            ABSENT_OMITTED,
            call(r#","mcp-param-region":"us""#, SCENARIOS[1].0, 200, RESULT),
        ),
        (NULL_NOT_EXPECTED, call("", SCENARIOS[0].0, 400, MISMATCH)),
        (ABSENT_NOT_EXPECTED, call("", SCENARIOS[1].0, 400, MISMATCH)),
        (REJECTED, call("", r#"{"region":"us"}"#, 200, RESULT)),
    ];
    for (check, with_list) in cases {
        assert_eq!(findings_for(check, &with_list).len(), 1, "{check}");
        let without_list = with_list.replacen(&tool_list(), "", 1);
        assert_eq!(
            findings_for(check, without_list.trim_start()),
            Vec::<String>::new(),
            "{check}"
        );
    }
}
