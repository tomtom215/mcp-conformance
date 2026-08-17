// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the per-request logging clauses.
//!
//! `logging/setLevel` is gone, so the level rides `_meta` and the checks turn on
//! whether *any* request in the session asked for logs. The case worth pinning
//! is a session that did ask: attributing a notification to one request is not
//! possible on a shared channel, so the check must stop there rather than guess.

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const REQUESTED: &str = "logging.level-requested";
const NOT_ON_SUB: &str = "logging.not-on-subscription";
const INVALID: &str = "logging.invalid-level-rejected";

/// A client request whose `_meta` sets `level`, or none when `level` is empty.
fn request(seq: u64, id: u64, level: &str) -> String {
    let meta = if level.is_empty() {
        String::new()
    } else {
        format!(r#","io.modelcontextprotocol/logLevel":{level}"#)
    };
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28"{meta}}}}}}}"#
        ),
    )
}

/// A `notifications/message`, optionally tagged with a subscription id.
fn log(seq: u64, subscription: Option<u64>) -> String {
    let tag = subscription
        .map(|id| format!(r#","_meta":{{"io.modelcontextprotocol/subscriptionId":{id}}}"#))
        .unwrap_or_default();
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"notifications/message","params":{{"level":"info","data":"x"{tag}}}}}"#
        ),
    )
}

#[test]
fn a_log_with_no_request_asking_for_one_is_reported() {
    let session = trace(&[request(0, 1, ""), log(1, None)]);
    let findings = findings_for(REQUESTED, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn a_session_that_asked_for_logs_may_receive_them() {
    let session = trace(&[request(0, 1, r#""info""#), log(1, None)]);
    assert!(findings_for(REQUESTED, &session).is_empty());
}

#[test]
fn a_session_with_no_logs_at_all_is_not_reported() {
    let session = trace(&[request(0, 1, "")]);
    assert!(findings_for(REQUESTED, &session).is_empty());
    assert!(findings_for(NOT_ON_SUB, &session).is_empty());
}

#[test]
fn a_log_tagged_with_a_subscription_is_off_its_own_stream() {
    let session = trace(&[request(0, 1, r#""info""#), log(1, Some(7))]);
    let findings = findings_for(NOT_ON_SUB, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("request-scoped"), "{findings:?}");

    let untagged = trace(&[request(0, 1, r#""info""#), log(1, None)]);
    assert!(findings_for(NOT_ON_SUB, &untagged).is_empty());
}

#[test]
fn an_unrecognized_level_must_draw_invalid_params() {
    let answered = |level: &str, answer: &str| {
        trace(&[
            request(0, 1, level),
            server(1, &format!(r#"{{"jsonrpc":"2.0","id":1,{answer}}}"#)),
        ])
    };
    let rejected = r#""error":{"code":-32602,"message":"bad level"}"#;
    let served = r#""result":{"resultType":"complete"}"#;

    for level in [r#""verbose""#, r#""INFO""#, "5", "null"] {
        let findings = findings_for(INVALID, &answered(level, served));
        assert_eq!(findings.len(), 1, "level {level}: {findings:?}");
    }
    assert!(findings_for(INVALID, &answered(r#""verbose""#, rejected)).is_empty());

    // Every RFC 5424 level is recognized, and a served request is fine.
    for level in [
        "debug",
        "info",
        "notice",
        "warning",
        "error",
        "critical",
        "alert",
        "emergency",
    ] {
        let session = answered(&format!(r#""{level}""#), served);
        assert!(findings_for(INVALID, &session).is_empty(), "level {level}");
    }
}

#[test]
fn a_request_that_sets_no_level_is_not_judged_for_its_validity() {
    let session = trace(&[
        request(0, 1, ""),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert!(findings_for(INVALID, &session).is_empty());
}
