// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the progress-pattern checks.

use crate::checks::stateless::testkit::{client, findings_for, server, trace};

const TYPE: &str = "progress.token-type";
const UNIQUE: &str = "progress.token-unique-in-flight";
const INCREASES: &str = "progress.value-increases";
const AFTER: &str = "progress.no-progress-after-completion";

/// A client `tools/call` with id `id`, opting into progress with `token` (JSON
/// text) when one is given.
fn request(seq: u64, id: u64, token: Option<&str>) -> String {
    let meta = token.map_or_else(String::new, |token| {
        format!(r#","params":{{"_meta":{{"progressToken":{token}}}}}"#)
    });
    client(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call"{meta}}}"#),
    )
}

/// A server `notifications/progress` for `token` (JSON text) at `progress`.
fn progress(seq: u64, token: &str, progress: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","method":"notifications/progress","params":{{"progressToken":{token},"progress":{progress}}}}}"#
        ),
    )
}

/// The server's answer to request `id`.
fn answer(seq: u64, id: u64) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete"}}}}"#),
    )
}

/// `lines` as a stdio recording; the testkit labels its records Streamable HTTP.
fn stdio(lines: &[String]) -> String {
    trace(lines).replace(r#""transport":"streamable-http""#, r#""transport":"stdio""#)
}

#[test]
fn a_token_is_a_string_or_an_integer() {
    for token in [r#""abc""#, "7", "-3"] {
        assert_eq!(
            findings_for(TYPE, &request(0, 1, Some(token))),
            Vec::<String>::new(),
            "{token}"
        );
    }
    for token in ["1.5", "true", "null", r#"{"a":1}"#, "[1]"] {
        let findings = findings_for(TYPE, &request(0, 1, Some(token)));
        assert_eq!(findings.len(), 1, "{token}: {findings:?}");
        assert!(
            findings[0].contains("not a string or integer"),
            "{findings:?}"
        );
    }
    // A notification is not a request opting in.
    let notification = client(
        0,
        r#"{"jsonrpc":"2.0","method":"notifications/x","params":{"_meta":{"progressToken":1.5}}}"#,
    );
    assert_eq!(findings_for(TYPE, &notification), Vec::<String>::new());
}

#[test]
fn a_token_is_unique_only_while_its_request_is_active() {
    let reused_in_flight = trace(&[request(0, 1, Some(r#""t""#)), request(1, 2, Some(r#""t""#))]);
    let findings = findings_for(UNIQUE, &reused_in_flight);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("seq 0"), "{findings:?}");

    // Reused after the first request was answered: legal.
    let reused_after = trace(&[
        request(0, 1, Some(r#""t""#)),
        answer(1, 1),
        request(2, 2, Some(r#""t""#)),
    ]);
    assert_eq!(findings_for(UNIQUE, &reused_after), Vec::<String>::new());

    // The string "1" and the integer 1 are different tokens.
    let distinct = trace(&[request(0, 1, Some(r#""1""#)), request(1, 2, Some("1"))]);
    assert_eq!(findings_for(UNIQUE, &distinct), Vec::<String>::new());
}

#[test]
fn progress_must_strictly_increase_per_token() {
    let rising = trace(&[
        request(0, 1, Some(r#""t""#)),
        progress(1, r#""t""#, "0.2"),
        progress(2, r#""t""#, "0.6"),
        progress(3, r#""t""#, "1"),
    ]);
    assert_eq!(findings_for(INCREASES, &rising), Vec::<String>::new());

    for (second, why) in [("10", "decreased"), ("50", "repeated")] {
        let falling = trace(&[
            request(0, 1, Some(r#""t""#)),
            progress(1, r#""t""#, "50"),
            progress(2, r#""t""#, second),
        ]);
        let findings = findings_for(INCREASES, &falling);
        assert_eq!(findings.len(), 1, "{why}: {findings:?}");
        assert!(
            findings[0].contains(&format!("from 50 to {second}")),
            "{findings:?}"
        );
    }
}

#[test]
fn a_new_request_restarts_its_tokens_sequence_and_tokens_are_independent() {
    let restarted = trace(&[
        request(0, 1, Some(r#""t""#)),
        progress(1, r#""t""#, "5"),
        answer(2, 1),
        request(3, 2, Some(r#""t""#)),
        progress(4, r#""t""#, "1"),
        // A second token's sequence is its own.
        request(5, 3, Some(r#""u""#)),
        progress(6, r#""u""#, "0"),
    ]);
    assert_eq!(findings_for(INCREASES, &restarted), Vec::<String>::new());
}

#[test]
fn progress_after_the_answer_is_reported_on_stdio() {
    let late = stdio(&[
        request(0, 1, Some(r#""t""#)),
        progress(1, r#""t""#, "1"),
        answer(2, 1),
        progress(3, r#""t""#, "2"),
    ]);
    let findings = findings_for(AFTER, &late);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("(seq 0) was answered at seq 2"),
        "{findings:?}"
    );
}

#[test]
fn progress_before_the_answer_or_for_a_reused_token_is_not() {
    let reused = stdio(&[
        request(0, 1, Some(r#""t""#)),
        progress(1, r#""t""#, "1"),
        answer(2, 1),
        request(3, 2, Some(r#""t""#)),
        progress(4, r#""t""#, "1"),
        // Never answered in the recording: still in progress.
        request(5, 3, Some(r#""u""#)),
        progress(6, r#""u""#, "1"),
    ]);
    assert_eq!(findings_for(AFTER, &reused), Vec::<String>::new());
}

#[test]
fn unknown_tokens_and_http_recordings_are_not_judged() {
    // A token no recorded request carried: the capture may have begun late.
    let unknown = stdio(&[answer(0, 1), progress(1, r#""t""#, "1")]);
    assert_eq!(findings_for(AFTER, &unknown), Vec::<String>::new());
    // Over Streamable HTTP the two may ride different streams, whose recorded
    // order is not their sent order.
    let http = trace(&[
        request(0, 1, Some(r#""t""#)),
        answer(1, 1),
        progress(2, r#""t""#, "1"),
    ]);
    assert_eq!(findings_for(AFTER, &http), Vec::<String>::new());
}
