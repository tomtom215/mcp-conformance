// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the ping rule.

use crate::checks::basic_utilities::testkit::{request, result, run, session};

const CHECK: &str = "ping.empty-result";

/// A `ping` `id` from the client when `from_client`, answered with `answer`
/// (a whole JSON-RPC response payload fragment after the id).
fn ping(from_client: bool, id: &str, answer: &str) -> String {
    let asked = request(3, from_client, id, "ping", "{}");
    let payload = format!(r#"{{"jsonrpc":"2.0","id":{id},{answer}}}"#);
    let answered = if from_client {
        crate::checks::basic_utilities::testkit::server(4, &payload)
    } else {
        crate::checks::basic_utilities::testkit::client(4, &payload)
    };
    session(&[asked, answered])
}

#[test]
fn an_empty_result_conforms_in_either_direction() {
    for from_client in [true, false] {
        let (findings, subjects) = run(CHECK, &ping(from_client, "7", r#""result":{}"#));
        assert_eq!(findings, Vec::<String>::new(), "from_client={from_client}");
        assert_eq!(subjects, 1);
    }
}

#[test]
fn meta_is_the_one_member_an_empty_result_may_carry() {
    let trace = ping(
        true,
        "7",
        r#""result":{"_meta":{"io.example/trace":"abc"}}"#,
    );
    assert_eq!(run(CHECK, &trace), (Vec::<String>::new(), 1));
}

#[test]
fn a_result_with_content_is_reported() {
    for from_client in [true, false] {
        let trace = ping(
            from_client,
            r#""p-1""#,
            r#""result":{"pong":true,"_meta":{}}"#,
        );
        let (findings, subjects) = run(CHECK, &trace);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("carrying `pong`"), "{findings:?}");
        assert!(!findings[0].contains("_meta"), "{findings:?}");
        assert_eq!(subjects, 1);
    }
}

#[test]
fn an_error_answer_is_reported_with_its_code() {
    let trace = ping(true, "7", r#""error":{"code":-32603,"message":"busy"}"#);
    let findings = run(CHECK, &trace).0;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("error (-32603)"), "{findings:?}");
}

#[test]
fn a_result_that_is_no_object_is_reported() {
    let trace = ping(true, "7", r#""result":[]"#);
    let findings = run(CHECK, &trace).0;
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("not an empty object"), "{findings:?}");
}

#[test]
fn an_unanswered_ping_and_other_exchanges_are_not_subjects() {
    // The recording may end before the answer: nothing is judged.
    let unanswered = session(&[request(3, true, "7", "ping", "{}")]);
    assert_eq!(run(CHECK, &unanswered), (Vec::<String>::new(), 0));
    // A non-empty result to anything but a ping is not this rule's.
    let listed = session(&[
        request(3, true, "7", "tools/list", "{}"),
        result(4, false, "7", r#"{"tools":[]}"#),
    ]);
    assert_eq!(run(CHECK, &listed), (Vec::<String>::new(), 0));
}
