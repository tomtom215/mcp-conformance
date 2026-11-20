// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `completion.error-codes`, branch by branch.

use super::super::testkit::{client, error, handshake, request, result, run, with};

const CHECK: &str = "completion.error-codes";

/// A whole request: a `ref/prompt` naming `prompt`, and an argument.
fn whole(prompt: &str) -> String {
    format!(
        r#"{{"ref":{{"type":"ref/prompt","name":"{prompt}"}},"argument":{{"name":"a","value":"v"}}}}"#
    )
}

/// A server declaring `capabilities`, a complete `prompts/list` holding
/// `review` (seq 3–4, id 2), then a `completion/complete` with `params`
/// answered by `answer` (seq 5–6, id 3).
fn completed(capabilities: &str, params: &str, answer: String) -> Vec<String> {
    with(
        handshake(capabilities),
        &[
            request(3, 2, "prompts/list", "{}"),
            result(4, 2, r#"{"prompts":[{"name":"review"}]}"#),
            request(5, 3, "completion/complete", params),
            answer,
        ],
    )
}

const DECLARED: &str = r#"{"completions":{},"prompts":{}}"#;
const WITHHELD: &str = r#"{"prompts":{}}"#;
const VALUES: &str = r#"{"completion":{"values":[]}}"#;

#[test]
fn a_session_with_no_completion_request_is_not_observed() {
    assert_eq!(run(CHECK, &handshake(DECLARED)), (0, Vec::new()));
}

#[test]
fn an_unsupported_capability_refused_as_method_not_found_passes() {
    let trace = completed(WITHHELD, &whole("review"), error(6, 3, "-32601"));
    assert_eq!(run(CHECK, &trace), (1, Vec::new()));
}

#[test]
fn an_unsupported_capability_refused_with_another_code_is_reported() {
    let trace = completed(WITHHELD, &whole("review"), error(6, 3, "-32602"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(
        findings[0].contains("drew error -32602 rather than -32601"),
        "{findings:?}"
    );
    assert!(
        findings[0].contains("declared no `completions`"),
        "{findings:?}"
    );
}

#[test]
fn an_undeclared_capability_that_answers_is_another_clauses_matter() {
    let trace = completed(WITHHELD, &whole("review"), result(6, 3, VALUES));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn an_internal_error_is_never_judged() {
    for capabilities in [WITHHELD, DECLARED] {
        let trace = completed(capabilities, "{}", error(6, 3, "-32603"));
        assert_eq!(run(CHECK, &trace), (0, Vec::new()));
    }
}

#[test]
fn a_session_without_a_declaration_is_not_judged() {
    let trace = [
        request(5, 3, "completion/complete", "{}"),
        error(6, 3, "-32000"),
    ];
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn the_discover_result_is_a_declaration_too() {
    let discovered = |capabilities: &str| {
        vec![
            request(0, 1, "server/discover", "{}"),
            result(1, 1, &format!(r#"{{"capabilities":{capabilities}}}"#)),
            request(5, 3, "completion/complete", "{}"),
            error(6, 3, "-32000"),
        ]
    };
    let (subjects, findings) = run(CHECK, &discovered("{}"));
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("rather than -32601"), "{findings:?}");
    let (subjects, findings) = run(CHECK, &discovered(r#"{"completions":{}}"#));
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("rather than -32602"), "{findings:?}");
}

#[test]
fn a_missing_member_refused_with_invalid_params_passes() {
    let no_argument = r#"{"ref":{"type":"ref/prompt","name":"review"}}"#;
    let trace = completed(DECLARED, no_argument, error(6, 3, "-32602"));
    assert_eq!(run(CHECK, &trace), (1, Vec::new()));
}

#[test]
fn a_missing_member_answered_any_other_way_is_reported() {
    let no_argument = r#"{"ref":{"type":"ref/prompt","name":"review"}}"#;
    let trace = completed(DECLARED, no_argument, result(6, 3, VALUES));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(
        findings[0].contains("drew a result rather than -32602"),
        "{findings:?}"
    );
    assert!(findings[0].contains("no `params.argument`"), "{findings:?}");

    let no_ref = r#"{"argument":{"name":"a","value":"v"}}"#;
    let trace = completed(DECLARED, no_ref, error(6, 3, "-32000"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("no `params.ref`"), "{findings:?}");
    assert!(findings[0].contains("drew error -32000"), "{findings:?}");
}

#[test]
fn a_request_with_no_params_is_missing_both() {
    let trace = with(
        handshake(DECLARED),
        &[
            client(
                5,
                r#"{"jsonrpc":"2.0","id":3,"method":"completion/complete"}"#,
            ),
            result(6, 3, VALUES),
        ],
    );
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("no `params.argument`"), "{findings:?}");
}

#[test]
fn an_unlisted_prompt_refused_with_invalid_params_passes() {
    let trace = completed(DECLARED, &whole("nonesuch"), error(6, 3, "-32602"));
    assert_eq!(run(CHECK, &trace), (1, Vec::new()));
}

#[test]
fn an_unlisted_prompt_refused_with_another_code_is_reported() {
    let trace = completed(DECLARED, &whole("nonesuch"), error(6, 3, "-32601"));
    let (subjects, findings) = run(CHECK, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("prompt \"nonesuch\""), "{findings:?}");
    assert!(
        findings[0].contains("drew error -32601 rather than -32602"),
        "{findings:?}"
    );
}

#[test]
fn an_unlisted_prompt_the_server_completes_for_is_not_judged() {
    let trace = completed(DECLARED, &whole("nonesuch"), result(6, 3, VALUES));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_whole_request_for_a_listed_prompt_is_outside_the_clause() {
    let trace = completed(DECLARED, &whole("review"), error(6, 3, "-32000"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn only_a_prompt_reference_names_a_prompt() {
    let resource = r#"{"ref":{"type":"ref/resource","name":"nonesuch","uri":"x:y"},"argument":{"name":"a","value":"v"}}"#;
    let trace = completed(DECLARED, resource, error(6, 3, "-32000"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
    let nameless = r#"{"ref":{"type":"ref/prompt"},"argument":{"name":"a","value":"v"}}"#;
    let trace = completed(DECLARED, nameless, error(6, 3, "-32000"));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn without_a_prompt_listing_no_prompt_is_unknown() {
    let trace = with(
        handshake(DECLARED),
        &[
            request(5, 3, "completion/complete", &whole("nonesuch")),
            error(6, 3, "-32601"),
        ],
    );
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}
