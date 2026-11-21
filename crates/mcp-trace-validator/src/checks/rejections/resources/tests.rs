// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `resources.not-found-code` and `resources.missing-resource-code`, branch by
//! branch. The two share their stimulus and differ in the code they expect.

use super::super::testkit::{error, handshake, request, result, run, server, with};

/// `RES-008`, `2025-11-25`: expects `-32002`.
const OLD: &str = "resources.not-found-code";
/// `RES-019`, `2026-07-28`: expects `-32602`.
const NEW: &str = "resources.missing-resource-code";

/// A handshake, then complete listings of `resources` and `templates` (seq
/// 3–6, ids 2–3).
fn listed(resources: &str, templates: &str) -> Vec<String> {
    with(
        handshake(r#"{"resources":{}}"#),
        &[
            request(3, 2, "resources/list", "{}"),
            result(4, 2, &format!(r#"{{"resources":[{resources}]}}"#)),
            request(5, 3, "resources/templates/list", "{}"),
            result(6, 3, &format!(r#"{{"resourceTemplates":[{templates}]}}"#)),
        ],
    )
}

/// The usual listings — a `file` resource and a `db` template — then a read
/// of `uri` answered by `answer` (seq 7–8, id 4).
fn read(uri: &str, answer: String) -> Vec<String> {
    with(
        listed(
            r#"{"uri":"file:///a.txt","name":"a"}"#,
            r#"{"uriTemplate":"db://{table}","name":"t"}"#,
        ),
        &[
            request(7, 4, "resources/read", &format!(r#"{{"uri":"{uri}"}}"#)),
            answer,
        ],
    )
}

const UNLISTED: &str = "nonesuch://x";

#[test]
fn a_session_with_no_read_is_not_observed() {
    let trace = listed(r#"{"uri":"file:///a.txt"}"#, "");
    assert_eq!(run(OLD, &trace), (0, Vec::new()));
    assert_eq!(run(NEW, &trace), (0, Vec::new()));
}

#[test]
fn each_revision_passes_its_own_code() {
    assert_eq!(
        run(OLD, &read(UNLISTED, error(8, 4, "-32002"))),
        (1, Vec::new())
    );
    assert_eq!(
        run(NEW, &read(UNLISTED, error(8, 4, "-32602"))),
        (1, Vec::new())
    );
}

#[test]
fn invalid_params_is_not_judged_at_the_earlier_revision() {
    assert_eq!(
        run(OLD, &read(UNLISTED, error(8, 4, "-32602"))),
        (0, Vec::new())
    );
}

#[test]
fn the_withdrawn_code_is_reported_at_the_later_revision() {
    let (subjects, findings) = run(NEW, &read(UNLISTED, error(8, 4, "-32002")));
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(findings[0].contains("\"nonesuch://x\""), "{findings:?}");
    assert!(
        findings[0].contains("error -32002, the code earlier revisions used"),
        "{findings:?}"
    );
    // It is the server's own word, so it needs no listing — and is reported
    // even for a URI under a listed scheme, or a request naming no URI.
    let bare = [
        request(7, 4, "resources/read", r#"{"uri":"file:///gone"}"#),
        error(8, 4, "-32002"),
        request(9, 5, "resources/read", "{}"),
        error(10, 5, "-32002"),
    ];
    let (subjects, findings) = run(NEW, &bare);
    assert_eq!((subjects, findings.len()), (2, 2), "{findings:?}");
    assert!(findings[1].contains("of null was refused"), "{findings:?}");
}

#[test]
fn any_other_code_is_reported_at_the_earlier_revision_and_unjudged_at_the_later() {
    let trace = read(UNLISTED, error(8, 4, "0"));
    let (subjects, findings) = run(OLD, &trace);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
    assert!(
        findings[0].contains("refused with error 0 rather than -32002"),
        "{findings:?}"
    );
    // At the later revision only the server's own "not found" is a witness:
    // that this refusal means the resource is missing is an inference.
    assert_eq!(run(NEW, &trace), (0, Vec::new()));
    assert_eq!(
        run(NEW, &read(UNLISTED, error(8, 4, "-32601"))),
        (0, Vec::new())
    );
}

/// Asserts neither check finds a subject in `trace`.
fn assert_unjudged(trace: &[String]) {
    assert_eq!(run(OLD, trace), (0, Vec::new()));
    assert_eq!(run(NEW, trace), (0, Vec::new()));
}

#[test]
fn an_internal_error_is_never_judged() {
    assert_unjudged(&read(UNLISTED, error(8, 4, "-32603")));
}

#[test]
fn a_served_result_is_never_judged() {
    assert_unjudged(&read(UNLISTED, result(8, 4, r#"{"contents":[]}"#)));
}

#[test]
fn a_uri_under_a_listed_scheme_is_never_judged() {
    assert_unjudged(&read("file:///other.txt", error(8, 4, "0")));
    // A template's scheme counts, and schemes compare case-insensitively.
    assert_unjudged(&read("db://users", error(8, 4, "0")));
    assert_unjudged(&read("DB://users", error(8, 4, "0")));
}

#[test]
fn a_listed_scheme_is_matched_whatever_its_case() {
    let trace = with(
        listed(r#"{"uri":"FILE:///a.txt"}"#, ""),
        &[
            request(7, 4, "resources/read", r#"{"uri":"file:///b.txt"}"#),
            error(8, 4, "0"),
        ],
    );
    assert_unjudged(&trace);
}

#[test]
fn a_string_with_no_scheme_is_not_judged() {
    assert_unjudged(&read("not a uri", error(8, 4, "0")));
    assert_unjudged(&read("1bad://x", error(8, 4, "0")));
}

#[test]
fn a_request_with_no_string_uri_is_not_judged() {
    let trace = with(
        listed("", ""),
        &[
            request(7, 4, "resources/read", r#"{"uri":7}"#),
            error(8, 4, "0"),
        ],
    );
    assert_unjudged(&trace);
}

#[test]
fn both_listings_must_be_in_the_session() {
    let read = [
        request(7, 4, "resources/read", r#"{"uri":"nonesuch://x"}"#),
        error(8, 4, "0"),
    ];
    let resources_only = with(
        handshake(r#"{"resources":{}}"#),
        &[
            request(3, 2, "resources/list", "{}"),
            result(4, 2, r#"{"resources":[]}"#),
        ],
    );
    assert_unjudged(&with(resources_only, &read));
    let templates_only = with(
        handshake(r#"{"resources":{}}"#),
        &[
            request(5, 3, "resources/templates/list", "{}"),
            result(6, 3, r#"{"resourceTemplates":[]}"#),
        ],
    );
    assert_unjudged(&with(templates_only, &read));
    // With both — empty as they are — the scheme is one the server lists nowhere.
    assert_eq!(run(OLD, &with(listed("", ""), &read)).1.len(), 1);
}

#[test]
fn a_template_that_leaves_its_scheme_open_matches_anything() {
    for template in ["{scheme}://x", "{+uri}", "a{b}c:rest"] {
        let trace = with(
            listed("", &format!(r#"{{"uriTemplate":"{template}"}}"#)),
            &[
                request(7, 4, "resources/read", r#"{"uri":"nonesuch://x"}"#),
                error(8, 4, "0"),
            ],
        );
        assert_unjudged(&trace);
    }
}

#[test]
fn a_listed_resource_with_no_scheme_excuses_nothing() {
    let trace = with(
        listed(r#"{"uri":"no scheme here"}"#, ""),
        &[
            request(7, 4, "resources/read", r#"{"uri":"nonesuch://x"}"#),
            error(8, 4, "0"),
        ],
    );
    assert_eq!(run(OLD, &trace).1.len(), 1);
}

#[test]
fn a_listing_announced_as_changed_proves_nothing() {
    let mut trace = read(UNLISTED, error(8, 4, "0"));
    trace.push(server(
        9,
        r#"{"jsonrpc":"2.0","method":"notifications/resources/list_changed"}"#,
    ));
    assert_unjudged(&trace);
}

#[test]
fn a_listing_cut_short_proves_nothing() {
    let read = [
        request(7, 4, "resources/read", r#"{"uri":"nonesuch://x"}"#),
        error(8, 4, "0"),
    ];
    let resources_short = with(
        handshake(r#"{"resources":{}}"#),
        &[
            request(3, 2, "resources/list", "{}"),
            result(4, 2, r#"{"resources":[],"nextCursor":"more"}"#),
            request(5, 3, "resources/templates/list", "{}"),
            result(6, 3, r#"{"resourceTemplates":[]}"#),
        ],
    );
    assert_unjudged(&with(resources_short, &read));
    let templates_short = with(
        handshake(r#"{"resources":{}}"#),
        &[
            request(3, 2, "resources/list", "{}"),
            result(4, 2, r#"{"resources":[]}"#),
            request(5, 3, "resources/templates/list", "{}"),
            result(6, 3, r#"{"resourceTemplates":[],"nextCursor":"more"}"#),
        ],
    );
    assert_unjudged(&with(templates_short, &read));
}

#[test]
fn invalid_params_for_an_ordinary_read_is_no_subject_at_the_later_revision() {
    // `-32602` says "invalid" as readily as "missing": only a URI the listings
    // place nowhere makes it this clause's answer.
    assert_eq!(
        run(NEW, &read("file:///other.txt", error(8, 4, "-32602"))),
        (0, Vec::new())
    );
}
