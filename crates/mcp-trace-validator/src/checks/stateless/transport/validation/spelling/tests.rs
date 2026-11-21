// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::expect_used)]

use crate::checks;
use crate::checks::stateless::testkit::{META, client, error, events, post, server, status, trace};
use crate::context::TraceContext;

const ENCODED: &str = "transport.encoded-header-decoded";
const NUMERIC: &str = "transport.integer-header-compared-numerically";

/// `file:///é`, and its sentinel form.
const URI: &str = "file:///\u{e9}";
const URI_ENCODED: &str = "=?base64?ZmlsZTovLy/DqQ==?=";
/// `sql`, encoded although it did not need to be.
const SQL_ENCODED: &str = "=?base64?c3Fs?=";

/// How many findings and subjects `check` reports on `lines`.
fn run(check: &str, lines: &[String]) -> (usize, u32) {
    let events = events(&trace(lines));
    let context = TraceContext::new(&events);
    let outcome = checks::find(check)
        .expect("check is registered")
        .run(&context);
    (outcome.findings.len(), outcome.subjects)
}

/// A request sent under `headers` (the version header added unless given),
/// answered with a result, or with error `code`.
fn exchange(
    first: u64,
    headers: &str,
    method: &str,
    params: &str,
    code: Option<i64>,
) -> Vec<String> {
    let headers = if headers.contains("mcp-protocol-version") || headers.contains("no-version") {
        format!("{{{headers}}}")
    } else {
        format!(r#"{{"mcp-protocol-version":"2026-07-28",{headers}}}"#)
    };
    let request = format!(
        r#"{{"jsonrpc":"2.0","id":{first},"method":"{method}","params":{{{params}{META}}}}}"#
    );
    let answer = code.map_or_else(
        || {
            server(
                first + 3,
                &format!(r#"{{"jsonrpc":"2.0","id":{first},"result":{{}}}}"#),
            )
        },
        |code| error(first + 3, &first.to_string(), code),
    );
    vec![
        post(first, &headers),
        client(first + 1, &request),
        status(first + 2, 200),
        answer,
    ]
}

/// A `resources/read` of [`URI`] under `headers`.
fn read(headers: &str, code: Option<i64>) -> Vec<String> {
    exchange(
        0,
        headers,
        "resources/read",
        &format!(r#""uri":"{URI}","#),
        code,
    )
}

fn read_headers(name: &str) -> String {
    format!(r#""mcp-method":"resources/read","mcp-name":"{name}""#)
}

/// A tool list declaring `sql`, with `limit` (integer) and `region` (string)
/// designated for headers, then a call of `tool` under `headers`.
fn call(tool: &str, headers: &str, arguments: &str, code: Option<i64>) -> Vec<String> {
    let listing = r#"{"jsonrpc":"2.0","id":90,"result":{"tools":[{"name":"sql","inputSchema":{"type":"object","properties":{"limit":{"type":"integer","x-mcp-header":"Limit"},"region":{"type":"string","x-mcp-header":"Region"}}}}]}}"#;
    let mut lines = vec![server(0, listing)];
    lines.extend(exchange(
        10,
        &format!(r#""mcp-method":"tools/call",{headers}"#),
        "tools/call",
        &format!(r#""name":"{tool}","arguments":{{{arguments}}},"#),
        code,
    ));
    lines
}

#[test]
fn an_encoded_name_answered_with_anything_but_header_mismatch_passes() {
    let headers = read_headers(URI_ENCODED);
    assert_eq!(run(ENCODED, &read(&headers, None)), (0, 1));
    assert_eq!(run(ENCODED, &read(&headers, Some(-32602))), (0, 1));
    // Not the numeric clause's subject.
    assert_eq!(run(NUMERIC, &read(&headers, Some(-32020))), (0, 0));
}

#[test]
fn an_encoded_name_rejected_as_a_mismatch_is_a_violation() {
    let lines = read(&read_headers(URI_ENCODED), Some(-32020));
    let events = events(&trace(&lines));
    let context = TraceContext::new(&events);
    let outcome = checks::find(ENCODED)
        .expect("check is registered")
        .run(&context);
    assert_eq!(outcome.findings.len(), 1);
    let finding = &outcome.findings[0];
    assert_eq!(finding.seq, Some(3));
    assert!(
        finding.detail.contains("POST at seq 0"),
        "{}",
        finding.detail
    );
    assert!(
        finding.detail.contains("the Base64 sentinel is decoded"),
        "{}",
        finding.detail
    );
}

#[test]
fn a_request_with_another_ground_for_header_mismatch_is_not_a_subject() {
    let encoded = read_headers(URI_ENCODED);
    for (why, headers) in [
        (
            "no version header",
            format!(r#""no-version":"x",{encoded}"#),
        ),
        (
            "version header disagrees with the body",
            format!(r#""mcp-protocol-version":"2025-11-25",{encoded}"#),
        ),
        (
            "Mcp-Method missing",
            format!(r#""mcp-name":"{URI_ENCODED}""#),
        ),
        (
            "Mcp-Method wrong",
            format!(r#""mcp-method":"tools/list","mcp-name":"{URI_ENCODED}""#),
        ),
        (
            "Mcp-Method encoded, which it may not be",
            format!(r#""mcp-method":"=?base64?cmVzb3VyY2VzL3JlYWQ=?=","mcp-name":"{URI_ENCODED}""#),
        ),
        (
            "an Mcp-Param header no designation explains",
            format!(r#"{encoded},"mcp-param-region":"x""#),
        ),
        (
            "the name decodes to another value",
            read_headers(SQL_ENCODED),
        ),
    ] {
        assert_eq!(run(ENCODED, &read(&headers, Some(-32020))), (0, 0), "{why}");
    }
    // A header outside the mirrored family does not disqualify a request.
    let harmless = format!(r#""x-other":"1",{encoded}"#);
    assert_eq!(run(ENCODED, &read(&harmless, Some(-32020))), (1, 1));
}

#[test]
fn only_an_answered_request_with_an_encoded_header_is_a_subject() {
    // Every header plain: nothing was encoded.
    let plain = exchange(
        0,
        r#""mcp-method":"prompts/get","mcp-name":"p""#,
        "prompts/get",
        r#""name":"p","#,
        Some(-32020),
    );
    assert_eq!(run(ENCODED, &plain), (0, 0));
    assert_eq!(run(NUMERIC, &plain), (0, 0));
    // A notification is not a request.
    let mut notification = read(&read_headers(URI_ENCODED), Some(-32020));
    notification[1] = client(
        1,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"resources/read","params":{{"uri":"{URI}",{META}}}}}"#
        ),
    );
    assert_eq!(run(ENCODED, &notification), (0, 0));
    // Mcp-Name on a method that sources none is a stray header.
    let stray = exchange(
        0,
        &format!(r#""mcp-method":"tools/list","mcp-name":"{SQL_ENCODED}""#),
        "tools/list",
        "",
        Some(-32020),
    );
    assert_eq!(run(ENCODED, &stray), (0, 0));
}

#[test]
fn a_tool_call_is_judged_only_against_a_listed_tool() {
    let headers = format!(r#""mcp-name":"{SQL_ENCODED}""#);
    assert_eq!(
        run(ENCODED, &call("sql", &headers, "", Some(-32020))),
        (1, 1)
    );
    assert_eq!(run(ENCODED, &call("sql", &headers, "", None)), (0, 1));
    // An unlisted tool may expect headers the trace knows nothing of.
    let unlisted = r#""mcp-name":"=?base64?b3RoZXI=?=""#;
    assert_eq!(
        run(ENCODED, &call("other", unlisted, "", Some(-32020))),
        (0, 0)
    );
}

#[test]
fn an_encoded_parameter_is_a_subject_and_an_unsafe_plain_one_disqualifies() {
    // `région`, encoded.
    let encoded = r#""mcp-name":"sql","mcp-param-region":"=?base64?csOpZ2lvbg==?=""#;
    let arguments = "\"region\":\"r\u{e9}gion\"";
    assert_eq!(
        run(ENCODED, &call("sql", encoded, arguments, Some(-32020))),
        (1, 1)
    );
    // The same value sent verbatim is a header no server need accept; the
    // encoded name beside it no longer isolates the decoding.
    let verbatim = format!("\"mcp-name\":\"{SQL_ENCODED}\",\"mcp-param-region\":\"r\u{e9}gion\"");
    assert_eq!(
        run(ENCODED, &call("sql", &verbatim, arguments, Some(-32020))),
        (0, 0)
    );
    // A sentinel-shaped body value sent verbatim is the client's fault.
    let sentinel = format!(r#""mcp-name":"{SQL_ENCODED}","mcp-param-region":"=?base64?eA==?=""#);
    let literal = r#""region":"=?base64?eA==?=""#;
    assert_eq!(
        run(ENCODED, &call("sql", &sentinel, literal, Some(-32020))),
        (0, 0)
    );
    // A designated parameter the body carries, with no header: a real fault.
    let missing = format!(r#""mcp-name":"{SQL_ENCODED}""#);
    assert_eq!(
        run(
            ENCODED,
            &call("sql", &missing, r#""region":"x""#, Some(-32020))
        ),
        (0, 0)
    );
}

#[test]
fn a_respelled_integer_is_the_numeric_clauses_subject() {
    let headers = r#""mcp-name":"sql","mcp-param-limit":"100.0""#;
    let arguments = r#""limit":100"#;
    assert_eq!(run(NUMERIC, &call("sql", headers, arguments, None)), (0, 1));
    assert_eq!(
        run(NUMERIC, &call("sql", headers, arguments, Some(-32602))),
        (0, 1)
    );
    assert_eq!(
        run(NUMERIC, &call("sql", headers, arguments, Some(-32020))),
        (1, 1)
    );
    // Not the decoding clause's subject.
    assert_eq!(
        run(ENCODED, &call("sql", headers, arguments, Some(-32020))),
        (0, 0)
    );
    // The canonical spelling is no respelling.
    let canonical = r#""mcp-name":"sql","mcp-param-limit":"100""#;
    assert_eq!(
        run(NUMERIC, &call("sql", canonical, arguments, Some(-32020))),
        (0, 0)
    );
    // A different number is a real mismatch.
    let different = r#""mcp-name":"sql","mcp-param-limit":"100.5""#;
    assert_eq!(
        run(NUMERIC, &call("sql", different, arguments, Some(-32020))),
        (0, 0)
    );
}

#[test]
fn a_request_using_both_spellings_is_neither_clauses_subject() {
    let headers = format!(r#""mcp-name":"{SQL_ENCODED}","mcp-param-limit":"100.0""#);
    let lines = call("sql", &headers, r#""limit":100"#, Some(-32020));
    assert_eq!(run(ENCODED, &lines), (0, 0));
    assert_eq!(run(NUMERIC, &lines), (0, 0));
}
