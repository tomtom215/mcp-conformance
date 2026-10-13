// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `client/sampling` checks.

use super::super::testkit::{
    ask, asked, findings_for, reply, reply_error, round, session, subjects_for,
};

const TEXT: &str = r#"{"role":"user","content":{"type":"text","text":"Weather?"}}"#;
const USE: &str = r#"{"role":"assistant","content":[{"type":"tool_use","id":"c1","name":"w","input":{}},{"type":"tool_use","id":"c2","name":"w","input":{}}]}"#;
const RESULTS: &str = r#"{"role":"user","content":[{"type":"tool_result","toolUseId":"c1","content":[]},{"type":"tool_result","toolUseId":"c2","content":[]}]}"#;
const ONE_RESULT: &str =
    r#"{"role":"user","content":{"type":"tool_result","toolUseId":"c1","content":[]}}"#;
const MIXED: &str = r#"{"role":"user","content":[{"type":"text","text":"here"},{"type":"tool_result","toolUseId":"c1","content":[]}]}"#;
const ANSWER_TEXT: &str = r#"{"role":"assistant","content":{"type":"text","text":"Sunny"},"model":"m","stopReason":"endTurn"}"#;
const ANSWER_TOOL: &str = r#"{"role":"assistant","content":[{"type":"tool_use","id":"c3","name":"w","input":{}}],"model":"m","stopReason":"toolUse"}"#;

/// Sampling params with `messages` and `extra` members.
fn params(messages: &[&str], extra: &str) -> String {
    format!(
        r#"{{"messages":[{}],"maxTokens":10{extra}}}"#,
        messages.join(",")
    )
}

