// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the versioning and cross-era clauses.
//!
//! Each check here reads a *pair* of things — an error and what followed it, a
//! capability surface and the keys inside it, a handshake refusal and its
//! contents — so the cases that matter are the ones where only half the pair is
//! present. Those are the ones a single corpus trace cannot reach.

use super::contains_revision;
use crate::checks::draft::testkit::{client, findings_for, server, trace};

const RETRY: &str = "versioning.retry-uses-supported-version";
const EXTENSIONS: &str = "versioning.extension-identifier-format";
const INIT_ERROR: &str = "versioning.initialize-error-names-versions";

/// A client request declaring `version`, with no extensions.
fn request_at(seq: u64, id: u64, version: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/list","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"{version}","io.modelcontextprotocol/clientCapabilities":{{}}}}}}}}"#
        ),
    )
}

/// A `-32022` whose `data` is `data`.
fn unsupported(seq: u64, id: u64, data: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32022,"message":"Unsupported protocol version"{data}}}}}"#
        ),
    )
}

/// A client request whose `_meta` client capabilities carry `extensions`.
fn client_extensions(seq: u64, extensions: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{{"extensions":{extensions}}}}}}}}}"#
        ),
    )
}

/// A `server/discover` exchange whose result capabilities carry `extensions`.
fn server_extensions(extensions: &str) -> String {
    trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}"#,
        ),
        server(
            1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{{"extensions":{extensions}}}}}}}"#
            ),
        ),
    ])
}

/// An `initialize` refused with `error`.
fn refused_handshake(error: &str) -> String {
    trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        ),
        server(1, &format!(r#"{{"jsonrpc":"2.0","id":1,"error":{error}}}"#)),
    ])
}

#[test]
fn without_a_supported_list_the_client_is_not_judged() {
    // Both shapes: no `-32022` at all, and a `-32022` whose list is missing or
    // empty. The missing list is the *server's* defect
    // (`transport.unsupported-version-error`); charging the client for it would
    // report the wrong party.
    for data in [
        "",
        r#","data":{"requested":"1900-01-01"}"#,
        r#","data":{"supported":[]}"#,
    ] {
        let session = trace(&[
            request_at(0, 1, "1900-01-01"),
            unsupported(1, 1, data),
            request_at(2, 2, "1899-01-01"),
        ]);
        assert!(
            findings_for(RETRY, &session).is_empty(),
            "data {data:?} should leave the client unjudged"
        );
    }
}

#[test]
fn retrying_outside_the_offered_list_is_the_violation() {
    let session = trace(&[
        request_at(0, 1, "1900-01-01"),
        unsupported(
            1,
            1,
            r#","data":{"supported":["2026-07-28"],"requested":"1900-01-01"}"#,
        ),
        request_at(2, 2, "1899-01-01"),
    ]);
    let findings = findings_for(RETRY, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("1899-01-01"), "{findings:?}");
    assert!(findings[0].contains("2026-07-28"), "{findings:?}");
}

