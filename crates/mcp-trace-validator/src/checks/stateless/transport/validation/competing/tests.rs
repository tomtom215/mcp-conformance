// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for requests more than one defined rejection fits: each clause must
//! accept the others' codes, and only theirs.

use crate::checks::stateless::testkit::{META, client, findings_for, post, server, status, trace};

/// TRAN-073's check.
const MISMATCH_CHECK: &str = "transport.version-mismatch-rejected";
/// TRAN-074's and VERS-001's obligation check.
const UNSUPPORTED_CHECK: &str = "transport.unsupported-version-error";
/// TRAN-096's check.
const PARAM_CHECK: &str = "transport.invalid-param-header-rejected";

/// A `_meta` envelope stating `version`, complete.
fn meta(version: &str) -> String {
    format!(
        r#""_meta":{{"io.modelcontextprotocol/protocolVersion":"{version}","io.modelcontextprotocol/clientCapabilities":{{}}}}"#
    )
}

/// A `_meta` envelope stating `version` but missing `clientCapabilities`.
fn malformed(version: &str) -> String {
    format!(r#""_meta":{{"io.modelcontextprotocol/protocolVersion":"{version}"}}"#)
}

/// An error answer to request id 2 carrying `code`.
fn refusal(code: i64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":2,"error":{{"code":{code},"message":"x","data":{{"supported":["2026-07-28"]}}}}}}"#
    )
}

/// The `server/discover` exchange declaring `versions`, at seqs 0–3.
fn discovery(versions: &str) -> Vec<String> {
    vec![
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
    ]
}

/// A `tools/list` POST at seqs 4–7 under `headers`, with `envelope` as its
/// `_meta`, answered `answer` under a 400 — after `discovery`, if any.
fn request(discovery: Vec<String>, headers: &str, envelope: &str, answer: &str) -> String {
    let mut lines = discovery;
    lines.extend([
        post(4, headers),
        client(
            5,
            &format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{{envelope}}}}}"#),
        ),
        status(6, 400),
        server(7, answer),
    ]);
    trace(&lines)
}