/// One `inputRequests` entry `a` asking for sampling with `params`.
fn entry(params: &str) -> String {
    format!(r#""a":{{"method":"sampling/createMessage","params":{params}}}"#)
}

#[test]
fn tool_enabled_sampling_needs_sampling_tools() {
    const CHECK: &str = "sampling.tools-declared";
    let tools = params(
        &[TEXT],
        r#","tools":[{"name":"w","inputSchema":{"type":"object"}}]"#,
    );
    for (capabilities, expected) in [
        (r#"{"sampling":{"tools":{}}}"#, 0),
        (r#"{"sampling":{}}"#, 1),
        ("{}", 1),
    ] {
        let mrtr = asked(capabilities, &entry(&tools));
        let direct = session(capabilities, &[ask(3, 1, "sampling/createMessage", &tools)]);
        for trace in [mrtr, direct] {
            let found = findings_for(CHECK, &trace);
            assert_eq!(found.len(), expected, "{capabilities}: {found:?}");
            assert_eq!(subjects_for(CHECK, &trace), 1);
        }
    }
    assert!(findings_for(CHECK, &asked("{}", &entry(&tools)))[0].contains("`sampling.tools`"));
    // No `tools`: not tool-enabled, whatever else it carries.
    let plain = asked(
        "{}",
        &entry(&params(&[TEXT], r#","toolChoice":{"mode":"auto"}"#)),
    );
    assert_eq!(subjects_for(CHECK, &plain), 0);
}

#[test]
fn include_context_values_are_avoided_and_need_sampling_context() {
    let using = |value: &str| params(&[TEXT], &format!(r#","includeContext":"{value}""#));
    for (value, capabilities, avoided, declared, declared_subjects) in [
        ("none", "{}", 0, 0, 0),
        ("thisServer", "{}", 1, 1, 1),
        ("allServers", r#"{"sampling":{}}"#, 1, 1, 1),
        ("allServers", r#"{"sampling":{"context":{}}}"#, 1, 0, 1),
    ] {
        let trace = asked(capabilities, &entry(&using(value)));
        assert_eq!(
            findings_for("sampling.include-context-avoided", &trace).len(),
            avoided,
            "{value}"
        );
        assert_eq!(subjects_for("sampling.include-context-avoided", &trace), 1);
        assert_eq!(
            findings_for("sampling.include-context-declared", &trace).len(),
            declared,
            "{value} {capabilities}"
        );
        assert_eq!(
            subjects_for("sampling.include-context-declared", &trace),
            declared_subjects
        );
    }
    let found = findings_for(
        "sampling.include-context-declared",
        &asked("{}", &entry(&using("thisServer"))),
    );
    assert!(
        found[0].contains(r#""thisServer""#) && found[0].contains("`sampling.context`"),
        "{found:?}"
    );
    assert_eq!(
        findings_for(
            "sampling.include-context-avoided",
            &asked("{}", &entry(&params(&[TEXT], "")))
        ),
        Vec::<String>::new()
    );
}

#[test]
fn tool_results_stand_alone_in_their_message() {
    const CHECK: &str = "sampling.tool-results-only";
    for (messages, subjects, expected) in [
        (vec![TEXT, USE, RESULTS], 1, 0),
        (vec![TEXT, USE, MIXED], 1, 1),
        (vec![TEXT], 0, 0),
        // A tool result in an assistant message is not this clause's.
        (
            vec![
                r#"{"role":"assistant","content":[{"type":"text","text":"x"},{"type":"tool_result","toolUseId":"c","content":[]}]}"#,
            ],
            0,
            0,
        ),
    ] {
        let trace = session(
            "{}",
            &[ask(3, 1, "sampling/createMessage", &params(&messages, ""))],
        );
        let found = findings_for(CHECK, &trace);
        assert_eq!(found.len(), expected, "{messages:?}");
        assert_eq!(subjects_for(CHECK, &trace), subjects, "{messages:?}");
    }
    let found = findings_for(
        CHECK,
        &asked("{}", &entry(&params(&[TEXT, USE, MIXED], ""))),
    );
    assert!(found[0].contains("`messages[2]`"), "{found:?}");
}

#[test]
fn every_tool_use_is_answered_by_the_next_message() {
    const CHECK: &str = "sampling.tool-use-answered";
    let text_reply = r#"{"role":"assistant","content":{"type":"text","text":"x"}}"#;
    for (messages, expected, reason) in [
        (vec![TEXT, USE, RESULTS, text_reply], 0, ""),
        (vec![TEXT, USE, ONE_RESULT], 1, r#""c2""#),
        (vec![TEXT, USE], 1, "last message"),
        (vec![TEXT, USE, TEXT, RESULTS], 1, "not a user message"),
        (vec![TEXT, USE, MIXED], 1, "not a user message"),
        (vec![TEXT, USE, text_reply], 1, "not a user message"),
        (
            vec![TEXT, USE, r#"{"role":"user","content":[]}"#],
            1,
            "not a user message",
        ),
    ] {
        let trace = asked("{}", &entry(&params(&messages, "")));
        let found = findings_for(CHECK, &trace);
        assert_eq!(found.len(), expected, "{messages:?}: {found:?}");
        assert_eq!(subjects_for(CHECK, &trace), 1);
        if expected > 0 {
            assert!(
                found[0].contains(reason) && found[0].contains("`messages[1]`"),
                "{found:?}"
            );
        }
    }
    assert_eq!(
        subjects_for(
            CHECK,
            &asked("{}", &entry(&params(&[TEXT, text_reply], "")))
        ),
        0
    );
}

#[test]
fn sampling_messages_have_a_role_and_content() {
    const CHECK: &str = "sampling.message-shape";
    for (message, expected) in [
        (TEXT, 0),
        (
            r#"{"role":"system","content":{"type":"text","text":"x"}}"#,
            1,
        ),
        (r#"{"content":{"type":"text","text":"x"}}"#, 1),
        (r#"{"role":"user"}"#, 1),
    ] {
        let trace = asked("{}", &entry(&params(&[message], "")));
        assert_eq!(findings_for(CHECK, &trace).len(), expected, "{message}");
        assert_eq!(subjects_for(CHECK, &trace), 1);
    }
    let caps = r#"{"sampling":{}}"#;
    let request = entry(&params(&[TEXT], ""));
    let good = round(caps, &request, &format!(r#""a":{ANSWER_TEXT}"#), caps);
    assert_eq!(findings_for(CHECK, &good), Vec::<String>::new());
    assert_eq!(subjects_for(CHECK, &good), 2);
    let bad = round(caps, &request, r#""a":{"role":"tool","content":{}}"#, caps);
    let found = findings_for(CHECK, &bad);
    assert_eq!(found.len(), 1);
    assert!(
        found[0].contains("the client's answer") && found[0].contains("`role`"),
        "{found:?}"
    );
    let refused = session(
        caps,
        &[
            ask(3, 1, "sampling/createMessage", &params(&[], "")),
            reply_error(4, 1, -1),
        ],
    );
    assert_eq!(subjects_for(CHECK, &refused), 0);
}

/// Sampling params offering one tool under `toolChoice` `mode`.
fn choosing(mode: &str) -> String {
    params(
        &[TEXT],
        &format!(
            r#","tools":[{{"name":"w","inputSchema":{{"type":"object"}}}}],"toolChoice":{{"mode":"{mode}"}}"#
        ),
    )
}

/// The client capabilities the tool-choice tests declare.
const TOOL_CAPS: &str = r#"{"sampling":{"tools":{}}}"#;

/// An MRTR round and a direct exchange, each asking under `mode` and answered
/// with `answer`.
fn answered_under(mode: &str, answer: &str) -> [String; 2] {
    [
        round(
            TOOL_CAPS,
            &entry(&choosing(mode)),
            &format!(r#""a":{answer}"#),
            TOOL_CAPS,
        ),
        session(
            TOOL_CAPS,
            &[
                ask(3, 1, "sampling/createMessage", &choosing(mode)),
                reply(4, 1, answer),
            ],
        ),
    ]
}

#[test]
fn tool_choice_is_honoured_by_the_answer() {
    const REQUIRED: &str = "sampling.tool-choice-required-honoured";
    const NONE: &str = "sampling.tool-choice-none-honoured";
    for (check, mode, answer, subjects, expected) in [
        (REQUIRED, "required", ANSWER_TOOL, 1, 0),
        (REQUIRED, "required", ANSWER_TEXT, 1, 1),
        (REQUIRED, "auto", ANSWER_TEXT, 0, 0),
        (REQUIRED, "none", ANSWER_TEXT, 0, 0),
        (NONE, "none", ANSWER_TEXT, 1, 0),
        (NONE, "none", ANSWER_TOOL, 1, 1),
        (NONE, "required", ANSWER_TOOL, 0, 0),
    ] {
        for trace in answered_under(mode, answer) {
            let found = findings_for(check, &trace);
            assert_eq!(found.len(), expected, "{check} {mode}: {found:?}");
            assert_eq!(subjects_for(check, &trace), subjects, "{check} {mode}");
        }
    }
}

#[test]
fn tool_choice_findings_say_which_way_the_answer_went() {
    let [mrtr, _] = answered_under("none", ANSWER_TOOL);
    let found = findings_for("sampling.tool-choice-none-honoured", &mrtr);
    assert!(found[0].ends_with("uses a tool"), "{found:?}");
    let [mrtr, _] = answered_under("required", ANSWER_TEXT);
    let found = findings_for("sampling.tool-choice-required-honoured", &mrtr);
    assert!(found[0].ends_with("uses no tool"), "{found:?}");
    let refused = session(
        TOOL_CAPS,
        &[
            ask(3, 1, "sampling/createMessage", &choosing("required")),
            reply_error(4, 1, -1),
        ],
    );
    assert_eq!(
        subjects_for("sampling.tool-choice-required-honoured", &refused),
        0
    );
}

#[test]
fn malformed_tool_messages_are_refused_with_invalid_params() {
    const CHECK: &str = "sampling.malformed-tool-messages-rejected";
    let caps = r#"{"sampling":{"tools":{}}}"#;
    let answered = |messages: &[&str], answer: String| {
        session(
            caps,
            &[
                ask(3, 1, "sampling/createMessage", &params(messages, "")),
                answer,
            ],
        )
    };
    for (messages, answer, subjects, expected) in [
        (vec![TEXT, USE, MIXED], reply_error(4, 1, -32602), 1, 0),
        (vec![TEXT, USE, ONE_RESULT], reply_error(4, 1, -32602), 1, 0),
        (vec![TEXT, USE, MIXED], reply(4, 1, ANSWER_TEXT), 1, 1),
        (vec![TEXT, USE, ONE_RESULT], reply_error(4, 1, -1), 1, 1),
        (vec![TEXT, MIXED], reply(4, 1, ANSWER_TEXT), 1, 1),
        (vec![TEXT, USE, RESULTS], reply(4, 1, ANSWER_TEXT), 0, 0),
    ] {
        let trace = answered(&messages, answer);
        assert_eq!(findings_for(CHECK, &trace).len(), expected, "{messages:?}");
        assert_eq!(subjects_for(CHECK, &trace), subjects, "{messages:?}");
    }
    let found = findings_for(CHECK, &answered(&[TEXT, USE], reply_error(4, 1, -1)));
    assert!(found[0].contains("error -1"), "{found:?}");
    let found = findings_for(CHECK, &answered(&[TEXT, USE], reply(4, 1, ANSWER_TEXT)));
    assert!(found[0].contains("a result"), "{found:?}");
    // An MRTR answer has no JSON-RPC error to give.
    let mrtr = round(
        caps,
        &entry(&params(&[TEXT, USE], "")),
        &format!(r#""a":{ANSWER_TEXT}"#),
        caps,
    );
    assert_eq!(subjects_for(CHECK, &mrtr), 0);
}
