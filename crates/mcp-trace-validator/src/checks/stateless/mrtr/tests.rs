// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the Multi Round-Trip Requests clauses.
//!
//! The recurring risk in this area is the retry correlation: every client-side
//! check turns on "which round is this a retry of", and a check that treated an
//! ordinary follow-up request as a retry would report conforming clients. Each
//! check is therefore pinned on a session where a plain request follows a round,
//! as well as on the violation it exists for.

use crate::checks::stateless::testkit::{client, findings_for, server, trace};

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

/// A request `id` for `tools/call` of tool `name`, whose `params` also carry `extra`.
fn call(seq: u64, id: u64, name: &str, extra: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}"{extra}}}}}"#
        ),
    )
}

/// Two concurrent rounds — `a` asking for `login` with state `sa`, `b` asking
/// for `llm` with state `sb` — then both retries, `b`'s first.
fn interleaved(retry_a: &str, retry_b: &str) -> String {
    trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "b", ""),
        input_required(2, 1, &format!(r#",{ELICIT},"requestState":"sa""#)),
        input_required(
            3,
            2,
            r#","inputRequests":{"llm":{"method":"sampling/createMessage","params":{}}},"requestState":"sb""#,
        ),
        call(4, 4, "b", retry_b),
        call(5, 3, "a", retry_a),
    ])
}

#[test]
fn interleaved_rounds_pair_by_the_state_each_retry_echoes() {
    // The official Python SDK's shape for two concurrent tool calls. Paired by
    // recency, `a`'s retry was judged against `b`'s round and failed four MUSTs.
    let conforming = interleaved(
        r#","inputResponses":{"login":{}},"requestState":"sa""#,
        r#","inputResponses":{"llm":{}},"requestState":"sb""#,
    );
    for check in [CARRIES_RESPONSES, ECHOED, UNSOLICITED, ID_DIFFERS, REASKED] {
        assert!(findings_for(check, &conforming).is_empty(), "{check}");
    }
}

#[test]
fn an_altered_state_is_still_judged_against_the_request_it_repeats() {
    // No round issued "tampered", but only one round answered tool `a`.
    let altered = interleaved(
        r#","inputResponses":{"login":{}},"requestState":"tampered""#,
        r#","inputResponses":{"llm":{}},"requestState":"sb""#,
    );
    let findings = findings_for(ECHOED, &altered);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("\"sa\""), "{findings:?}");
}

#[test]
fn rounds_that_would_be_judged_differently_leave_a_retry_unjudged() {
    // Two open rounds of the same tool with different states, and a retry that
    // echoes neither: either pairing is a guess, so nothing is reported.
    let ambiguous = trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "a", ""),
        input_required(2, 1, &format!(r#",{ELICIT},"requestState":"s1""#)),
        input_required(3, 2, &format!(r#",{ELICIT},"requestState":"s2""#)),
        call(4, 3, "a", r#","inputResponses":{"login":{}}"#),
    ]);
    for check in [CARRIES_RESPONSES, ECHOED, UNSOLICITED, ID_DIFFERS] {
        assert!(findings_for(check, &ambiguous).is_empty(), "{check}");
    }
    // Rounds that ask for the same thing with no state are interchangeable, so
    // the shortfall is still reported whichever one the retry answers.
    let interchangeable = trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "a", ""),
        input_required(2, 1, &format!(",{ELICIT}")),
        input_required(3, 2, &format!(",{ELICIT}")),
        call(4, 3, "a", r#","inputResponses":{}"#),
    ]);
    assert_eq!(findings_for(CARRIES_RESPONSES, &interchangeable).len(), 1);
}

/// The originating request id of the round `retry_seq`'s retry pairs with, or
/// why it pairs with none.
fn paired_origin(document: &str, retry_seq: u64) -> Result<String, &'static str> {
    use super::pairing::{Pairing, retries_with_rounds};
    let events = crate::checks::stateless::testkit::events(document);
    let context = crate::context::TraceContext::new(&events);
    let pairs = retries_with_rounds(&context);
    let (_, pairing) = pairs
        .iter()
        .find(|(retry, _)| retry.seq == retry_seq)
        .ok_or("not a retry in the trace")?;
    match pairing {
        Pairing::Round(round) => Ok(round.origin.1.to_string()),
        Pairing::Ambiguous => Err("ambiguous"),
        Pairing::Unseen => Err("unseen"),
    }
}

const SAMPLE: &str = r#""inputRequests":{"llm":{"method":"sampling/createMessage","params":{}}}"#;

#[test]
fn a_retry_pairs_with_the_round_for_its_own_tool() {
    // Two open rounds asking for different inputs, no state: only the round
    // for the same tool can be the one.
    let document = trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "b", ""),
        input_required(2, 1, &format!(",{ELICIT}")),
        input_required(3, 2, &format!(",{SAMPLE}")),
        call(4, 3, "b", r#","inputResponses":{"llm":{}}"#),
    ]);
    assert_eq!(paired_origin(&document, 4), Ok("2".to_owned()));
}

#[test]
fn a_resource_read_retry_pairs_by_its_uri() {
    let read = |seq: u64, id: u64, uri: &str, extra: &str| {
        client(
            seq,
            &format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"resources/read","params":{{"uri":"{uri}"{extra}}}}}"#
            ),
        )
    };
    let document = trace(&[
        read(0, 1, "file:///x", ""),
        read(1, 2, "file:///y", ""),
        input_required(2, 1, &format!(",{ELICIT}")),
        input_required(3, 2, &format!(",{SAMPLE}")),
        read(4, 3, "file:///y", r#","inputResponses":{"llm":{}}"#),
    ]);
    assert_eq!(paired_origin(&document, 4), Ok("2".to_owned()));
}

#[test]
fn a_retry_pairs_only_with_rounds_of_its_own_method() {
    // A prompt and a tool share the name `t`; the tools/call retry answers the
    // tools/call round.
    let document = trace(&[
        request(0, 1, "prompts/get", ""),
        request(1, 2, "tools/call", ""),
        input_required(2, 1, &format!(",{ELICIT}")),
        input_required(3, 2, &format!(",{SAMPLE}")),
        request(4, 3, "tools/call", r#","inputResponses":{"llm":{}}"#),
    ]);
    assert_eq!(paired_origin(&document, 4), Ok("2".to_owned()));
}

#[test]
fn rounds_that_ask_for_nothing_are_interchangeable() {
    // Two rounds of the same tool, neither asking for input nor issuing state
    // (load shedding): any pairing judges the same, so the latest is taken.
    let document = trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "a", ""),
        input_required(2, 1, ""),
        input_required(3, 2, ""),
        call(4, 3, "a", r#","inputResponses":{}"#),
    ]);
    assert_eq!(paired_origin(&document, 4), Ok("2".to_owned()));
}