/// The headers of a `tools/list` POST stating `version`.
fn headers(version: &str) -> String {
    format!(r#"{{"mcp-protocol-version":"{version}","mcp-method":"tools/list"}}"#)
}

#[test]
fn a_mismatch_naming_an_unsupported_version_may_draw_either_rejection() {
    // Header `2026-07-28`, body `2099-01-01`, which the server does not
    // implement: TRAN-073 and TRAN-074 both bind, and either code satisfies
    // both checks.
    let discovered = || discovery(r#"["2026-07-28"]"#);
    for code in [-32020, -32022] {
        let answered = request(
            discovered(),
            &headers("2026-07-28"),
            &meta("2099-01-01"),
            &refusal(code),
        );
        for check in [MISMATCH_CHECK, UNSUPPORTED_CHECK] {
            let findings = findings_for(check, &answered);
            assert!(findings.is_empty(), "{check} on {code}: {findings:?}");
        }
    }
    // The unsupported version may be the header's, too: with the two
    // disagreeing, the text does not say which one is "requested".
    let header_unsupported = request(
        discovered(),
        &headers("2099-01-01"),
        &meta("2026-07-28"),
        &refusal(-32022),
    );
    assert!(findings_for(MISMATCH_CHECK, &header_unsupported).is_empty());

    // A result is still no rejection at all, under either clause.
    let served = request(
        discovered(),
        &headers("2026-07-28"),
        &meta("2099-01-01"),
        r#"{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete","tools":[]}}"#,
    );
    assert_eq!(findings_for(MISMATCH_CHECK, &served).len(), 1);
    assert_eq!(findings_for(UNSUPPORTED_CHECK, &served).len(), 1);
}

#[test]
fn unsupported_version_is_admitted_only_when_the_server_disclaims_one() {
    // Both versions supported: the mismatch is the only fault, and `-32022`
    // answers a fault the request does not have.
    let both = request(
        discovery(r#"["2026-07-28","2025-11-25"]"#),
        &headers("2025-11-25"),
        &meta("2026-07-28"),
        &refusal(-32022),
    );
    assert_eq!(findings_for(MISMATCH_CHECK, &both).len(), 1);

    // No `server/discover`: the `-32022`'s own list is the server's account,
    // and it omits the body's version.
    let listed = request(
        vec![],
        &headers("2026-07-28"),
        &meta("2099-01-01"),
        &refusal(-32022),
    );
    assert!(findings_for(MISMATCH_CHECK, &listed).is_empty());

    // An empty list, or none at all, disclaims nothing.
    for answer in [
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32022,"message":"x","data":{"supported":[]}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32022,"message":"x"}}"#,
    ] {
        let unlisted = request(vec![], &headers("2026-07-28"), &meta("2099-01-01"), answer);
        assert_eq!(findings_for(MISMATCH_CHECK, &unlisted).len(), 1, "{answer}");
    }
}

#[test]
fn a_malformed_envelope_may_draw_invalid_params_instead() {
    // `basic/index`: a request missing a required `_meta` field "MUST" draw
    // `-32602` — which competes with both header and version rejections.
    let discovered = || discovery(r#"["2026-07-28"]"#);
    let unsupported = request(
        discovered(),
        &headers("2099-01-01"),
        &malformed("2099-01-01"),
        &refusal(-32602),
    );
    assert!(findings_for(UNSUPPORTED_CHECK, &unsupported).is_empty());
    let mismatched = request(
        discovered(),
        &headers("2026-07-28"),
        &malformed("2099-01-01"),
        &refusal(-32602),
    );
    assert!(findings_for(MISMATCH_CHECK, &mismatched).is_empty());

    // With the envelope complete, `-32602` answers nothing the request did.
    let complete = request(
        discovered(),
        &headers("2099-01-01"),
        &meta("2099-01-01"),
        &refusal(-32602),
    );
    assert_eq!(findings_for(UNSUPPORTED_CHECK, &complete).len(), 1);
}

#[test]
fn header_mismatch_is_admitted_only_for_a_header_fault() {
    // Header and body agree on the unsupported version: `-32020` names a
    // mismatch that is not there.
    let agreeing = request(
        discovery(r#"["2026-07-28"]"#),
        &headers("2099-01-01"),
        &meta("2099-01-01"),
        &refusal(-32020),
    );
    let findings = findings_for(UNSUPPORTED_CHECK, &agreeing);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("error -32020"), "{findings:?}");

    // On stdio there is no header to disagree with.
    let stdio = [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}}"#,
        r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{}}}}"#,
        r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2099-01-01","io.modelcontextprotocol/clientCapabilities":{}}}}}"#,
        r#"{"seq":3,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"error":{"code":-32020,"message":"x"}}}"#,
    ]
    .join("\n");
    assert_eq!(findings_for(UNSUPPORTED_CHECK, &stdio).len(), 1);
}

#[test]
fn an_invalid_param_header_competes_with_an_unsupported_version() {
    // A recognized `Mcp-Param-Region` with leading whitespace (TRAN-096) on a
    // request whose agreeing header and body name an unsupported version.
    let mut lines = discovery(r#"["2026-07-28"]"#);
    lines[3] = server(
        3,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{"tools":{}}}}"#,
    );
    lines.push(server(
        8,
        r#"{"jsonrpc":"2.0","id":9,"result":{"resultType":"complete","tools":[{"name":"q","inputSchema":{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Region"}}}}]}}"#,
    ));
    let call = |code: i64| {
        let mut lines = lines.clone();
        lines.extend([
            post(
                9,
                r#"{"mcp-protocol-version":"2099-01-01","mcp-method":"tools/call","mcp-name":"q","mcp-param-region":" us"}"#,
            ),
            client(
                10,
                &format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"q","arguments":{{"region":" us"}},{}}}}}"#,
                    meta("2099-01-01")
                ),
            ),
            status(11, 400),
            server(12, &refusal(code)),
        ]);
        trace(&lines)
    };
    for code in [-32020, -32022] {
        for check in [PARAM_CHECK, UNSUPPORTED_CHECK] {
            let findings = findings_for(check, &call(code));
            assert!(findings.is_empty(), "{check} on {code}: {findings:?}");
        }
    }
}