#[test]
fn retrying_inside_the_offered_list_conforms() {
    let session = trace(&[
        request_at(0, 1, "1900-01-01"),
        unsupported(1, 1, r#","data":{"supported":["2026-07-28"]}"#),
        request_at(2, 2, "2026-07-28"),
    ]);
    assert!(findings_for(RETRY, &session).is_empty());
}

#[test]
fn the_request_that_drew_the_error_is_not_itself_a_violation() {
    // Only requests *after* the list arrives are judged: the client could not
    // have selected from a list it had not been given.
    let session = trace(&[
        request_at(0, 1, "1900-01-01"),
        unsupported(1, 1, r#","data":{"supported":["2026-07-28"]}"#),
    ]);
    assert!(findings_for(RETRY, &session).is_empty());
}

#[test]
fn a_later_error_replaces_the_offered_list() {
    let session = trace(&[
        request_at(0, 1, "1900-01-01"),
        unsupported(1, 1, r#","data":{"supported":["2025-11-25"]}"#),
        request_at(2, 2, "2025-11-25"),
        unsupported(3, 2, r#","data":{"supported":["2026-07-28"]}"#),
        request_at(4, 3, "2025-11-25"),
    ]);
    let findings = findings_for(RETRY, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("seq 3"), "{findings:?}");
}

#[test]
fn a_request_declaring_no_version_is_not_judged_here() {
    // Its absence is BASE-030's finding, not a wrong version selection.
    let session = trace(&[
        request_at(0, 1, "1900-01-01"),
        unsupported(1, 1, r#","data":{"supported":["2026-07-28"]}"#),
        client(2, r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#),
    ]);
    assert!(findings_for(RETRY, &session).is_empty());
}

#[test]
fn a_notification_is_not_a_retry() {
    let session = trace(&[
        request_at(0, 1, "1900-01-01"),
        unsupported(1, 1, r#","data":{"supported":["2026-07-28"]}"#),
        client(
            2,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"1899-01-01"}}}"#,
        ),
    ]);
    assert!(findings_for(RETRY, &session).is_empty());
}

#[test]
fn a_prefixed_extension_identifier_conforms() {
    let session = client_extensions(
        0,
        r#"{"io.modelcontextprotocol/ui":{"mimeTypes":["text/html"]}}"#,
    );
    assert!(findings_for(EXTENSIONS, &session).is_empty());
}

#[test]
fn an_extension_identifier_without_a_prefix_is_reported() {
    // Valid as a bare `_meta` key — where the prefix is optional — and invalid
    // here, which is the clause's whole addition.
    let session = client_extensions(0, r#"{"ui":{}}"#);
    let findings = findings_for(EXTENSIONS, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("no prefix"), "{findings:?}");
    assert!(findings[0].contains("client capabilities"), "{findings:?}");
}

#[test]
fn an_extension_identifier_breaking_the_meta_grammar_is_reported() {
    let session = client_extensions(0, r#"{"1bad./x":{}}"#);
    let findings = findings_for(EXTENSIONS, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("prefix label"), "{findings:?}");
}

#[test]
fn the_servers_own_extensions_are_judged_too() {
    let findings = findings_for(EXTENSIONS, &server_extensions(r#"{"tasks":{}}"#));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("server capabilities"), "{findings:?}");
    let clean = server_extensions(r#"{"io.modelcontextprotocol/tasks":{}}"#);
    assert!(findings_for(EXTENSIONS, &clean).is_empty());
}

#[test]
fn a_capability_surface_without_extensions_is_not_judged() {
    for extensions in ["{}", "null", "[]", r#""io.modelcontextprotocol/ui""#] {
        let session = client_extensions(0, extensions);
        assert!(
            findings_for(EXTENSIONS, &session).is_empty(),
            "extensions {extensions} should yield no identifiers"
        );
    }
}

#[test]
fn a_handshake_answered_with_a_result_is_a_dual_era_server() {
    // Never reached: the clause is about a *refusal*, and a server that answers
    // the handshake is serving the legacy era rather than refusing it.
    let session = trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}"#,
        ),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}"#,
        ),
    ]);
    assert!(findings_for(INIT_ERROR, &session).is_empty());
}

#[test]
fn a_refusal_naming_no_version_is_reported() {
    let findings = findings_for(
        INIT_ERROR,
        &refused_handshake(r#"{"code":-32601,"message":"Method not found"}"#),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("fall-forward"), "{findings:?}");
}

#[test]
fn a_refusal_names_its_versions_in_data_or_in_prose() {
    for error in [
        r#"{"code":-32022,"message":"Unsupported","data":{"supported":["2026-07-28"]}}"#,
        r#"{"code":-32601,"message":"This server speaks 2026-07-28 and later only"}"#,
        r#"{"code":-32601,"message":"no","data":{"note":["see 2026-07-28"]}}"#,
    ] {
        assert!(
            findings_for(INIT_ERROR, &refused_handshake(error)).is_empty(),
            "{error} names a version"
        );
    }
}

#[test]
fn a_date_shaped_string_that_is_not_a_revision_does_not_count() {
    let findings = findings_for(
        INIT_ERROR,
        &refused_handshake(r#"{"code":-32601,"message":"retry after 2026-13-45"}"#),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn contains_revision_scans_anywhere_in_the_string() {
    assert!(contains_revision("2026-07-28"));
    assert!(contains_revision("we speak 2026-07-28."));
    assert!(contains_revision("…2026-07-28"), "multi-byte prefix");
    assert!(!contains_revision("2026-07-2"));
    assert!(!contains_revision("2026/07/28"));
    assert!(!contains_revision("2026-00-28"));
    assert!(!contains_revision(""));
}
