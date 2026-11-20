// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `pagination.malformed-cursor-rejected`, branch by branch.

use super::super::testkit::{error, request, result, run};

const CHECK: &str = "pagination.malformed-cursor-rejected";

/// One `method` request with `params`, answered by `answer`.
fn listed(method: &str, params: &str, answer: String) -> Vec<String> {
    vec![request(0, 1, method, params), answer]
}

#[test]
fn a_request_with_no_cursor_or_a_string_one_is_not_observed() {
    for params in [
        "{}",
        r#"{"cursor":null}"#,
        r#"{"cursor":"never-issued"}"#,
        r#"{"cursor":""}"#,
    ] {
        let trace = listed("tools/list", params, result(1, 1, r#"{"tools":[]}"#));
        assert_eq!(run(CHECK, &trace), (0, Vec::new()), "{params}");
    }
}

#[test]
fn a_cursor_that_is_no_string_refused_with_invalid_params_passes() {
    for cursor in ["7", "true", "[]", r#"{"page":2}"#] {
        let trace = listed(
            "tools/list",
            &format!(r#"{{"cursor":{cursor}}}"#),
            error(1, 1, "-32602"),
        );
        assert_eq!(run(CHECK, &trace), (1, Vec::new()), "{cursor}");
    }
}

#[test]
fn a_cursor_that_is_no_string_honoured_is_reported() {
    let trace = listed(
        "tools/list",
        r#"{"cursor":7}"#,
        result(1, 1, r#"{"tools":[]}"#),
    );
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(
        findings[0].contains("`tools/list` presented the cursor 7,"),
        "{findings:?}"
    );
    assert!(
        findings[0].contains("drew a result rather than -32602"),
        "{findings:?}"
    );
}

#[test]
fn another_error_code_is_reported() {
    let trace = listed("prompts/list", r#"{"cursor":7}"#, error(1, 1, "-32601"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("drew error -32601"), "{findings:?}");
}

#[test]
fn an_error_naming_another_failure_is_not_judged() {
    let trace = listed("tools/list", r#"{"cursor":7}"#, error(1, 1, "-32603"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn every_paginated_method_is_judged_and_no_other() {
    for method in [
        "resources/list",
        "resources/templates/list",
        "prompts/list",
        "tools/list",
    ] {
        let trace = listed(method, r#"{"cursor":7}"#, result(1, 1, "{}"));
        assert_eq!(run(CHECK, &trace).1.len(), 1, "{method}");
    }
    let trace = listed("tools/call", r#"{"cursor":7}"#, result(1, 1, "{}"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}
