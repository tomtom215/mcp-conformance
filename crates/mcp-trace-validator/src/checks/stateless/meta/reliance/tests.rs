// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the capability-reliance clauses: which capability, and which
//! member of it, an input request needs the request it answers to have declared.

use crate::checks::stateless::testkit::{client, findings_for, server, trace};

const CHECK: &str = "meta.no-undeclared-capability-reliance";

/// A `tools/call` with id `id` declaring `capabilities`.
fn call(seq: u64, id: u64, capabilities: &str) -> String {
    client(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"t","_meta":{{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{capabilities}}}}}}}"#
        ),
    )
}

/// An `input_required` result for `id` carrying one input request, `request`.
fn input_required(seq: u64, id: u64, request: &str) -> String {
    server(
        seq,
        &format!(
            r#"{{"jsonrpc":"2.0","id":{id},"result":{{"resultType":"input_required","inputRequests":{{"a":{request}}}}}}}"#
        ),
    )
}

/// A `tools/call` declaring `capabilities`, answered by an `input_required`
/// result asking for `method`.
fn asks_for(capabilities: &str, method: &str) -> String {
    asks_with(capabilities, &format!(r#"{{"method":"{method}"}}"#))
}

/// A `tools/call` declaring `capabilities`, answered by an `input_required`
/// result carrying `request` verbatim.
fn asks_with(capabilities: &str, request: &str) -> String {
    trace(&[call(0, 1, capabilities), input_required(1, 1, request)])
}

#[test]
fn an_input_request_is_matched_to_the_capability_it_needs() {
    // Each of the three methods maps to its own capability name; asking for one
    // the request never declared is the violation.
    for (method, capability) in [
        ("elicitation/create", "elicitation"),
        ("sampling/createMessage", "sampling"),
        ("roots/list", "roots"),
    ] {
        assert_eq!(
            findings_for(CHECK, &asks_for("{}", method)).len(),
            1,
            "{method} needs {capability}"
        );
        let declared = format!(r#"{{"{capability}":{{}}}}"#);
        assert!(
            findings_for(CHECK, &asks_for(&declared, method)).is_empty(),
            "{method} is fine once {capability} is declared"
        );
        // Declaring a *different* capability does not cover it.
        assert_eq!(
            findings_for(CHECK, &asks_for(r#"{"logging":{}}"#, method)).len(),
            1,
            "{method} is not covered by an unrelated capability"
        );
    }

    // A method outside the map needs no capability and is not reported.
    assert!(findings_for(CHECK, &asks_for("{}", "tools/list")).is_empty());
}

#[test]
fn a_reused_id_is_judged_by_the_request_its_result_answers() {
    // BASE-045 permits reusing an id once answered. Keyed by id alone, the
    // `input_required` answering the first request was held against the
    // second, which declared nothing.
    let elicit = r#"{"method":"elicitation/create"}"#;
    let done = |seq| {
        server(
            seq,
            r#"{"jsonrpc":"2.0","id":1,"result":{"resultType":"complete","content":[]}}"#,
        )
    };
    let declared_first = trace(&[
        call(0, 1, r#"{"elicitation":{}}"#),
        input_required(1, 1, elicit),
        call(2, 1, "{}"),
        done(3),
    ]);
    let findings = findings_for(CHECK, &declared_first);
    assert!(findings.is_empty(), "{findings:?}");

    // And the other way round: the later request is the one that asked
    // without declaring, whatever the earlier one sharing its id declared.
    let declared_second = trace(&[
        call(0, 1, "{}"),
        input_required(1, 1, elicit),
        call(2, 1, r#"{"elicitation":{}}"#),
        done(3),
    ]);
    assert_eq!(findings_for(CHECK, &declared_second).len(), 1);
}

#[test]
fn an_elicitation_needs_the_mode_it_asks_in() {
    let elicit = |mode: &str| format!(r#"{{"method":"elicitation/create","params":{{{mode}}}}}"#);
    let url = elicit(r#""mode":"url""#);
    let form = elicit(r#""mode":"form""#);
    let unstated = elicit("");
    // (declared elicitation, request, findings)
    for (declared, request, expected) in [
        // An empty object is form only: "Equivalent to { "form": {} }".
        ("{}", &url, 1),
        ("{}", &form, 0),
        ("{}", &unstated, 0),
        (r#"{"form":{}}"#, &url, 1),
        (r#"{"form":{}}"#, &unstated, 0),
        // URL only: a form-mode request, stated or by default, is unsupported.
        (r#"{"url":{}}"#, &url, 0),
        (r#"{"url":{}}"#, &form, 1),
        (r#"{"url":{}}"#, &unstated, 1),
        (r#"{"form":{},"url":{}}"#, &url, 0),
        (r#"{"form":{},"url":{}}"#, &form, 0),
        // A mode the revision does not define selects nothing to hold it to.
        (r#"{"url":{}}"#, &elicit(r#""mode":"other""#), 0),
        // A malformed declaration says nothing is missing.
        ("true", &url, 0),
    ] {
        let capabilities = format!(r#"{{"elicitation":{declared}}}"#);
        let findings = findings_for(CHECK, &asks_with(&capabilities, request));
        assert_eq!(
            findings.len(),
            expected,
            "{declared} vs {request}: {findings:?}"
        );
        if expected == 1 {
            assert!(findings[0].contains("`elicitation."), "{findings:?}");
        }
    }
}

#[test]
fn tool_enabled_sampling_needs_sampling_tools() {
    let sample = |extra: &str| {
        format!(
            r#"{{"method":"sampling/createMessage","params":{{"messages":[],"maxTokens":1{extra}}}}}"#
        )
    };
    let tools = sample(r#","tools":[]"#);
    let choice = sample(r#","toolChoice":{"mode":"auto"}"#);
    for (declared, request, expected) in [
        ("{}", &tools, 1),
        ("{}", &choice, 1),
        ("{}", &sample(""), 0),
        (r#"{"tools":{}}"#, &tools, 0),
        (r#"{"tools":{}}"#, &choice, 0),
        (r#"{"context":{}}"#, &tools, 1),
    ] {
        let capabilities = format!(r#"{{"sampling":{declared}}}"#);
        let findings = findings_for(CHECK, &asks_with(&capabilities, request));
        assert_eq!(
            findings.len(),
            expected,
            "{declared} vs {request}: {findings:?}"
        );
        if expected == 1 {
            assert!(findings[0].contains("`sampling.tools`"), "{findings:?}");
        }
    }
}
