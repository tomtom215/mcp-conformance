// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Which round a retry answers, judged only from what had been said when it
//! was sent.
//!
//! The correlation clauses read a trace as a conversation, and a conversation
//! has an order: a later exchange cannot tell what an earlier one meant. Two
//! properties pin that here, each against a trace that conforms. First,
//! *suffix invariance*: appending an independent exchange must not change what
//! is found before it. Second, an opaque `requestState` is the server's own
//! (`basic/patterns/mrtr`: "an opaque string meaningful only to the server"),
//! and the page requires uniqueness only of `inputRequests` keys within a
//! request, so a server may issue the same value in two unrelated rounds, and
//! the check must tell them apart by the request each retry repeats rather than
//! let the last one win.

use super::tests::{
    CARRIES_RESPONSES, ECHOED, ELICIT, ID_DIFFERS, REASKED, SCOPED, UNSOLICITED, call,
    input_required, request,
};
use crate::checks::stateless::testkit::{findings_for, server, trace};

/// A completed result for `id`.
fn complete(seq: u64, id: u64) -> String {
    server(
        seq,
        &format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"complete"}}}}"#),
    )
}

/// An `inputRequests` map asking for `llm` by sampling.
const SAMPLE: &str = r#","inputRequests":{"llm":{"method":"sampling/createMessage","params":{}}}"#;

/// Every client-side check that reads a retry's pairing.
const CLIENT_SIDE: [&str; 6] = [
    CARRIES_RESPONSES,
    ECHOED,
    UNSOLICITED,
    ID_DIFFERS,
    REASKED,
    SCOPED,
];

