// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the response-stream clauses: what may travel on the stream a POST
//! opened, and the exact moment a close stops being "during".

use crate::checks::draft::testkit::{META, client, findings_for, post, server, trace};

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
fn only_a_client_sent_response_over_this_transport_is_reported() {
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

    // A *server* response is the normal case and must not be reported — the
    // direction filter and the transport filter are both load-bearing.
    let mut lines = call(0);
    lines.push(server(
        2,
        r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
    ));
    assert!(findings_for(check, &trace(&lines)).is_empty());

    // The same client response over stdio belongs to the stdio clauses.
    let stdio = [
        r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":9,"result":{}}}"#,
    ]
    .join("\n");
    assert!(findings_for(check, &stdio).is_empty());
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
