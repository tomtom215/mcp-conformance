// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the Multi Round-Trip Requests clauses.
//!
//! The recurring risk in this area is the retry correlation: every client-side
//! check turns on "which round is this a retry of", and a check that treated an
//! ordinary follow-up request as a retry would report conforming clients. Each
//! check is therefore pinned on a session where a plain request follows a round,
//! as well as on the violation it exists for.

use crate::checks::draft::testkit::{client, findings_for, server, trace};

const SUPPORTED_METHODS: &str = "mrtr.input-required-supported-methods";
const REQUEST_METHODS: &str = "mrtr.input-request-methods";
const HAS_CONTENT: &str = "mrtr.input-required-has-content";
const CARRIES_RESPONSES: &str = "mrtr.retry-carries-input-responses";
const ECHOED: &str = "mrtr.request-state-echoed";
const UNSOLICITED: &str = "mrtr.no-unsolicited-request-state";
const ID_DIFFERS: &str = "mrtr.retry-id-differs";
const SCOPED: &str = "mrtr.request-state-scoped-to-retry";
const REASKED: &str = "mrtr.missing-input-reasked";

/// One elicitation input request.
const ELICIT: &str = r#""inputRequests":{"login":{"method":"elicitation/create","params":{"mode":"form","message":"?"}}}"#;

/// A client request `id` for `method`, whose `params` also carry `extra`.
fn request(seq: u64, id: u64, method: &str, extra: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{{"name":"t"{extra}}}}}"#
        ),
    )
}

/// A server `input_required` result for `id`, carrying `body`.
fn input_required(seq: u64, id: u64, body: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"input_required"{body}}}}}"#
        ),
    )
}

/// A `tools/call` round asking for `login`, with state `state`, then `retry_extra`.
fn round_then(state: &str, retry_extra: &str) -> String {
    trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, &format!(",{ELICIT}{state}")),
        request(2, 2, "tools/call", retry_extra),
    ])
}

#[test]
fn input_required_answers_only_the_three_supported_requests() {
    for method in ["tools/call", "prompts/get", "resources/read"] {
        let session = trace(&[
            request(0, 1, method, ""),
            input_required(1, 1, &format!(",{ELICIT}")),
        ]);
        assert!(
            findings_for(SUPPORTED_METHODS, &session).is_empty(),
            "{method} supports it"
        );
    }
    let session = trace(&[
        request(0, 1, "ping", ""),
        input_required(1, 1, &format!(",{ELICIT}")),
    ]);
    let findings = findings_for(SUPPORTED_METHODS, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("ping"), "{findings:?}");
}

#[test]
fn an_ordinary_result_is_not_a_round() {
    let session = trace(&[
        request(0, 1, "ping", ""),
        server(
            1,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete"}}"#,
        ),
    ]);
    assert!(findings_for(SUPPORTED_METHODS, &session).is_empty());
    assert!(findings_for(HAS_CONTENT, &session).is_empty());
}

#[test]
fn input_requests_may_only_ask_for_the_three_request_objects() {
    for method in ["elicitation/create", "sampling/createMessage", "roots/list"] {
        let body = format!(r#","inputRequests":{{"k":{{"method":"{method}"}}}}"#);
        let session = trace(&[request(0, 1, "tools/call", ""), input_required(1, 1, &body)]);
        assert!(
            findings_for(REQUEST_METHODS, &session).is_empty(),
            "{method} is permitted"
        );
    }

    let session = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","inputRequests":{"k":{"method":"tools/list"}}"#),
    ]);
    let findings = findings_for(REQUEST_METHODS, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("tools/list"), "{findings:?}");

    // A value that is not a request object at all.
    let shapeless = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","inputRequests":{"k":{"params":{}}}"#),
    ]);
    assert_eq!(findings_for(REQUEST_METHODS, &shapeless).len(), 1);

    // No `inputRequests` at all: nothing to judge.
    let stateful = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","requestState":"s""#),
    ]);
    assert!(findings_for(REQUEST_METHODS, &stateful).is_empty());
}

#[test]
fn an_input_required_must_carry_requests_or_state() {
    let empty = trace(&[request(0, 1, "tools/call", ""), input_required(1, 1, "")]);
    assert_eq!(findings_for(HAS_CONTENT, &empty).len(), 1);

    for body in [
        format!(",{ELICIT}"),
        r#","requestState":"s""#.to_owned(),
        format!(",{ELICIT},\"requestState\":\"s\""),
    ] {
        let session = trace(&[request(0, 1, "tools/call", ""), input_required(1, 1, &body)]);
        assert!(
            findings_for(HAS_CONTENT, &session).is_empty(),
            "body {body} is sufficient"
        );
    }
}

