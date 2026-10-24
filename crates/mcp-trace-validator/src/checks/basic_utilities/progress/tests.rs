// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the progress rules: the token's shape and sequence, and the
//! session-start witness behind the unknown-token rule.

use crate::checks::basic_utilities::testkit::{
    findings_for, handshake, http, notify, request, result, run, session, subjects_for, trace,
};

const TYPE: &str = "progress.token-string-or-integer";
const UNIQUE: &str = "progress.token-unique-among-active-requests";
const INCREASES: &str = "progress.value-strictly-increases";
const ISSUED: &str = "progress.token-issued-in-session";

/// A `tools/call` (from the client) or `roots/list` (from the server) with id
/// `id`, opting into progress with `token` (JSON text) when given.
pub(super) fn ask(seq: u64, from_client: bool, id: &str, token: Option<&str>) -> String {
    let meta = token.map_or_else(String::new, |token| {
        format!(r#","_meta":{{"progressToken":{token}}}"#)
    });
    let (method, params) = if from_client {
        (
            "tools/call",
            format!(r#"{{"name":"export","arguments":{{}}{meta}}}"#),
        )
    } else {
        (
            "roots/list",
            format!("{{{}}}", meta.trim_start_matches(',')),
        )
    };
    request(seq, from_client, id, method, &params)
}

/// A `notifications/progress` for `token` (JSON text) at `progress`, from the
/// client when `from_client`.
pub(super) fn progress(seq: u64, from_client: bool, token: &str, progress: &str) -> String {
    notify(
        seq,
        from_client,
        "notifications/progress",
        &format!(r#"{{"progressToken":{token},"progress":{progress}}}"#),
    )
}

/// The answer to request `id`, from the client when `from_client`.
pub(super) fn answer(seq: u64, from_client: bool, id: &str) -> String {
    let body = if from_client {
        r#"{"roots":[]}"#
    } else {
        r#"{"content":[]}"#
    };
    result(seq, from_client, id, body)
}

#[test]
fn a_token_is_a_string_or_an_integer_whichever_party_asks() {
    for from_client in [true, false] {
        for token in [r#""abc""#, "7", "-3"] {
            let trace = session(&[ask(3, from_client, "2", Some(token))]);
            assert_eq!(run(TYPE, &trace), (Vec::<String>::new(), 1), "{token}");
        }
        for token in ["1.5", "true", "null", r#"{"a":1}"#, "[1]"] {
            let trace = session(&[ask(3, from_client, "2", Some(token))]);
            let findings = findings_for(TYPE, &trace);
            assert_eq!(findings.len(), 1, "{token}: {findings:?}");
            assert!(
                findings[0].contains("not a string or integer"),
                "{findings:?}"
            );
        }
    }
    // A notification carrying a token in its `_meta` is not a request opting in,
    // and a request without a token puts nothing to the test.
    let neither = session(&[
        notify(
            3,
            true,
            "notifications/x",
            r#"{"_meta":{"progressToken":1.5}}"#,
        ),
        ask(4, true, "2", None),
    ]);
    assert_eq!(run(TYPE, &neither), (Vec::<String>::new(), 0));
}

#[test]
fn a_token_is_unique_only_while_its_request_is_active() {
    let reused_in_flight = session(&[
        ask(3, true, "2", Some(r#""t""#)),
        ask(4, true, "3", Some(r#""t""#)),
    ]);
    let (findings, subjects) = run(UNIQUE, &reused_in_flight);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("seq 3"), "{findings:?}");
    assert_eq!(subjects, 2);

    // Reused after the first request was answered: legal.
    let reused_after = session(&[
        ask(3, true, "2", Some(r#""t""#)),
        answer(4, false, "2"),
        ask(5, true, "3", Some(r#""t""#)),
    ]);
    assert_eq!(findings_for(UNIQUE, &reused_after), Vec::<String>::new());

    // The string "1" and the integer 1 are different tokens.
    let distinct = session(&[
        ask(3, true, "2", Some(r#""1""#)),
        ask(4, true, "3", Some("1")),
    ]);
    assert_eq!(findings_for(UNIQUE, &distinct), Vec::<String>::new());

    // The parties' tokens are separate namespaces; the server's reuse of its
    // own is not.
    let server_too = session(&[
        ask(3, true, "2", Some(r#""t""#)),
        ask(4, false, r#""s1""#, Some(r#""t""#)),
        ask(5, false, r#""s2""#, Some(r#""t""#)),
    ]);
    let findings = findings_for(UNIQUE, &server_too);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("server's request at seq 4"),
        "{findings:?}"
    );
}

#[test]
fn progress_must_strictly_increase_per_token_and_party() {
    let rising = session(&[
        ask(3, true, "2", Some(r#""t""#)),
        progress(4, false, r#""t""#, "0.2"),
        progress(5, false, r#""t""#, "0.6"),
        progress(6, false, r#""t""#, "1"),
    ]);
    assert_eq!(run(INCREASES, &rising), (Vec::<String>::new(), 2));

    for (second, why) in [("10", "decreased"), ("50", "repeated")] {
        let falling = session(&[
            ask(3, true, "2", Some(r#""t""#)),
            progress(4, false, r#""t""#, "50"),
            progress(5, false, r#""t""#, second),
        ]);
        let findings = findings_for(INCREASES, &falling);
        assert_eq!(findings.len(), 1, "{why}: {findings:?}");
        assert!(
            findings[0].contains(&format!("from 50 to {second}")),
            "{findings:?}"
        );
    }

    // The client reporting on a server request is judged the same way.
    let client_falls = session(&[
        ask(3, false, r#""s1""#, Some(r#""r""#)),
        progress(4, true, r#""r""#, "2"),
        progress(5, true, r#""r""#, "1"),
    ]);
    assert_eq!(findings_for(INCREASES, &client_falls).len(), 1);
}

#[test]
fn a_new_request_restarts_its_tokens_sequence_and_tokens_are_independent() {
    let restarted = session(&[
        ask(3, true, "2", Some(r#""t""#)),
        progress(4, false, r#""t""#, "5"),
        answer(5, false, "2"),
        ask(6, true, "3", Some(r#""t""#)),
        progress(7, false, r#""t""#, "1"),
        // A second token's sequence is its own, and so is the other party's
        // sequence under the same text.
        ask(8, true, "4", Some(r#""u""#)),
        progress(9, false, r#""u""#, "0"),
        ask(10, false, r#""s1""#, Some(r#""t""#)),
        progress(11, true, r#""t""#, "0"),
        // A non-numeric `progress` is the wire schema's, and skipped.
        progress(12, false, r#""t""#, r#""two""#),
    ]);
    assert_eq!(findings_for(INCREASES, &restarted), Vec::<String>::new());
}

#[test]
fn a_token_no_request_provided_is_reported_once_the_session_start_is_recorded() {
    let ghost = session(&[
        ask(3, true, "2", None),
        progress(4, false, r#""ghost""#, "1"),
        answer(5, false, "2"),
    ]);
    let (findings, subjects) = run(ISSUED, &ghost);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("no request from the client provided"),
        "{findings:?}"
    );
    assert!(findings[0].contains("seq 0"), "{findings:?}");
    assert_eq!(subjects, 1);

    // The same token issued by the client beforehand: provided.
    let provided = session(&[
        ask(3, true, "2", Some(r#""ghost""#)),
        progress(4, false, r#""ghost""#, "1"),
    ]);
    assert_eq!(run(ISSUED, &provided), (Vec::<String>::new(), 1));

    // Issued only after the notification: not provided when it was sent.
    let late = session(&[
        progress(3, false, r#""ghost""#, "1"),
        ask(4, true, "2", Some(r#""ghost""#)),
    ]);
    assert_eq!(findings_for(ISSUED, &late).len(), 1);
}

#[test]
fn a_token_the_other_party_provided_is_not_this_partys() {
    // The server opts into progress on its own request, then reports progress
    // under that token: nothing the client asked for.
    let crossed = session(&[
        ask(3, false, r#""s1""#, Some(r#""mine""#)),
        progress(4, false, r#""mine""#, "1"),
    ]);
    let findings = findings_for(ISSUED, &crossed);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("provided it in a request of its own"),
        "{findings:?}"
    );
    // The client reporting on the server's token is the proper direction.
    let proper = session(&[
        ask(3, false, r#""s1""#, Some(r#""mine""#)),
        progress(4, true, r#""mine""#, "1"),
    ]);
    assert_eq!(run(ISSUED, &proper), (Vec::<String>::new(), 1));
}

#[test]
fn without_the_handshake_nothing_about_a_tokens_provenance_is_judged() {
    // The capture may have begun mid-session: the token may predate it.
    let headless = trace(&[progress(0, false, r#""ghost""#, "1"), answer(1, false, "2")]);
    assert_eq!(run(ISSUED, &headless), (Vec::<String>::new(), 0));
    // A notification recorded before the handshake is not the session's.
    let mut lines = vec![progress(0, false, r#""ghost""#, "1")];
    lines.extend(handshake("{}", r#"{"tools":{}}"#).into_iter().map(|line| {
        line.replacen("\"seq\":0,", "\"seq\":10,", 1)
            .replacen("\"seq\":1,", "\"seq\":11,", 1)
            .replacen("\"seq\":2,", "\"seq\":12,", 1)
    }));
    assert_eq!(subjects_for(ISSUED, &trace(&lines)), 0);
    // Over Streamable HTTP the rule reads the same session start.
    let over_http = trace(&[
        http(
            0,
            true,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        ),
        http(
            1,
            false,
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"1"}}}"#,
        ),
        http(
            2,
            false,
            r#"{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":"ghost","progress":1}}"#,
        ),
    ]);
    assert_eq!(findings_for(ISSUED, &over_http).len(), 1);
}