#[test]
fn a_later_round_reusing_a_state_does_not_convict_an_earlier_correct_retry() {
    // Two unrelated exchanges, each retry echoing its own round's state, and
    // the server happening to mint the same opaque value for both. Judged by a
    // map over the whole trace, the later `prompts/get` round overwrote the
    // earlier `tools/call` one and the first retry was reported at seq 2.
    let session = trace(&[
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","requestState":"S""#),
        request(2, 2, "tools/call", r#","requestState":"S""#),
        complete(3, 2),
        request(4, 3, "prompts/get", ""),
        input_required(5, 3, r#","requestState":"S""#),
        request(6, 4, "prompts/get", r#","requestState":"S""#),
    ]);
    assert_eq!(findings_for(SCOPED, &session), Vec::<String>::new());
}

#[test]
fn appending_an_independent_exchange_changes_nothing_found_before_it() {
    // The prefix holds one real violation: a state drawn by `tools/call` is
    // presented on `prompts/get`.
    let prefix = vec![
        request(0, 1, "tools/call", ""),
        input_required(1, 1, r#","requestState":"S""#),
        request(2, 2, "prompts/get", r#","requestState":"S""#),
    ];
    let before = findings_for(SCOPED, &trace(&prefix));
    assert_eq!(before.len(), 1, "{before:?}");

    // An unrelated later exchange that issues the same value. It must neither
    // remove that finding (a later `prompts/get` round made the map agree with
    // the retry) nor add one.
    let mut extended = prefix;
    extended.extend([
        request(3, 3, "prompts/get", ""),
        input_required(4, 3, r#","requestState":"S""#),
        request(5, 4, "prompts/get", r#","requestState":"S""#),
    ]);
    let after = findings_for(SCOPED, &trace(&extended));
    assert_eq!(after, before);
}

#[test]
fn a_state_not_yet_issued_when_a_retry_presents_it_is_not_judged_against_a_later_round() {
    // Nothing had issued "S" when the retry arrived. Whatever a later round
    // issues cannot say what the earlier request used.
    let session = trace(&[
        request(0, 1, "prompts/get", r#","requestState":"S""#),
        request(1, 2, "tools/call", ""),
        input_required(2, 2, r#","requestState":"S""#),
    ]);
    assert_eq!(findings_for(SCOPED, &session), Vec::<String>::new());
}

#[test]
fn a_state_presented_for_another_target_of_the_same_method_is_reported() {
    // `tools/call` of `b` carries the state drawn by `tools/call` of `a`: the
    // same method, a different request. The method alone cannot tell them apart.
    let session = trace(&[
        call(0, 1, "a", ""),
        input_required(1, 1, r#","requestState":"S""#),
        call(2, 2, "b", r#","requestState":"S""#),
    ]);
    let findings = findings_for(SCOPED, &session);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("\"b\"") && findings[0].contains("\"a\""),
        "{findings:?}"
    );

    // The same tool again is the server's further round on a repeated attempt,
    // which the page allows.
    let repeated = trace(&[
        call(0, 1, "a", ""),
        input_required(1, 1, r#","requestState":"S""#),
        call(2, 2, "a", r#","requestState":"S""#),
    ]);
    assert_eq!(findings_for(SCOPED, &repeated), Vec::<String>::new());
}

#[test]
fn rounds_sharing_a_state_are_told_apart_by_the_request_each_retry_repeats() {
    // Two tools in flight, one opaque state minted for both, different inputs
    // asked of each. Paired by state alone, the first retry went to the second
    // tool's round (the latest to issue the value) and was reported missing the
    // second tool's input.
    let session = trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "b", ""),
        input_required(2, 1, &format!(r#",{ELICIT},"requestState":"S""#)),
        input_required(3, 2, &format!(r#"{SAMPLE},"requestState":"S""#)),
        call(
            4,
            3,
            "a",
            r#","inputResponses":{"login":{}},"requestState":"S""#,
        ),
        call(
            5,
            4,
            "b",
            r#","inputResponses":{"llm":{}},"requestState":"S""#,
        ),
    ]);
    for check in CLIENT_SIDE {
        assert_eq!(
            findings_for(check, &session),
            Vec::<String>::new(),
            "{check}"
        );
    }
}

#[test]
fn a_state_reused_on_another_request_is_still_paired_with_the_round_that_issued_it() {
    // Genuine cross-request reuse stays judged: no round of tool `b` issued "S",
    // so the retry is paired with the round that did (tool `a`'s), where the
    // clause reports it.
    let session = trace(&[
        call(0, 1, "a", ""),
        input_required(1, 1, &format!(r#",{ELICIT},"requestState":"S""#)),
        call(
            2,
            2,
            "b",
            r#","inputResponses":{"login":{}},"requestState":"S""#,
        ),
    ]);
    assert_eq!(findings_for(SCOPED, &session).len(), 1);
}

#[test]
fn rounds_of_one_target_sharing_a_state_but_asking_differently_leave_the_retry_unjudged() {
    // Two concurrent calls of the same tool, the same state, different inputs.
    // Nothing on the wire says which the retry answers; judging it against the
    // later round was a guess.
    let session = trace(&[
        call(0, 1, "a", ""),
        call(1, 2, "a", ""),
        input_required(2, 1, &format!(r#",{ELICIT},"requestState":"S""#)),
        input_required(3, 2, &format!(r#"{SAMPLE},"requestState":"S""#)),
        call(
            4,
            3,
            "a",
            r#","inputResponses":{"login":{}},"requestState":"S""#,
        ),
    ]);
    assert_eq!(
        findings_for(CARRIES_RESPONSES, &session),
        Vec::<String>::new()
    );
}

#[test]
fn a_server_rejecting_a_retry_for_a_fault_of_its_own_is_not_blamed_for_not_asking_again() {
    // The round issued state "S" and asked for `login`. The retry tampers with
    // the state (or drops it, or reuses the first request's id) and omits the
    // input, and the server rejects it with -32602, as the page requires for a
    // state that fails verification. The error is explained by the fault, so it
    // is no evidence that the missing input was the cause.
    let rejected = |retry: String| {
        trace(&[
            request(0, 1, "tools/call", ""),
            input_required(1, 1, &format!(r#",{ELICIT},"requestState":"S""#)),
            retry,
            server(
                3,
                r#"{"jsonrpc":"2.0","id":2,"error":{"code":-32602,"message":"state failed verification"}}"#,
            ),
        ])
    };
    let altered = rejected(request(2, 2, "tools/call", r#","requestState":"TAMPERED""#));
    assert_eq!(findings_for(REASKED, &altered), Vec::<String>::new());
    let dropped = rejected(request(2, 2, "tools/call", r#","inputResponses":{}"#));
    assert_eq!(findings_for(REASKED, &dropped), Vec::<String>::new());

    // Control: the state echoed exactly and the input still missing, answered
    // with an error. That is the clause's own case, and it still fires.
    let clean = rejected(request(2, 2, "tools/call", r#","requestState":"S""#));
    assert_eq!(findings_for(REASKED, &clean).len(), 1);
}
