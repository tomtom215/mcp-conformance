// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the self-identification SHOULDs: `clientInfo` on requests,
//! `serverInfo` on results and on the discovery result.

use crate::checks::stateless::testkit::{META, client, error, findings_for, server, trace};

const CLIENT: &str = "meta.client-info-on-requests";
const RESULTS: &str = "meta.server-info-on-results";
const DISCOVER: &str = "discover.server-info-included";

const CLIENT_INFO: &str = r#""io.modelcontextprotocol/clientInfo":{"name":"c","version":"0"}"#;
const SERVER_INFO: &str =
    r#""_meta":{"io.modelcontextprotocol/serverInfo":{"name":"s","version":"0"}}"#;

/// A request for `method` whose `_meta` is the envelope plus `extra`.
fn request(seq: u64, id: u64, method: &str, extra: &str) -> String {
    // The envelope without its closing brace, so `extra` lands inside it.
    let meta = &META[..META.len() - 1];
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{{meta}{extra}}}}}}}"#
        ),
    )
}

/// A result for `id` whose members are `members`.
fn result(seq: u64, id: u64, members: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete"{members}}}}}"#),
    )
}

#[test]
fn a_request_without_client_info_warns() {
    let without = trace(&[request(0, 1, "ping", "")]);
    assert_eq!(findings_for(CLIENT, &without).len(), 1);
    let with = trace(&[request(0, 1, "ping", &format!(",{CLIENT_INFO}"))]);
    assert_eq!(findings_for(CLIENT, &with), Vec::<String>::new());
}

#[test]
fn only_requests_under_the_envelope_are_judged() {
    // A notification, a legacy `initialize`, and a request with no envelope
    // (BASE-030's finding) are all outside the clause.
    let outside = trace(&[
        client(
            0,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        ),
        client(
            1,
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}"#,
        ),
        client(2, r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#),
    ]);
    assert_eq!(findings_for(CLIENT, &outside), Vec::<String>::new());
}

#[test]
fn every_result_should_name_the_server() {
    let session = trace(&[
        request(0, 1, "ping", ""),
        result(1, 1, ""),
        request(2, 2, "ping", ""),
        result(3, 2, &format!(",{SERVER_INFO}")),
        request(4, 3, "ping", ""),
        error(5, "3", -32603),
    ]);
    // One result without it; the one with it and the error are no finding.
    let findings = findings_for(RESULTS, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("`ping`"), "{findings:?}");
    // Not the discovery clause's business: no discover result here.
    assert_eq!(findings_for(DISCOVER, &session), Vec::<String>::new());
}

#[test]
fn a_discover_result_without_server_info_breaks_both_clauses() {
    let bare = trace(&[
        request(0, 1, "server/discover", ""),
        result(
            1,
            1,
            r#","supportedVersions":["2026-07-28"],"capabilities":{}"#,
        ),
    ]);
    assert_eq!(findings_for(DISCOVER, &bare).len(), 1);
    assert_eq!(findings_for(RESULTS, &bare).len(), 1);

    let named = trace(&[
        request(0, 1, "server/discover", ""),
        result(
            1,
            1,
            &format!(r#","supportedVersions":["2026-07-28"],"capabilities":{{}},{SERVER_INFO}"#),
        ),
    ]);
    assert_eq!(findings_for(DISCOVER, &named), Vec::<String>::new());
    assert_eq!(findings_for(RESULTS, &named), Vec::<String>::new());
}
