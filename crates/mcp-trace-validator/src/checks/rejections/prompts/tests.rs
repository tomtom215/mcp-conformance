// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `prompts.get-error-codes`, branch by branch — and with it the listing
//! evidence (`super::super::listing`) every unknown-name check rests on.

use super::super::testkit::{client, error, handshake, request, result, run, server, with};

const CHECK: &str = "prompts.get-error-codes";

/// `review` with a required `diff` and an optional `tone`.
const REVIEW: &str = r#"{"name":"review","arguments":[{"name":"diff","required":true},{"name":"tone","required":false}]}"#;

/// A handshake, then one complete `prompts/list` page holding `prompts` (seq
/// 3–4, id 2).
fn listed(prompts: &str) -> Vec<String> {
    with(
        handshake(r#"{"prompts":{}}"#),
        &[
            request(3, 2, "prompts/list", "{}"),
            result(4, 2, &format!(r#"{{"prompts":[{prompts}]}}"#)),
        ],
    )
}

/// `listed(REVIEW)`, then a `prompts/get` with `params` answered by `answer`
/// (seq 5–6, id 3).
fn got(params: &str, answer: String) -> Vec<String> {
    with(
        listed(REVIEW),
        &[request(5, 3, "prompts/get", params), answer],
    )
}

const MISSING: &str = r#"{"name":"review","arguments":{"tone":"terse"}}"#;
const UNKNOWN: &str = r#"{"name":"nonesuch"}"#;

#[test]
fn a_session_with_no_prompts_get_is_not_observed() {
    assert_eq!(run(CHECK, &listed(REVIEW)), (0, Vec::new()));
}

#[test]
fn a_well_formed_call_is_outside_the_clause() {
    let trace = got(
        r#"{"name":"review","arguments":{"diff":"d"}}"#,
        error(6, 3, "-32000"),
    );
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_call_with_no_name_is_not_judged() {
    assert_eq!(
        run(CHECK, &got("{}", error(6, 3, "-32000"))),
        (0, Vec::new())
    );
    let bare = with(
        listed(REVIEW),
        &[
            client(5, r#"{"jsonrpc":"2.0","id":3,"method":"prompts/get"}"#),
            error(6, 3, "-32000"),
        ],
    );
    assert_eq!(run(CHECK, &bare), (0, Vec::new()));
}

#[test]
fn a_missing_required_argument_refused_with_invalid_params_passes() {
    assert_eq!(
        run(CHECK, &got(MISSING, error(6, 3, "-32602"))),
        (1, Vec::new())
    );
    // Omitting `arguments` altogether omits the argument just the same.
    assert_eq!(
        run(CHECK, &got(r#"{"name":"review"}"#, error(6, 3, "-32602"))),
        (1, Vec::new())
    );
}

#[test]
fn a_missing_required_argument_served_is_reported() {
    let (subjects, findings) = run(CHECK, &got(MISSING, result(6, 3, r#"{"messages":[]}"#)));
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains(r#"["diff"]"#), "{findings:?}");
    assert!(
        findings[0].contains("drew a result rather than -32602"),
        "{findings:?}"
    );
}

#[test]
fn a_missing_required_argument_refused_with_another_code_is_reported() {
    let (subjects, findings) = run(CHECK, &got(MISSING, error(6, 3, "-32000")));
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("drew error -32000"), "{findings:?}");
}

#[test]
fn asking_for_the_missing_input_is_a_conforming_answer() {
    let asked = result(
        6,
        3,
        r#"{"resultType":"input_required","inputRequests":{}}"#,
    );
    assert_eq!(run(CHECK, &got(MISSING, asked)), (1, Vec::new()));
    // Any other `resultType` is a served result.
    let complete = result(6, 3, r#"{"resultType":"complete","messages":[]}"#);
    assert_eq!(run(CHECK, &got(MISSING, complete)).1.len(), 1);
}

#[test]
fn a_retry_carrying_input_responses_is_not_judged() {
    let retry = r#"{"name":"review","arguments":{},"inputResponses":{"diff":{}}}"#;
    assert_eq!(
        run(CHECK, &got(retry, result(6, 3, r#"{"messages":[]}"#))),
        (0, Vec::new())
    );
}

#[test]
fn an_error_naming_another_failure_is_never_judged() {
    for code in [
        "-32603", "-32700", "-32600", "-32020", "-32021", "-32022", "-32042",
    ] {
        assert_eq!(
            run(CHECK, &got(MISSING, error(6, 3, code))),
            (0, Vec::new()),
            "{code}"
        );
        assert_eq!(
            run(CHECK, &got(UNKNOWN, error(6, 3, code))),
            (0, Vec::new()),
            "{code}"
        );
    }
    // Their neighbours are ordinary wrong codes.
    for code in ["-32604", "-32019", "-32023", "-32041", "-32043", "-32601"] {
        assert_eq!(
            run(CHECK, &got(UNKNOWN, error(6, 3, code))).1.len(),
            1,
            "{code}"
        );
    }
}

#[test]
fn a_requirement_the_server_dropped_in_a_later_listing_is_not_held() {
    let mut trace = got(MISSING, result(6, 3, r#"{"messages":[]}"#));
    trace.push(request(7, 4, "prompts/list", "{}"));
    trace.push(result(
        8,
        4,
        r#"{"prompts":[{"name":"review","arguments":[{"name":"diff"}]}]}"#,
    ));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_requirement_every_listing_repeats_is_held() {
    let mut trace = got(MISSING, result(6, 3, r#"{"messages":[]}"#));
    trace.push(request(7, 4, "prompts/list", "{}"));
    trace.push(result(8, 4, &format!(r#"{{"prompts":[{REVIEW}]}}"#)));
    assert_eq!(run(CHECK, &trace).1.len(), 1);
}

#[test]
fn a_declaration_announced_as_changed_is_not_held() {
    let mut trace = got(MISSING, result(6, 3, r#"{"messages":[]}"#));
    trace.push(server(
        7,
        r#"{"jsonrpc":"2.0","method":"notifications/prompts/list_changed"}"#,
    ));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn an_unlisted_name_refused_with_invalid_params_passes() {
    assert_eq!(
        run(CHECK, &got(UNKNOWN, error(6, 3, "-32602"))),
        (1, Vec::new())
    );
}

#[test]
fn an_unlisted_name_refused_with_another_code_is_reported() {
    let (subjects, findings) = run(CHECK, &got(UNKNOWN, error(6, 3, "-32601")));
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("\"nonesuch\""), "{findings:?}");
    assert!(findings[0].contains("with error -32601"), "{findings:?}");
    // An error whose code is not an integer is not `-32602` either.
    let (_, findings) = run(CHECK, &got(UNKNOWN, error(6, 3, r#""nope""#)));
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].contains("an error with no integer code"),
        "{findings:?}"
    );
}

#[test]
fn an_unlisted_name_the_server_serves_is_not_judged() {
    // A server may serve a prompt it does not list.
    let trace = got(UNKNOWN, result(6, 3, r#"{"messages":[]}"#));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_listed_name_refused_for_reasons_of_its_own_is_not_judged() {
    let trace = with(
        listed(r#"{"name":"plain"}"#),
        &[
            request(5, 3, "prompts/get", r#"{"name":"plain"}"#),
            error(6, 3, "-32000"),
        ],
    );
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn without_a_listing_no_name_is_unknown() {
    let trace = with(
        handshake(r#"{"prompts":{}}"#),
        &[request(5, 3, "prompts/get", UNKNOWN), error(6, 3, "-32601")],
    );
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn a_listing_announced_as_changed_proves_no_name_unknown() {
    let mut trace = got(UNKNOWN, error(6, 3, "-32601"));
    trace.push(server(
        7,
        r#"{"jsonrpc":"2.0","method":"notifications/prompts/list_changed"}"#,
    ));
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}

#[test]
fn only_the_servers_own_announcement_withdraws_the_listing() {
    // The same method from the client, and another list's announcement from
    // the server, say nothing about the prompt list.
    let mut trace = got(UNKNOWN, error(6, 3, "-32601"));
    trace.push(client(
        7,
        r#"{"jsonrpc":"2.0","method":"notifications/prompts/list_changed"}"#,
    ));
    trace.push(server(
        8,
        r#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#,
    ));
    assert_eq!(run(CHECK, &trace).1.len(), 1);
}

/// A handshake, a `prompts/list` first page (id 2) issuing `next`, then
/// `rest`, then a refused `prompts/get` of `nonesuch`.
fn paged(first_params: &str, next: &str, rest: &[String]) -> Vec<String> {
    let mut trace = with(
        handshake(r#"{"prompts":{}}"#),
        &[
            request(3, 2, "prompts/list", first_params),
            result(
                4,
                2,
                &format!(r#"{{"prompts":[{{"name":"a"}}],"nextCursor":{next}}}"#),
            ),
        ],
    );
    trace.extend_from_slice(rest);
    trace.push(request(20, 9, "prompts/get", UNKNOWN));
    trace.push(error(21, 9, "-32601"));
    trace
}

#[test]
fn a_listing_cut_short_proves_no_name_unknown() {
    assert_eq!(run(CHECK, &paged("{}", r#""c1""#, &[])), (0, Vec::new()));
}

#[test]
fn a_listing_followed_to_its_last_page_is_whole() {
    let rest = [
        request(5, 3, "prompts/list", r#"{"cursor":"c1"}"#),
        result(6, 3, r#"{"prompts":[{"name":"b"}]}"#),
    ];
    assert_eq!(run(CHECK, &paged("{}", r#""c1""#, &rest)).1.len(), 1);
    // …and a name on its second page is a listed name.
    let mut trace = paged("{}", r#""c1""#, &rest);
    trace.push(request(22, 10, "prompts/get", r#"{"name":"b"}"#));
    trace.push(error(23, 10, "-32601"));
    assert_eq!(run(CHECK, &trace).1.len(), 1);
}

#[test]
fn a_continuation_that_was_refused_leaves_the_listing_cut_short() {
    let rest = [
        request(5, 3, "prompts/list", r#"{"cursor":"c1"}"#),
        error(6, 3, "-32603"),
    ];
    assert_eq!(run(CHECK, &paged("{}", r#""c1""#, &rest)), (0, Vec::new()));
}

#[test]
fn following_a_different_cursor_does_not_complete_the_listing() {
    let rest = [
        request(5, 3, "prompts/list", r#"{"cursor":"other"}"#),
        result(6, 3, r#"{"prompts":[]}"#),
    ];
    assert_eq!(run(CHECK, &paged("{}", r#""c1""#, &rest)), (0, Vec::new()));
}

#[test]
fn a_listing_with_no_first_page_proves_no_name_unknown() {
    // The recording holds a continuation only: the page before it is unseen.
    assert_eq!(
        run(CHECK, &paged(r#"{"cursor":"c0"}"#, "null", &[])),
        (0, Vec::new())
    );
}

#[test]
fn a_null_cursor_asks_for_the_first_page_and_a_null_next_cursor_ends_it() {
    assert_eq!(
        run(CHECK, &paged(r#"{"cursor":null}"#, "null", &[]))
            .1
            .len(),
        1
    );
}

#[test]
fn a_next_cursor_that_is_no_string_leaves_the_listing_unproven() {
    assert_eq!(run(CHECK, &paged("{}", "5", &[])), (0, Vec::new()));
}

#[test]
fn a_list_result_without_its_array_lists_nothing() {
    // An answer that is not a listing is neither a first page nor a whole one.
    let trace = with(
        handshake(r#"{"prompts":{}}"#),
        &[
            request(3, 2, "prompts/list", "{}"),
            result(4, 2, "{}"),
            request(5, 3, "prompts/get", UNKNOWN),
            error(6, 3, "-32601"),
        ],
    );
    assert_eq!(run(CHECK, &trace), (0, Vec::new()));
}
