// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the server-owned rejection clauses: which answer a bad POST had to
//! draw, and under which HTTP status.

use crate::checks::draft::testkit::{
    META, client, error, findings_for, post, server, status, trace,
};

/// A `tools/list` POST whose header states `header_version` and whose `_meta`
/// states `body_version`, answered by `answer` under HTTP `code`.
fn exchange(header_version: &str, body_version: &str, code: u16, answer: &str) -> String {
    trace(&[
        post(
            0,
            &format!(r#"{{"mcp-protocol-version":"{header_version}","mcp-method":"tools/list"}}"#),
        ),
        client(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"{body_version}","io.modelcontextprotocol/clientCapabilities":{{}}}}}}}}"#
            ),
        ),
        status(2, code),
        server(3, answer),
    ])
}

const RESULT: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","tools":[]}}"#;
const MISMATCH: &str = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32020,"message":"x"}}"#;

#[test]
fn a_version_mismatch_must_draw_header_mismatch() {
    let check = "transport.version-mismatch-rejected";

    // Answered with a result: the server did not reject what it had to.
    let findings = findings_for(check, &exchange("2025-11-25", "2026-07-28", 200, RESULT));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("a result"), "{findings:?}");

    // Answered with the wrong error: still not a rejection, and the finding
    // names the code that was used instead.
    let findings = findings_for(
        check,
        &exchange(
            "2025-11-25",
            "2026-07-28",
            400,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"x"}}"#,
        ),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("error -32602"), "{findings:?}");

    // Rejected correctly, and the agreeing case: silence in both.
    assert!(findings_for(check, &exchange("2025-11-25", "2026-07-28", 400, MISMATCH)).is_empty());
    assert!(findings_for(check, &exchange("2026-07-28", "2026-07-28", 200, RESULT)).is_empty());
}

#[test]
fn header_mismatch_must_ride_a_400() {
    let check = "transport.header-mismatch-status";

    // The code is right, the status is not.
    let findings = findings_for(check, &exchange("2026-07-28", "2026-07-28", 500, MISMATCH));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("HTTP 500"), "{findings:?}");

    // 400 is the required pairing.
    assert!(findings_for(check, &exchange("2026-07-28", "2026-07-28", 400, MISMATCH)).is_empty());

    // Any other error code is some other clause's business.
    assert!(
        findings_for(
            check,
            &exchange(
                "2026-07-28",
                "2026-07-28",
                500,
                r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"x"}}"#
            )
        )
        .is_empty()
    );
}

/// A discovery exchange declaring `versions`, then a request naming `requested`
/// that the server answers with `answer`.
fn after_discovery(versions: &str, requested: &str, answer: &str) -> String {
    trace(&[
        post(
            0,
            r#"{"mcp-protocol-version":"2026-07-28","mcp-method":"server/discover"}"#,
        ),
        client(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{{META}}}}}"#
            ),
        ),
        status(2, 200),
        server(
            3,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","supportedVersions":{versions},"capabilities":{{}}}}}}"#
            ),
        ),
        post(
            4,
            &format!(r#"{{"mcp-protocol-version":"{requested}","mcp-method":"tools/list"}}"#),
        ),
        client(
            5,
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"{requested}","io.modelcontextprotocol/clientCapabilities":{{}}}}}}}}"#
            ),
        ),
        status(6, 200),
        server(7, answer),
    ])
}

#[test]
fn an_unsupported_version_is_judged_against_what_the_server_declared() {
    let check = "transport.unsupported-version-error";
    let result = r#"{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","tools":[]}}"#;

    // Outside the declared set, answered with a result.
    let findings = findings_for(
        check,
        &after_discovery(r#"["2026-07-28"]"#, "2025-11-25", result),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");

    // Inside the declared set: nothing to report.
    assert!(
        findings_for(
            check,
            &after_discovery(r#"["2026-07-28","2025-11-25"]"#, "2025-11-25", result)
        )
        .is_empty()
    );

    // Declared as an empty list: the server said nothing usable, so the check
    // abstains rather than treating every version as unsupported.
    assert!(findings_for(check, &after_discovery("[]", "2025-11-25", result)).is_empty());

    // With no discovery at all there is no list to judge against.
    assert!(findings_for(check, &exchange("2026-07-28", "2026-07-28", 200, RESULT)).is_empty());
}

#[test]
fn the_unsupported_version_error_must_list_the_versions_and_carry_a_400() {
    let check = "transport.unsupported-version-error";

    let without_list = trace(&[
        post(
            0,
            r#"{"mcp-protocol-version":"2026-07-28","mcp-method":"tools/list"}"#,
        ),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{META}}}}}"#),
        ),
        status(2, 400),
        error(3, "1", -32022),
    ]);
    let findings = findings_for(check, &without_list);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("data.supported"), "{findings:?}");

    // With the list present and a 400, nothing is reported.
    let complete = trace(&[
        post(
            0,
            r#"{"mcp-protocol-version":"2026-07-28","mcp-method":"tools/list"}"#,
        ),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{{META}}}}}"#),
        ),
        status(2, 400),
        server(
            3,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32022,"message":"x","data":{"supported":["2026-07-28"],"requested":"1900-01-01"}}}"#,
        ),
    ]);
    assert!(findings_for(check, &complete).is_empty());

    // An empty list is not a list of supported versions, and a non-400 status
    // is its own finding on top.
    let empty_and_200 = complete
        .replace(r#"["2026-07-28"]"#, "[]")
        .replace(r#""status":400"#, r#""status":200"#);
    let findings = findings_for(check, &empty_and_200);
    assert_eq!(findings.len(), 2, "{findings:?}");
}

#[test]
fn method_not_found_must_ride_a_404() {
    let check = "transport.unknown-method-404";
    let not_found = trace(&[
        post(
            0,
            r#"{"mcp-protocol-version":"2026-07-28","mcp-method":"tools/nope"}"#,
        ),
        client(
            1,
            &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"tools/nope","params":{{{META}}}}}"#),
        ),
        status(2, 200),
        error(3, "1", -32601),
    ]);
    assert_eq!(findings_for(check, &not_found).len(), 1);
    assert!(findings_for(check, &not_found.replace("200", "404")).is_empty());

    // On stdio there is no status and no POST, so the clause does not bind.
    let stdio = [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/nope"}}"#,
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}}"#,
    ]
    .join("\n");
    assert!(findings_for(check, &stdio).is_empty());
}