#[test]
fn a_retry_must_answer_everything_the_round_asked_for() {
    let complete = round_then("", r#","inputResponses":{"login":{"action":"accept"}}"#);
    assert!(findings_for(CARRIES_RESPONSES, &complete).is_empty());

    let two_asked = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(
            1,
            1,
            r#","inputRequests":{"login":{"method":"elicitation/create"},"roots":{"method":"roots/list"}}"#,
        ),
        request(
            2,
            2,
            "tools/call",
            r#","inputResponses":{"login":{"action":"accept"}}"#,
        ),
    ]);
    let findings = findings_for(CARRIES_RESPONSES, &two_asked);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("roots"), "{findings:?}");
}

#[test]
fn a_plain_follow_up_request_is_not_a_retry() {
    // The load-bearing case for every client-side check: a request carrying
    // neither `inputResponses` nor `requestState` is a new request, not a
    // half-finished retry, and judging it would fail conforming clients that
    // simply moved on.
    let moved_on = round_then("", "");
    for check in [CARRIES_RESPONSES, ECHOED, UNSOLICITED, ID_DIFFERS, SCOPED] {
        assert!(
            findings_for(check, &moved_on).is_empty(),
            "{check} treated a plain request as a retry"
        );
    }
}

#[test]
fn a_retry_echoes_the_exact_state_it_was_given() {
    let exact = round_then(
        r#","requestState":"opaque-blob""#,
        r#","inputResponses":{"login":{}},"requestState":"opaque-blob""#,
    );
    assert!(findings_for(ECHOED, &exact).is_empty());

    let altered = round_then(
        r#","requestState":"opaque-blob""#,
        r#","inputResponses":{"login":{}},"requestState":"tampered""#,
    );
    let findings = findings_for(ECHOED, &altered);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("tampered"), "{findings:?}");

    let dropped = round_then(
        r#","requestState":"opaque-blob""#,
        r#","inputResponses":{"login":{}}"#,
    );
    let findings = findings_for(ECHOED, &dropped);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("omits"), "{findings:?}");
}

#[test]
fn a_round_without_state_asks_for_none_back() {
    let no_state = round_then("", r#","inputResponses":{"login":{}}"#);
    assert!(findings_for(ECHOED, &no_state).is_empty());

    // …and a retry that invents one is reported by the sibling clause.
    let invented = round_then(
        "",
        r#","inputResponses":{"login":{}},"requestState":"mine""#,
    );
    assert!(findings_for(ECHOED, &invented).is_empty());
    let findings = findings_for(UNSOLICITED, &invented);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn state_presented_with_no_round_at_all_is_unsolicited() {
    let alone = client(
        0,
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"t","requestState":"mine"}}"#,
    );
    assert_eq!(findings_for(UNSOLICITED, &alone).len(), 1);
}

#[test]
fn the_retry_is_a_new_request_with_a_new_id() {
    let reused = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, &format!(",{ELICIT}")),
        request(2, 1, "tools/call", r#","inputResponses":{"login":{}}"#),
    ]);
    let findings = findings_for(ID_DIFFERS, &reused);
    assert_eq!(findings.len(), 1, "{findings:?}");

    let fresh = round_then("", r#","inputResponses":{"login":{}}"#);
    assert!(findings_for(ID_DIFFERS, &fresh).is_empty());
}

#[test]
fn a_state_is_scoped_to_the_request_that_drew_it() {
    let elsewhere = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","requestState":"blob""#),
        request(2, 2, "prompts/get", r#","requestState":"blob""#),
    ]);
    let findings = findings_for(SCOPED, &elsewhere);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("prompts/get"), "{findings:?}");

    let same = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","requestState":"blob""#),
        request(2, 2, "tools/call", r#","requestState":"blob""#),
    ]);
    assert!(findings_for(SCOPED, &same).is_empty());
}

#[test]
fn a_shortfall_should_draw_another_round_not_an_error() {
    let short = |answer: &str| {
        trace(&[
            request(0, 1, "tools/call", ""),
            input_required(1, 1, &format!(",{ELICIT}")),
            request(2, 2, "tools/call", r#","requestState":"blob""#),
            server(3, answer),
        ])
    };

    let errored = short(r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"x"}}"#);
    let findings = findings_for(REASKED, &errored);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("login"), "{findings:?}");

    // Asking again is the conforming answer.
    let reasked = short(
        r#"{"jsonrpc":"2.0","id":2,"result":{"resultType":"input_required","requestState":"blob2"}}"#,
    );
    assert!(findings_for(REASKED, &reasked).is_empty());

    // A complete retry that still drew an error is some other problem, and this
    // clause must not claim it.
    let complete = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, &format!(",{ELICIT}")),
        request(
            2,
            2,
            "tools/call",
            r#","inputResponses":{"login":{"action":"accept"}}"#,
        ),
        server(
            3,
            r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"x"}}"#,
        ),
    ]);
    assert!(findings_for(REASKED, &complete).is_empty());

    // An unanswered retry is not evidence either way.
    let unanswered = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, &format!(",{ELICIT}")),
        request(2, 2, "tools/call", r#","requestState":"blob""#),
    ]);
    assert!(findings_for(REASKED, &unanswered).is_empty());
}
