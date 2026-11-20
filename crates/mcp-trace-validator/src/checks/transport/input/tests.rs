// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::super::testkit::{Session, ping};

const CHECK: &str = "transport.unaccepted-input-error-status";

const NOTIFICATION: &str = r#"{"jsonrpc":"2.0","method":"notifications/x"}"#;
const RESPONSE: &str = r#"{"jsonrpc":"2.0","id":4,"result":{}}"#;
/// The refusal body the page describes: a JSON-RPC error with no `id`.
const DECLINED: &str = r#"{"jsonrpc":"2.0","error":{"code":-32600,"message":"no"}}"#;
const DECLINED_NULL_ID: &str =
    r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"no"}}"#;

fn posted(payload: &str, status: u16) -> Session {
    Session::default().post("{}", payload, status)
}

#[test]
fn an_input_refused_with_an_error_status_passes() {
    for status in [400, 404, 500] {
        assert_eq!(posted(NOTIFICATION, status).clean(CHECK), 1, "{status}");
    }
    assert_eq!(posted(RESPONSE, 400).clean(CHECK), 1);
    // The body the page permits changes nothing.
    assert_eq!(posted(NOTIFICATION, 400).answers(DECLINED).clean(CHECK), 1);
}

#[test]
fn an_accepted_input_is_not_a_subject() {
    for status in [202, 200, 399] {
        assert_eq!(posted(NOTIFICATION, status).clean(CHECK), 0, "{status}");
    }
}

#[test]
fn an_input_declined_in_band_under_a_non_error_status_is_a_violation() {
    let finding = posted(NOTIFICATION, 202).answers(DECLINED).one(CHECK);
    assert!(finding.contains("JSON-RPC notification"), "{finding}");
    assert!(finding.contains("under HTTP 202"), "{finding}");
    let finding = posted(RESPONSE, 200).answers(DECLINED_NULL_ID).one(CHECK);
    assert!(finding.contains("JSON-RPC response"), "{finding}");
    assert!(finding.contains("under HTTP 200"), "{finding}");
    // 399 is the last status that is not an error.
    posted(NOTIFICATION, 399).answers(DECLINED).one(CHECK);
}

#[test]
fn only_an_id_less_error_straight_after_the_status_declines() {
    // An error naming an id answers some request, not this input.
    let with_id = r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32600,"message":"no"}}"#;
    assert_eq!(posted(NOTIFICATION, 202).answers(with_id).clean(CHECK), 0);
    // A body whose `error` is not an error object is not JSON-RPC's.
    let not_an_error = r#"{"error":"invalid_request"}"#;
    assert_eq!(
        posted(NOTIFICATION, 202).answers(not_an_error).clean(CHECK),
        0
    );
    // A notification is not a refusal.
    assert_eq!(
        posted(NOTIFICATION, 202).answers(NOTIFICATION).clean(CHECK),
        0
    );
    // The client's next message is not the server's body.
    assert_eq!(posted(NOTIFICATION, 202).sends(DECLINED).clean(CHECK), 0);
    // Not straight after: another event came between.
    let later = posted(NOTIFICATION, 202)
        .answers(NOTIFICATION)
        .answers(DECLINED);
    assert_eq!(later.clean(CHECK), 0);
}

#[test]
fn only_a_posted_notification_or_response_is_judged() {
    // A request has its own clauses, whatever its status.
    assert_eq!(posted(&ping(1), 400).clean(CHECK), 0);
    assert_eq!(posted(&ping(1), 200).answers(DECLINED).clean(CHECK), 0);
    // A body that is not a JSON-RPC message is outside the quote.
    assert_eq!(posted("[1,2]", 400).clean(CHECK), 0);
    assert_eq!(posted("[1,2]", 202).answers(DECLINED).clean(CHECK), 0);
    // A request with no body the trace holds carried nothing to judge.
    assert_eq!(Session::default().bare("POST", "{}", 400).clean(CHECK), 0);
    assert_eq!(Session::default().bare("GET", "{}", 400).clean(CHECK), 0);
}
