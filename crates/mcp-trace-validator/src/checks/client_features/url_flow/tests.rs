// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `2025-11-25` URL mode bookkeeping checks.

use super::super::testkit::{
    ask, client, findings_for, reply_error, server, session, subjects_for,
};

const URL: &str = r#"{"mode":"url","message":"Connect","url":"https://example.com/connect","elicitationId":"e1"}"#;
const COMPLETE: &str = "elicitation.complete-notification-id";
const LISTS: &str = "elicitation.url-required-error-lists";
const URL_MODE: &str = "elicitation.url-required-error-url-mode";

/// The server's completion notification with `params`.
fn complete(seq: u64, params: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"notifications/elicitation/complete","params":{params}}}"#
        ),
    )
}

/// A URL mode elicitation establishing `e1`, at seq 3.
fn url_ask() -> String {
    ask(3, 1, "elicitation/create", URL)
}

/// A session whose only server message after the handshake is a `-32042`
/// error carrying `data` (a `,"data":…` member, or nothing).
fn url_required(data: &str) -> String {
    session(
        "{}",
        &[server(
            3,
            &format!(r#"{{"jsonrpc":"2.0","id":9,"error":{{"code":-32042,"message":"m"{data}}}}}"#),
        )],
    )
}

#[test]
fn completion_notifications_name_an_established_id() {
    let clean = session("{}", &[url_ask(), complete(4, r#"{"elicitationId":"e1"}"#)]);
    assert_eq!(findings_for(COMPLETE, &clean), Vec::<String>::new());
    let form_with_id = ask(
        3,
        1,
        "elicitation/create",
        r#"{"message":"m","elicitationId":"e1"}"#,
    );
    for (lines, reason) in [
        (
            vec![url_ask(), complete(4, r#"{"elicitationId":"e2"}"#)],
            r#""e2""#,
        ),
        (
            vec![
                complete(3, r#"{"elicitationId":"e1"}"#),
                ask(4, 1, "elicitation/create", URL),
            ],
            r#""e1""#,
        ),
        (vec![url_ask(), complete(4, "{}")], "no string"),
        (
            vec![form_with_id, complete(4, r#"{"elicitationId":"e1"}"#)],
            r#""e1""#,
        ),
    ] {
        let found = findings_for(COMPLETE, &session("{}", &lines));
        assert_eq!(found.len(), 1, "{lines:?}");
        assert!(found[0].contains(reason), "{found:?}");
    }
}

#[test]
fn an_error_can_establish_an_id_and_only_the_server_notifies() {
    let error = server(
        3,
        r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32042,"message":"m","data":{"elicitations":[{"mode":"url","elicitationId":"e9","url":"https://x","message":"m"}]}}}"#,
    );
    let completed = session("{}", &[error, complete(4, r#"{"elicitationId":"e9"}"#)]);
    assert_eq!(findings_for(COMPLETE, &completed), Vec::<String>::new());
    let from_client = client(
        3,
        r#"{"jsonrpc":"2.0","method":"notifications/elicitation/complete","params":{}}"#,
    );
    assert_eq!(subjects_for(COMPLETE, &session("{}", &[from_client])), 0);
}

#[test]
fn url_required_errors_list_url_mode_elicitations() {
    let good = r#","data":{"elicitations":[{"mode":"url","elicitationId":"e","url":"https://x","message":"m"}]}"#;
    let mixed =
        r#","data":{"elicitations":[{"mode":"form","elicitationId":"e"},{"mode":"url"},{}]}"#;
    for (check, data, subjects, expected) in [
        (LISTS, good, 1, 0),
        (LISTS, "", 1, 1),
        (LISTS, r#","data":{"elicitations":[]}"#, 1, 1),
        (LISTS, r#","data":{"elicitations":{}}"#, 1, 1),
        (URL_MODE, good, 1, 0),
        (URL_MODE, "", 0, 0),
        (URL_MODE, mixed, 3, 3),
    ] {
        let trace = url_required(data);
        assert_eq!(
            findings_for(check, &trace).len(),
            expected,
            "{check} {data}"
        );
        assert_eq!(subjects_for(check, &trace), subjects, "{check} {data}");
    }
    let found = findings_for(URL_MODE, &url_required(r#","data":{"elicitations":[{}]}"#));
    assert!(
        found[0].ends_with(r#"is not `mode: "url"` and has no `elicitationId`"#),
        "{found:?}"
    );
}

#[test]
fn only_the_servers_minus_32042_is_judged() {
    let other = session(
        "{}",
        &[server(
            3,
            r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32602,"message":"m"}}"#,
        )],
    );
    assert_eq!(subjects_for(LISTS, &other), 0);
    let client_error = session("{}", &[reply_error(3, 1, -32042)]);
    assert_eq!(subjects_for(LISTS, &client_error), 0);
}
