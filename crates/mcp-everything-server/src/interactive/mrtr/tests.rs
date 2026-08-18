// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the MRTR round.
//!
//! [`ask`] itself needs a live peer on its legacy branch, so what is pinned
//! here is the round bookkeeping — which answer a retry carries, and which
//! state the next round issues. The stateless branch end to end, including the
//! shape of the `InputRequiredResult` on the wire, is pinned by
//! `tests/stateless_stdio.rs`.

use super::*;

/// A round with `responses`, as the client would have re-sent them.
fn round(state: Option<&str>, responses: Option<serde_json::Value>) -> Round {
    Round {
        tool: "test_elicitation",
        state: state.map(ToOwned::to_owned),
        responses: responses
            .map(|value| serde_json::from_value(value).expect("input responses are a map of JSON")),
    }
}

#[test]
fn a_first_call_carries_no_answer() {
    assert!(round(None, None).answer().is_none());
}

#[test]
fn a_retry_carrying_the_requested_key_answers_it() {
    let answered = round(
        Some("test_elicitation:1"),
        Some(serde_json::json!({ "input": { "action": "accept" } })),
    );
    assert_eq!(
        answered.answer(),
        Some(serde_json::json!({ "action": "accept" }))
    );
}

#[test]
fn a_retry_carrying_someone_elses_key_is_not_an_answer() {
    // MRTR-024: the client sent responses, but not the one asked for. The
    // server must ask again rather than read whatever is there — a tool that
    // took the first value in the map would happily consume an answer meant
    // for a different input request.
    let wrong = round(
        Some("test_elicitation:1"),
        Some(serde_json::json!({ "other": { "action": "accept" } })),
    );
    assert!(wrong.answer().is_none());
}

#[test]
fn the_first_state_names_the_tool_and_its_round() {
    assert_eq!(round(None, None).next_state(), "test_elicitation:1");
}

#[test]
fn a_second_round_advances_the_state() {
    // The client echoed round 1's state back and still did not answer; the
    // next ask must be distinguishable from the first, or a trace could not
    // show that two rounds happened.
    assert_eq!(
        round(Some("test_elicitation:1"), None).next_state(),
        "test_elicitation:2"
    );
}

#[test]
fn an_unparseable_state_restarts_the_count_rather_than_failing() {
    // MRTR-007: `requestState` is attacker-controlled input. It carries no
    // authority here, so the only sane response to a forged one is to ignore
    // what it claims — never to trust it, and never to error out on a value
    // the server does not depend on.
    for forged in ["nonsense", "test_elicitation:not-a-number", "", ":"] {
        assert_eq!(
            round(Some(forged), None).next_state(),
            "test_elicitation:1",
            "forged state {forged:?}"
        );
    }
}

#[test]
fn a_round_count_at_the_ceiling_does_not_wrap() {
    let state = format!("test_elicitation:{}", u32::MAX);
    assert_eq!(
        round(Some(&state), None).next_state(),
        format!("test_elicitation:{}", u32::MAX)
    );
}

#[test]
fn an_elicitation_ask_becomes_an_elicitation_input_request() {
    // MRTR-006: `inputRequests` values must be one of three request types, and
    // the wire form is what a client matches on.
    let ask = Ask::Elicit(ElicitRequestParams::FormElicitationParams {
        meta: None,
        message: "hello".to_owned(),
        requested_schema: rmcp::model::ElicitationSchema::builder()
            .required_property(
                "answer",
                rmcp::model::PrimitiveSchemaDefinition::String(rmcp::model::StringSchema::new()),
            )
            .build()
            .expect("a one-property schema"),
    });
    let wire = serde_json::to_value(ask.into_input_request()).expect("serializes");
    assert_eq!(wire["method"], "elicitation/create", "{wire}");
}

#[test]
fn a_sampling_ask_becomes_a_create_message_input_request() {
    let ask = Ask::Sample(CreateMessageRequestParams::new(
        vec![rmcp::model::SamplingMessage::user_text("hi")],
        100,
    ));
    let wire = serde_json::to_value(ask.into_input_request()).expect("serializes");
    assert_eq!(wire["method"], "sampling/createMessage", "{wire}");
}
