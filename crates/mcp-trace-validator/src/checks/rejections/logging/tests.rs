// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `logging.set-level-invalid-rejected`, branch by branch.

use super::super::testkit::{error, handshake, request, result, run, with};

const CHECK: &str = "logging.set-level-invalid-rejected";

/// A session declaring `logging` whose `logging/setLevel` carries `params` and
/// is answered by `answer`.
fn set_level(params: &str, answer: String) -> Vec<String> {
    with(
        handshake(r#"{"logging":{}}"#),
        &[request(3, 2, "logging/setLevel", params), answer],
    )
}

#[test]
fn a_session_that_never_sets_a_level_is_not_observed() {
    assert_eq!(run(CHECK, &handshake(r#"{"logging":{}}"#)), (0, Vec::new()));
}

#[test]
fn every_real_level_is_outside_the_clause() {
    for level in [
        "debug",
        "info",
        "notice",
        "warning",
        "error",
        "critical",
        "alert",
        "emergency",
    ] {
        let trace = set_level(&format!(r#"{{"level":"{level}"}}"#), error(4, 2, "-32000"));
        assert_eq!(run(CHECK, &trace), (0, Vec::new()), "{level}");
    }
}

#[test]
fn an_invalid_level_refused_with_invalid_params_passes() {
    let trace = set_level(r#"{"level":"verbose"}"#, error(4, 2, "-32602"));
    assert_eq!(run(CHECK, &trace), (1, Vec::new()));
}

#[test]
fn an_invalid_level_accepted_is_reported() {
    let trace = set_level(r#"{"level":"verbose"}"#, result(4, 2, "{}"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("\"verbose\""), "{findings:?}");
    assert!(
        findings[0].contains("drew a result rather than -32602"),
        "{findings:?}"
    );
}

#[test]
fn an_invalid_level_refused_with_another_code_is_reported() {
    let trace = set_level(r#"{"level":"verbose"}"#, error(4, 2, "-32601"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(findings[0].contains("drew error -32601"), "{findings:?}");
}

#[test]
fn a_level_that_is_not_a_string_is_invalid_too() {
    let trace = set_level(r#"{"level":7}"#, result(4, 2, "{}"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("level 7,"), "{findings:?}");
}

#[test]
fn an_internal_error_is_the_clauses_other_case_and_is_not_judged() {
    let trace = set_level(r#"{"level":"verbose"}"#, error(4, 2, "-32603"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_request_with_no_level_is_not_this_clauses_failure_case() {
    let trace = set_level("{}", result(4, 2, "{}"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_server_that_declared_no_logging_is_not_judged() {
    let trace = with(
        handshake("{}"),
        &[
            request(3, 2, "logging/setLevel", r#"{"level":"verbose"}"#),
            result(4, 2, "{}"),
        ],
    );
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_session_without_its_handshake_is_not_judged() {
    let trace = [
        request(3, 2, "logging/setLevel", r#"{"level":"verbose"}"#),
        result(4, 2, "{}"),
    ];
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}
