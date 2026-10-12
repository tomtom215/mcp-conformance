// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the server-cancellation rule.

use crate::checks::stateless::testkit::{client, findings_for, server, trace};

const CHECK: &str = "cancellation.server-cancels-only-subscriptions";

/// A client request `id` (JSON text) for `method`.
fn request(seq: u64, id: &str, method: &str) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}"}}"#),
    )
}

/// A server `notifications/cancelled` whose `params` are `params`.
fn cancel(seq: u64, params: &str) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","method":"notifications/cancelled","params":{params}}}"#),
    )
}

#[test]
fn ending_a_subscription_is_the_permitted_cancellation() {
    let findings = findings_for(
        CHECK,
        &trace(&[
            request(0, r#""listen-1""#, "subscriptions/listen"),
            cancel(1, r#"{"requestId":"listen-1","reason":"shutting down"}"#),
        ]),
    );
    assert_eq!(findings, Vec::<String>::new());
}

#[test]
fn cancelling_any_other_request_is_reported() {
    let findings = findings_for(
        CHECK,
        &trace(&[
            request(0, "1", "tools/call"),
            cancel(1, r#"{"requestId":1,"reason":"server busy"}"#),
        ]),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("request 1, a `tools/call`"),
        "{findings:?}"
    );
}

#[test]
fn a_cancellation_naming_nothing_is_reported() {
    for params in ["{}", r#"{"requestId":null}"#, r#"{"reason":"x"}"#] {
        let findings = findings_for(CHECK, &cancel(0, params));
        assert_eq!(findings.len(), 1, "{params}: {findings:?}");
        assert!(
            findings[0].contains("no `params.requestId`"),
            "{findings:?}"
        );
    }
}

#[test]
fn the_latest_request_with_an_id_is_the_one_cancelled() {
    // Id 7 named a listen first and a `tools/call` after its reuse: cancelling
    // 7 now cancels the call, which a server may not do.
    let findings = findings_for(
        CHECK,
        &trace(&[
            request(0, "7", "subscriptions/listen"),
            server(
                1,
                r#"{"jsonrpc":"2.0","id":7,"result":{"resultType":"complete"}}"#,
            ),
            request(2, "7", "tools/call"),
            cancel(3, r#"{"requestId":7}"#),
        ]),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    // And the other way round is the teardown.
    let findings = findings_for(
        CHECK,
        &trace(&[
            request(0, "7", "tools/call"),
            server(
                1,
                r#"{"jsonrpc":"2.0","id":7,"result":{"resultType":"complete"}}"#,
            ),
            request(2, "7", "subscriptions/listen"),
            cancel(3, r#"{"requestId":7}"#),
        ]),
    );
    assert_eq!(findings, Vec::<String>::new());
}

#[test]
fn ids_are_matched_as_json_values_and_unknown_ids_are_not_judged() {
    // The string "1" is not the integer 1: the listen is not what was cancelled,
    // and no request the trace shows is — the capture may have begun mid-session.
    let findings = findings_for(
        CHECK,
        &trace(&[
            request(0, r#""1""#, "subscriptions/listen"),
            cancel(1, r#"{"requestId":1}"#),
        ]),
    );
    assert_eq!(findings, Vec::<String>::new());
}

#[test]
fn client_cancellations_and_server_requests_are_not_this_rule() {
    let findings = findings_for(
        CHECK,
        &trace(&[
            request(0, "1", "tools/call"),
            client(
                1,
                r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            ),
            // Carrying an id makes it a request, which is TRAN-066's to report.
            server(
                2,
                r#"{"jsonrpc":"2.0","id":9,"method":"notifications/cancelled","params":{"requestId":1}}"#,
            ),
        ]),
    );
    assert_eq!(findings, Vec::<String>::new());
}
