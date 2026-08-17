// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `server/discover` clauses.
//!
//! Both checks turn on an antecedent rather than on a single field, so the
//! interesting cases are the ones where the antecedent is *not* met: a session
//! with no probe, an error that is not `-32601`, a legacy-only client. Each is
//! pinned here, because a check that fires on those would report conforming
//! sessions — and the corpus, which carries one violation per requirement,
//! cannot see it.

use crate::checks::draft::testkit::{META, client, error, findings_for, server, trace};

const IMPLEMENTED: &str = "discover.implemented";
const PROBE_FIRST: &str = "discover.dual-era-probe-first";

/// A client `server/discover` request with id `1`.
fn probe(seq: u64) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{{META}}}}}"#),
    )
}

/// A client request for `method`, carrying the modern `_meta` when `modern`.
fn request(seq: u64, id: u64, method: &str, modern: bool) -> String {
    let params = if modern {
        format!(r#","params":{{{META}}}"#)
    } else {
        String::new()
    };
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}"{params}}}"#),
    )
}

#[test]
fn a_session_that_never_probes_is_not_judged() {
    // The clause binds the answer to a probe; with no probe there is no evidence
    // either way, and reporting "the server did not implement discovery" from
    // silence would fail every session that simply never asked.
    let no_probe = trace(&[
        request(0, 1, "tools/list", true),
        server(1, r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#),
    ]);
    assert!(findings_for(IMPLEMENTED, &no_probe).is_empty());
}

#[test]
fn method_not_found_for_the_probe_is_the_violation() {
    let refused = trace(&[probe(0), error(1, "1", -32601)]);
    let findings = findings_for(IMPLEMENTED, &refused);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("-32601"), "{findings:?}");
}

#[test]
fn an_answered_probe_conforms() {
    let answered = trace(&[
        probe(0),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","supportedVersions":["2026-07-28"],"capabilities":{}}}"#,
        ),
    ]);
    assert!(findings_for(IMPLEMENTED, &answered).is_empty());
}

#[test]
fn errors_other_than_method_not_found_do_not_prove_absence() {
    // -32022 (unsupported version) and -32602 are answers *from* an
    // implementation: the server understood the method and refused the call.
    for code in [-32022, -32602, -32603, -32600] {
        let answered = trace(&[probe(0), error(1, "1", code)]);
        assert!(
            findings_for(IMPLEMENTED, &answered).is_empty(),
            "code {code} should not read as an unimplemented method"
        );
    }
}

#[test]
fn the_error_must_answer_the_probe_by_id() {
    // A -32601 for some other outstanding request says nothing about discovery.
    let unrelated = trace(&[
        probe(0),
        request(1, 7, "tools/call", true),
        error(2, "7", -32601),
    ]);
    assert!(findings_for(IMPLEMENTED, &unrelated).is_empty());
}

#[test]
fn only_a_server_sent_error_answers_a_probe() {
    // A client-sent message carrying the probe's id is not the server's answer;
    // without the direction filter this would report the server for it.
    let client_error = trace(&[
        probe(0),
        client(1, r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}"#),
    ]);
    assert!(findings_for(IMPLEMENTED, &client_error).is_empty());
}

#[test]
fn only_a_client_sent_request_counts_as_a_probe() {
    // The probe is a client request. A server-sent `server/discover` (which the
    // revision has no notion of) must not register one, or the -32601 below
    // would be attributed to a probe that never happened.
    let server_side = trace(&[
        server(0, r#"{"jsonrpc":"2.0","id":1,"method":"server/discover"}"#),
        error(1, "1", -32601),
    ]);
    assert!(findings_for(IMPLEMENTED, &server_side).is_empty());
}

#[test]
fn a_modern_only_client_is_not_asked_to_probe() {
    // No `initialize` anywhere: the antecedent ("supports both … eras") is not
    // met, and the clause does not bind. Probing is RECOMMENDED for these
    // clients, not SHOULD — a different sentence, on a different page.
    let modern = trace(&[
        request(0, 1, "tools/list", true),
        server(1, r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#),
    ]);
    assert!(findings_for(PROBE_FIRST, &modern).is_empty());
}

#[test]
fn a_legacy_only_client_is_not_reported() {
    // `initialize` with no modern `_meta` anywhere is a legacy client, whose
    // session is simply not this clause's subject. Reporting it would make every
    // legacy capture read as a client defect.
    let legacy = trace(&[
        request(0, 1, "initialize", false),
        server(1, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
        request(2, 2, "tools/list", false),
    ]);
    assert!(findings_for(PROBE_FIRST, &legacy).is_empty());
}

#[test]
fn a_dual_era_client_that_skips_the_probe_is_reported_at_its_first_request() {
    let skipped = trace(&[
        request(0, 1, "tools/call", true),
        error(1, "1", -32600),
        request(2, 2, "initialize", false),
    ]);
    let findings = findings_for(PROBE_FIRST, &skipped);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("tools/call"), "{findings:?}");
}

#[test]
fn a_dual_era_client_that_probes_first_conforms() {
    let probed = trace(&[
        probe(0),
        error(1, "1", -32601),
        request(2, 2, "initialize", false),
        server(3, r#"{"jsonrpc":"2.0","id":2,"result":{}}"#),
    ]);
    assert!(findings_for(PROBE_FIRST, &probed).is_empty());
}

#[test]
fn the_probe_may_follow_notifications_because_only_requests_count() {
    // "before sending any other request" — a notification is not a request, so
    // one ahead of the probe does not move "first".
    let notified = trace(&[
        client(0, r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#),
        probe(1),
        error(2, "1", -32601),
        request(3, 2, "initialize", false),
    ]);
    assert!(findings_for(PROBE_FIRST, &notified).is_empty());
}

#[test]
fn initialize_first_with_a_later_modern_request_is_reported() {
    // The fallback done backwards: handshake first, modern retry after.
    let backwards = trace(&[
        request(0, 1, "initialize", false),
        server(1, r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"x"}}"#),
        request(2, 2, "tools/list", true),
    ]);
    let findings = findings_for(PROBE_FIRST, &backwards);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("initialize"), "{findings:?}");
}

#[test]
fn a_session_with_no_client_requests_is_not_judged() {
    let quiet = trace(&[server(0, r#"{"jsonrpc":"2.0","method":"notifications/message"}"#)]);
    assert!(findings_for(PROBE_FIRST, &quiet).is_empty());
    assert!(findings_for(IMPLEMENTED, &quiet).is_empty());
}
