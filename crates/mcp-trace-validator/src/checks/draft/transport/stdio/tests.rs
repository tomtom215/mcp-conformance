// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the stdio binding's cancellation clauses.
//!
//! The fixtures use the shared testkit, whose records are labelled
//! `streamable-http`. That is deliberate rather than sloppy: neither check
//! filters on transport, because the rule is about the cancellation
//! *notification* and a capture is free to carry one on any binding. What
//! separates these from `transport.no-messages-after-cancellation` is the
//! signal they read — a notification here, a stream close there — not the
//! transport label.

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const REFERENCES: &str = "transport.cancel-notification-references-request";
const AFTER: &str = "transport.no-messages-after-cancel-notification";

/// A client `notifications/cancelled` whose `params` are `params`.
fn cancel(seq: u64, params: &str) -> String {
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","method":"notifications/cancelled"{params}}}"#),
    )
}

/// A client request `id` opting into progress token `token` when given.
fn request(seq: u64, id: u64, token: Option<&str>) -> String {
    let meta = token.map_or_else(String::new, |token| {
        format!(r#","params":{{"_meta":{{"progressToken":"{token}"}}}}"#)
    });
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call"{meta}}}"#),
    )
}

/// A server `notifications/progress` for `token`.
fn progress(seq: u64, token: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"notifications/progress","params":{{"progressToken":"{token}","progress":1}}}}"#
        ),
    )
}

#[test]
fn a_cancellation_must_name_a_request() {
    for params in [
        "",
        r#","params":{}"#,
        r#","params":{"reason":"user"}"#,
        r#","params":{"requestId":null}"#,
    ] {
        let findings = findings_for(REFERENCES, &cancel(0, params));
        assert_eq!(findings.len(), 1, "params {params:?}: {findings:?}");
        assert!(findings[0].contains("requestId"), "{findings:?}");
    }
}

#[test]
fn a_cancellation_naming_a_request_conforms() {
    for params in [
        r#","params":{"requestId":1}"#,
        r#","params":{"requestId":"abc"}"#,
        r#","params":{"requestId":1,"reason":"user"}"#,
    ] {
        assert!(
            findings_for(REFERENCES, &cancel(0, params)).is_empty(),
            "params {params:?}"
        );
    }
}

#[test]
fn only_the_clients_cancellations_are_judged() {
    // The clause binds the client. A server-sent notification of the same name
    // is not the client cancelling anything.
    let from_server = server(0, r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#);
    assert!(findings_for(REFERENCES, &from_server).is_empty());

    // Nor is a message with an id, which is a request rather than a notification.
    let as_request = client(
        0,
        r#"{"jsonrpc":"2.0","id":1,"method":"notifications/cancelled"}"#,
    );
    assert!(findings_for(REFERENCES, &as_request).is_empty());
}

#[test]
fn other_notifications_are_not_this_clauses_business() {
    let unrelated = client(0, r#"{"jsonrpc":"2.0","method":"notifications/progress"}"#);
    assert!(findings_for(REFERENCES, &unrelated).is_empty());
}

#[test]
fn answering_a_cancelled_request_is_the_violation() {
    let session = trace(&[
        request(0, 1, None),
        cancel(1, r#","params":{"requestId":1}"#),
        server(
            2,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    let findings = findings_for(AFTER, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("a response"), "{findings:?}");
    assert!(findings[0].contains("seq 1"), "{findings:?}");

    // An error is a response too.
    let errored = session.replace(
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"x"}}"#,
    );
    assert_eq!(findings_for(AFTER, &errored).len(), 1);
}

#[test]
fn answering_before_the_cancellation_conforms() {
    // The server had already finished; the late notification cancels nothing.
    let session = trace(&[
        request(0, 1, None),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
        cancel(2, r#","params":{"requestId":1}"#),
    ]);
    assert!(findings_for(AFTER, &session).is_empty());
}

#[test]
fn a_different_request_may_still_be_answered() {
    let session = trace(&[
        request(0, 1, None),
        request(1, 2, None),
        cancel(2, r#","params":{"requestId":1}"#),
        server(
            3,
            r#"{"jsonrpc":"2.0","id":2,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert!(findings_for(AFTER, &session).is_empty());
}

#[test]
fn progress_for_a_cancelled_request_is_a_further_message() {
    // Correlated by the token the request opted into, not by JSON-RPC id — the
    // only link a progress notification carries.
    let session = trace(&[
        request(0, 1, Some("tok-1")),
        cancel(1, r#","params":{"requestId":1}"#),
        progress(2, "tok-1"),
    ]);
    let findings = findings_for(AFTER, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("progress"), "{findings:?}");

    // A token belonging to some other request is not this request's progress.
    let other = trace(&[
        request(0, 1, Some("tok-1")),
        request(1, 2, Some("tok-2")),
        cancel(2, r#","params":{"requestId":1}"#),
        progress(3, "tok-2"),
    ]);
    assert!(findings_for(AFTER, &other).is_empty());

    // And progress that arrived before the cancellation is not "further".
    let early = trace(&[
        request(0, 1, Some("tok-1")),
        progress(1, "tok-1"),
        cancel(2, r#","params":{"requestId":1}"#),
    ]);
    assert!(findings_for(AFTER, &early).is_empty());
}

#[test]
fn a_cancellation_for_a_request_the_recording_never_saw_still_binds() {
    // The prohibition is on the id, not on this trace having witnessed the
    // request open — a recording that starts mid-session must not excuse the
    // server from honouring a cancellation it received.
    let session = trace(&[
        cancel(0, r#","params":{"requestId":9}"#),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":9,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert_eq!(findings_for(AFTER, &session).len(), 1);
}

#[test]
fn a_session_without_cancellation_is_not_judged() {
    let session = trace(&[
        request(0, 1, Some("tok-1")),
        progress(1, "tok-1"),
        server(
            2,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert!(findings_for(AFTER, &session).is_empty());
    assert!(findings_for(REFERENCES, &session).is_empty());
}

#[test]
fn a_cancellation_naming_no_request_cancels_nothing_here() {
    // TRAN-123 reports the malformed notification; this check must not invent a
    // cancellation out of it and start failing the server as well.
    let session = trace(&[
        request(0, 1, None),
        cancel(1, r#","params":{}"#),
        server(
            2,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert!(findings_for(AFTER, &session).is_empty());
}
