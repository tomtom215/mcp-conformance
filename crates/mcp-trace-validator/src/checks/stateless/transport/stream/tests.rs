// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the response-stream clauses: what may travel on the stream a POST
//! opened, and the exact moment a close stops being "during".

use crate::checks::stateless::testkit::{META, client, findings_for, post, server, trace};

/// A `tools/call` POST and its request message, framing whatever follows.
fn call(seq: u64) -> Vec<String> {
    vec![
        post(seq, r#"{"mcp-method":"tools/call","mcp-name":"echo"}"#),
        client(
            seq + 1,
            &format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"echo",{META}}}}}"#
            ),
        ),
    ]
}

/// A transport-close lifecycle event on Streamable HTTP.
fn close(seq: u64) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"lifecycle","event":"transport-close"}}"#
    )
}

#[test]
fn only_a_client_sent_response_is_reported_on_any_binding() {
    let check = "transport.client-no-responses";

    // The violation: a client POSTing a result.
    let mut lines = call(0);
    lines.push(client(2, r#"{"jsonrpc":"2.0","id":9,"result":{}}"#));
    assert_eq!(findings_for(check, &trace(&lines)).len(), 1);

    // A client *error* response is equally forbidden.
    let mut lines = call(0);
    lines.push(client(
        2,
        r#"{"jsonrpc":"2.0","id":9,"error":{"code":-32000,"message":"x"}}"#,
    ));
    assert_eq!(findings_for(check, &trace(&lines)).len(), 1);

    // A *server* response is the normal case and must not be reported: the
    // direction filter is the one that carries this clause.
    let mut lines = call(0);
    lines.push(server(
        2,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
    ));
    assert!(findings_for(check, &trace(&lines)).is_empty());

    // A client message carrying a result but *no* id is not a response this
    // clause can name — there is nothing for it to answer, so BASE-004 owns it.
    // Reporting it here would mean the three conditions were being read as
    // alternatives rather than as one shape.
    let mut lines = call(0);
    lines.push(client(2, r#"{"jsonrpc":"2.0","result":{}}"#));
    assert!(findings_for(check, &trace(&lines)).is_empty());

    // Nor is a message that carries a result alongside a method.
    let mut lines = call(0);
    lines.push(client(
        2,
        r#"{"jsonrpc":"2.0","id":9,"method":"ping","result":{}}"#,
    ));
    assert!(findings_for(check, &trace(&lines)).is_empty());

    // The same client response over stdio is reported too. Both binding pages
    // state the rule — TRAN-060 for POSTs, TRAN-119 for stdio writes — and it is
    // one rule, because the revision left nothing anywhere for a client response
    // to answer. Scoping this to Streamable HTTP would have made it inspect
    // nothing on a stdio capture and report TRAN-119 as a vacuous pass.
    let stdio = [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":9,"result":{}}}"#,
    ]
    .join("\n");
    assert_eq!(findings_for(check, &stdio).len(), 1);
}

#[test]
fn cancellation_binds_messages_strictly_after_the_close() {
    let check = "transport.no-messages-after-cancellation";
    let answer = server(
        3,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
    );

    // Answered after the close: the request was still outstanding, so this is
    // the violation.
    let mut lines = call(0);
    lines.push(close(2));
    lines.push(answer.clone());
    let findings = findings_for(check, &trace(&lines));
    assert_eq!(findings.len(), 1, "{findings:?}");

    // Answered before the close: nothing is outstanding when it happens.
    let mut lines = call(0);
    lines.push(server(
        2,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
    ));
    lines.push(close(3));
    assert!(findings_for(check, &trace(&lines)).is_empty());

    // A *client* message after the close is not the server sending anything.
    let mut lines = call(0);
    lines.push(close(2));
    lines.push(client(3, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#));
    assert!(
        findings_for(check, &trace(&lines)).is_empty(),
        "the clause binds the server"
    );

    // A server message for an id that was never outstanding is a different
    // defect (BASE-046's), not a message "for" a cancelled request.
    let mut lines = call(0);
    lines.push(close(2));
    lines.push(server(
        3,
        r#"{"jsonrpc":"2.0","id":77,"result":{"resultType":"complete"}}"#,
    ));
    assert!(findings_for(check, &trace(&lines)).is_empty());

    // With no close recorded there is no cancellation to judge against.
    let mut lines = call(0);
    lines.push(answer);
    assert!(findings_for(check, &trace(&lines)).is_empty());
}

#[test]
fn a_close_with_several_requests_in_flight_cancels_none_of_them() {
    let check = "transport.no-messages-after-cancellation";
    let second = client(
        2,
        &format!(r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{{META}}}}}"#),
    );
    let mut lines = call(0);
    lines.push(second);
    lines.push(close(3));
    lines.push(server(
        4,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
    ));
    // Which of the two streams closed is unknown; neither answer is convicted.
    assert!(findings_for(check, &trace(&lines)).is_empty());
}

#[test]
fn a_transport_abort_is_not_a_cancellation() {
    // The capture proxy records an upstream failure — answered 502 by the
    // proxy itself — as an abort. The client closed nothing.
    let check = "transport.no-messages-after-cancellation";
    let mut lines = call(0);
    lines.push(
        r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"lifecycle","event":"transport-abort"}"#
            .to_owned(),
    );
    lines.push(server(
        3,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
    ));
    assert!(findings_for(check, &trace(&lines)).is_empty());
}

#[test]
fn an_event_stream_is_recognized_in_any_case() {
    // RFC 9110 §8.3.1: "The type and subtype tokens are case-insensitive."
    let check = "transport.accel-buffering-header";
    let response = |headers: &str| {
        let mut lines = call(0);
        lines.push(format!(
            r#"{{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{headers}}}"#
        ));
        trace(&lines)
    };
    for media_type in ["text/event-stream", "Text/Event-Stream; charset=utf-8"] {
        let bare = response(&format!(r#"{{"content-type":"{media_type}"}}"#));
        assert_eq!(findings_for(check, &bare).len(), 1, "{media_type}");
        let marked = response(&format!(
            r#"{{"content-type":"{media_type}","x-accel-buffering":"no"}}"#
        ));
        assert!(findings_for(check, &marked).is_empty(), "{media_type}");
    }
    // A JSON response is no stream for a proxy to buffer.
    assert!(findings_for(check, &response(r#"{"content-type":"application/json"}"#)).is_empty());
}

#[test]
fn a_close_straight_after_a_response_is_that_responses_stream_ending() {
    // "The final JSON-RPC response SHOULD terminate the stream": a close
    // recorded next is the answered request's, even with one other request —
    // here a long-lived subscription — still open beside it.
    let check = "transport.no-messages-after-cancellation";
    let listen = client(
        0,
        &format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"subscriptions/listen","params":{{{META}}}}}"#
        ),
    );
    let later = server(
        6,
        r#"{"jsonrpc":"2.0","id":7,"result":{"resultType":"complete"}}"#,
    );
    for answer in [
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"x"}}"#,
    ] {
        let mut lines = vec![listen.clone()];
        lines.extend(call(1));
        lines.push(server(3, answer));
        lines.push(close(4));
        lines.push(later.clone());
        let findings = findings_for(check, &trace(&lines));
        assert!(findings.is_empty(), "{answer}: {findings:?}");

        // With anything recorded between the response and the close, the close
        // is no longer the response's, and the one request open owns it again.
        let mut lines = vec![listen.clone()];
        lines.extend(call(1));
        lines.push(server(3, answer));
        lines.push(server(
            4,
            r#"{"jsonrpc":"2.0","method":"notifications/subscriptions/acknowledged"}"#,
        ));
        lines.push(close(5));
        lines.push(later.clone());
        let findings = findings_for(check, &trace(&lines));
        assert_eq!(findings.len(), 1, "{answer}: {findings:?}");
    }
}
