// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use crate::checks::stateless::testkit::{META, client, findings_for, server, trace};

const CHECK: &str = "transport.x-mcp-header-invalid-tool-not-called";

/// A tool whose only annotation sits under `anyOf` (TRAN-082).
const UNREACHABLE: &str = r#"{"name":"deploy","inputSchema":{"type":"object","properties":{"region":{"anyOf":[{"type":"string","x-mcp-header":"Region"}]}}}}"#;
/// A tool whose annotation is not a header token (TRAN-080).
const BAD_NAME: &str = r#"{"name":"deploy","inputSchema":{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Re gion"}}}}"#;
/// The same tool, annotated validly.
const VALID: &str = r#"{"name":"deploy","inputSchema":{"type":"object","properties":{"region":{"type":"string","x-mcp-header":"Region"}}}}"#;
/// A second, valid tool with no annotation.
const OTHER: &str = r#"{"name":"echo","inputSchema":{"type":"object"}}"#;

fn list(seq: u64, id: u64, tools: &str) -> [String; 2] {
    [
        client(
            seq,
            &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{{META}}}}}"#),
        ),
        server(
            seq + 1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete","tools":[{tools}]}}}}"#
            ),
        ),
    ]
}

fn call(seq: u64, id: u64, tool: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{tool}","arguments":{{"region":"us"}},{META}}}}}"#
        ),
    )
}

#[test]
fn calling_a_tool_the_client_had_to_reject_is_a_finding() {
    for invalid in [UNREACHABLE, BAD_NAME] {
        let [request, response] = list(0, 1, invalid);
        let findings = findings_for(CHECK, &trace(&[request, response, call(2, 2, "deploy")]));
        assert_eq!(findings.len(), 1, "{invalid}: {findings:?}");
        assert!(findings[0].contains("\"deploy\""), "{findings:?}");
    }
}

#[test]
fn calling_only_the_valid_neighbour_is_no_finding() {
    let [request, response] = list(0, 1, &format!("{UNREACHABLE},{OTHER}"));
    assert_eq!(
        findings_for(CHECK, &trace(&[request, response, call(2, 2, "echo")])),
        Vec::<String>::new()
    );
}

#[test]
fn a_later_valid_definition_closes_the_window() {
    let [a, b] = list(0, 1, UNREACHABLE);
    let [c, d] = list(2, 2, VALID);
    assert_eq!(
        findings_for(CHECK, &trace(&[a, b, c, d, call(4, 3, "deploy")])),
        Vec::<String>::new()
    );
    // And the order matters: a call before the invalid list is no finding.
    let [a, b] = list(1, 2, UNREACHABLE);
    assert_eq!(
        findings_for(CHECK, &trace(&[call(0, 1, "deploy"), a, b])),
        Vec::<String>::new()
    );
}

#[test]
fn stdio_clients_may_ignore_the_annotations() {
    let [request, response] = list(0, 1, UNREACHABLE);
    let stdio =
        trace(&[request, response, call(2, 2, "deploy")]).replace("streamable-http", "stdio");
    assert_eq!(findings_for(CHECK, &stdio), Vec::<String>::new());
}
