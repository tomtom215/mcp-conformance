// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `client/elicitation` checks, each held to both revisions'
//! ways of asking.

use super::super::testkit::{
    ask, asked, findings_for, reply, reply_error, round, session, subjects_for, trace,
};

const FORM: &str = r#"{"mode":"form","message":"Name?","requestedSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}"#;
const BARE_FORM: &str = r#"{"message":"Name?","requestedSchema":{"type":"object"}}"#;
const URL: &str = r#"{"mode":"url","message":"Connect","url":"https://example.com/connect","elicitationId":"e1"}"#;

/// One `inputRequests` entry `a` asking `elicitation/create` with `params`.
fn entry(params: &str) -> String {
    format!(r#""a":{{"method":"elicitation/create","params":{params}}}"#)
}

#[test]
fn mode_supported_reads_the_declared_modes() {
    const CHECK: &str = "elicitation.mode-supported";
    for (capabilities, params, expected) in [
        (r#"{"elicitation":{}}"#, FORM, 0),
        (r#"{"elicitation":{}}"#, BARE_FORM, 0),
        (r#"{"elicitation":{}}"#, URL, 1),
        (r#"{"elicitation":{"form":{}}}"#, URL, 1),
        (r#"{"elicitation":{"url":{}}}"#, URL, 0),
        (r#"{"elicitation":{"url":{}}}"#, FORM, 1),
        (r#"{"elicitation":{"url":null}}"#, FORM, 0),
        (r#"{"elicitation":{"form":{},"url":{}}}"#, FORM, 0),
        (r#"{"elicitation":{"form":{},"url":{}}}"#, URL, 0),
        (r#"{"elicitation":true}"#, FORM, 0),
        (r#"{"elicitation":true}"#, URL, 1),
        (r"{}", FORM, 1),
        (r#"{"elicitation":false}"#, FORM, 1),
    ] {
        let mrtr = asked(capabilities, &entry(params));
        let direct = session(capabilities, &[ask(3, 1, "elicitation/create", params)]);
        for trace in [mrtr, direct] {
            let findings = findings_for(CHECK, &trace);
            assert_eq!(
                findings.len(),
                expected,
                "{capabilities} {params}: {findings:?}"
            );
            assert_eq!(subjects_for(CHECK, &trace), 1);
        }
    }
}

#[test]
fn mode_supported_names_what_is_missing_and_abstains_when_it_cannot_know() {
    const CHECK: &str = "elicitation.mode-supported";
    // A mode that is no mode is the parameter clause's, and an unknowable
    // declaration judges nothing.
    let other = asked(
        r#"{"elicitation":{}}"#,
        &entry(r#"{"mode":"x","message":"m"}"#),
    );
    assert_eq!(subjects_for(CHECK, &other), 0);
    let headless = trace(&[ask(0, 1, "elicitation/create", URL)]);
    assert_eq!(subjects_for(CHECK, &headless), 0);
    let found = findings_for(CHECK, &asked(r#"{"elicitation":{}}"#, &entry(URL)));
    assert!(found[0].contains("`inputRequests[a]`"), "{found:?}");
    assert!(found[0].contains("url mode"), "{found:?}");
    assert!(found[0].contains("`elicitation.url`"), "{found:?}");
    assert!(
        found[0].contains("the request's `clientCapabilities`"),
        "{found:?}"
    );
    let found = findings_for(
        CHECK,
        &session(
            r#"{"elicitation":{"url":{}}}"#,
            &[ask(3, 1, "elicitation/create", FORM)],
        ),
    );
    assert!(
        found[0].contains("the `elicitation/create` request"),
        "{found:?}"
    );
    assert!(found[0].contains("`elicitation.form`"), "{found:?}");
    assert!(found[0].contains("`initialize`"), "{found:?}");
    let found = findings_for(CHECK, &asked("{}", &entry(FORM)));
    assert!(
        found[0].ends_with("does not declare `elicitation` at all"),
        "{found:?}"
    );
}

#[test]
fn request_parameters_need_a_message_and_a_known_mode() {
    const CHECK: &str = "elicitation.request-parameters";
    for (params, expected) in [
        (FORM, 0),
        (URL, 0),
        (BARE_FORM, 0),
        (r#"{"mode":"form","requestedSchema":{}}"#, 1),
        (r#"{"message":7}"#, 1),
        (r#"{"mode":"chat","message":"m"}"#, 1),
        (r#"{"mode":3}"#, 2),
    ] {
        let found = findings_for(CHECK, &asked("{}", &entry(params)));
        assert_eq!(found.len(), expected, "{params}: {found:?}");
    }
    let no_params = asked("{}", r#""a":{"method":"elicitation/create"}"#);
    assert_eq!(findings_for(CHECK, &no_params).len(), 1);
    assert_eq!(subjects_for(CHECK, &no_params), 1);
    let found = findings_for(
        CHECK,
        &asked("{}", &entry(r#"{"mode":"chat","message":"m"}"#)),
    );
    assert!(found[0].contains(r#""chat""#), "{found:?}");
    assert!(
        findings_for(CHECK, &asked("{}", &entry(r#"{"mode":"form"}"#)))[0]
            .contains("no string `message`")
    );
}

#[test]
fn form_mode_needs_a_requested_schema() {
    const CHECK: &str = "elicitation.form-schema-present";
    for (params, subjects, expected) in [
        (FORM, 1, 0),
        (BARE_FORM, 1, 0),
        (r#"{"message":"m"}"#, 1, 1),
        (
            r#"{"mode":"form","message":"m","requestedSchema":"x"}"#,
            1,
            1,
        ),
        (URL, 0, 0),
        (r#"{"mode":"x"}"#, 0, 0),
    ] {
        let trace = session("{}", &[ask(3, 1, "elicitation/create", params)]);
        assert_eq!(findings_for(CHECK, &trace).len(), expected, "{params}");
        assert_eq!(subjects_for(CHECK, &trace), subjects, "{params}");
    }
}

#[test]
fn url_mode_parameters_differ_by_revision() {
    let without_id = r#"{"mode":"url","message":"m","url":"https://example.com"}"#;
    let without_url = r#"{"mode":"url","message":"m"}"#;
    for (check, params, expected) in [
        ("elicitation.url-parameters", without_id, 0),
        ("elicitation.url-parameters-with-id", without_id, 1),
        ("elicitation.url-parameters-with-id", URL, 0),
        ("elicitation.url-parameters", without_url, 1),
        (
            "elicitation.url-parameters",
            r#"{"mode":"url","url":"https://x"}"#,
            1,
        ),
        (
            "elicitation.url-parameters",
            r#"{"mode":"url","message":"m","url":5}"#,
            1,
        ),
    ] {
        let found = findings_for(check, &asked("{}", &entry(params)));
        assert_eq!(found.len(), expected, "{check} {params}: {found:?}");
    }
    let found = findings_for(
        "elicitation.url-parameters-with-id",
        &asked("{}", &entry(r#"{"mode":"url"}"#)),
    );
    assert!(
        found[0].ends_with("`message`, `url`, `elicitationId`"),
        "{found:?}"
    );
    assert_eq!(
        subjects_for("elicitation.url-parameters", &asked("{}", &entry(FORM))),
        0
    );
}

#[test]
fn url_must_be_an_rfc_3986_uri() {
    const CHECK: &str = "elicitation.url-valid";
    let url = |url: &str| format!(r#"{{"mode":"url","message":"m","url":"{url}"}}"#);
    assert_eq!(
        findings_for(
            CHECK,
            &asked("{}", &entry(&url("https://example.com/a?b#c")))
        ),
        Vec::<String>::new()
    );
    let found = findings_for(CHECK, &asked("{}", &entry(&url("not a url"))));
    assert_eq!(found.len(), 1);
    assert!(
        found[0].contains(r#""not a url""#) && found[0].contains("no scheme"),
        "{found:?}"
    );
    assert_eq!(
        findings_for(
            CHECK,
            &session(
                "{}",
                &[ask(3, 1, "elicitation/create", &url("https://bad host"))]
            )
        )
        .len(),
        1
    );
    // Missing, non-string or non-URL-mode: not this clause's subject.
    for params in [
        r#"{"mode":"url","message":"m"}"#,
        r#"{"mode":"url","url":1}"#,
        FORM,
    ] {
        assert_eq!(
            subjects_for(CHECK, &asked("{}", &entry(params))),
            0,
            "{params}"
        );
    }
}

#[test]
fn an_accepted_form_is_held_to_its_schema() {
    const CHECK: &str = "elicitation.response-matches-schema";
    let caps = r#"{"elicitation":{}}"#;
    for (response, subjects, expected) in [
        (r#"{"action":"accept","content":{"name":"octocat"}}"#, 1, 0),
        (r#"{"action":"accept","content":{}}"#, 1, 1),
        (r#"{"action":"accept","content":{"name":3}}"#, 1, 1),
        (r#"{"action":"accept"}"#, 0, 0),
        (r#"{"action":"decline","content":{}}"#, 0, 0),
        (r#"{"content":{}}"#, 0, 0),
    ] {
        let mrtr = round(caps, &entry(FORM), &format!(r#""a":{response}"#), caps);
        let direct = session(
            caps,
            &[ask(3, 1, "elicitation/create", FORM), reply(4, 1, response)],
        );
        for trace in [mrtr, direct] {
            assert_eq!(findings_for(CHECK, &trace).len(), expected, "{response}");
            assert_eq!(subjects_for(CHECK, &trace), subjects, "{response}");
        }
    }
    let found = findings_for(
        CHECK,
        &round(
            caps,
            &entry(FORM),
            r#""a":{"action":"accept","content":{}}"#,
            caps,
        ),
    );
    assert!(
        found[0].contains("`inputRequests[a]`") && found[0].contains("name"),
        "{found:?}"
    );
    // URL mode, a schema that is not an object, or one the engine cannot
    // compile: nothing to validate against.
    for params in [
        URL,
        r#"{"message":"m","requestedSchema":true}"#,
        r#"{"message":"m","requestedSchema":{"$schema":"urn:unknown"}}"#,
    ] {
        let trace = session(
            caps,
            &[
                ask(3, 1, "elicitation/create", params),
                reply(4, 1, r#"{"action":"accept","content":{}}"#),
            ],
        );
        assert_eq!(subjects_for(CHECK, &trace), 0, "{params}");
    }
    // An error answer has no result to hold.
    let error = session(
        caps,
        &[ask(3, 1, "elicitation/create", FORM), reply_error(4, 1, -1)],
    );
    assert_eq!(subjects_for(CHECK, &error), 0);
}

#[test]
fn a_client_that_answers_must_have_declared_the_capability() {
    let accept = r#"{"action":"accept","content":{"name":"n"}}"#;
    for (check, method, capability, params, result) in [
        (
            "elicitation.client-declares-capability",
            "elicitation/create",
            "elicitation",
            FORM,
            accept,
        ),
        (
            "sampling.client-declares-capability",
            "sampling/createMessage",
            "sampling",
            r#"{"messages":[],"maxTokens":1}"#,
            r#"{"role":"assistant","content":{"type":"text","text":"t"},"model":"m"}"#,
        ),
        (
            "roots.client-declares-capability",
            "roots/list",
            "roots",
            "{}",
            r#"{"roots":[]}"#,
        ),
    ] {
        let declared = format!(r#"{{"{capability}":{{}}}}"#);
        let requests = format!(r#""a":{{"method":"{method}","params":{params}}}"#);
        let responses = format!(r#""a":{result}"#);
        // The retry's own declaration is the one the clause reads.
        assert_eq!(
            findings_for(check, &round("{}", &requests, &responses, &declared)),
            Vec::<String>::new()
        );
        let found = findings_for(check, &round(&declared, &requests, &responses, "{}"));
        assert_eq!(found.len(), 1, "{check}");
        assert!(
            found[0].contains(&format!("the `{capability}` capability")),
            "{found:?}"
        );
        assert!(
            found[0].contains("the retry's `clientCapabilities`"),
            "{found:?}"
        );
        // Directly: the initialize declaration, and only results count.
        let answered =
            |caps: &str| session(caps, &[ask(3, 1, method, params), reply(4, 1, result)]);
        assert_eq!(
            findings_for(check, &answered(&declared)),
            Vec::<String>::new()
        );
        let found = findings_for(check, &answered("{}"));
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("`initialize`"), "{found:?}");
        assert_eq!(subjects_for(check, &answered("{}")), 1);
        let refused = session(
            "{}",
            &[ask(3, 1, method, params), reply_error(4, 1, -32601)],
        );
        assert_eq!(subjects_for(check, &refused), 0);
        let headless = trace(&[ask(0, 1, method, params), reply(1, 1, result)]);
        assert_eq!(subjects_for(check, &headless), 0);
    }
}

#[test]
fn an_undeclared_mode_is_refused_with_invalid_params() {
    const CHECK: &str = "elicitation.undeclared-mode-rejected";
    let caps = r#"{"elicitation":{"form":{}}}"#;
    let answered = |caps: &str, params: &str, answer: String| {
        session(caps, &[ask(3, 1, "elicitation/create", params), answer])
    };
    assert_eq!(
        findings_for(CHECK, &answered(caps, URL, reply_error(4, 1, -32602))),
        Vec::<String>::new()
    );
    assert_eq!(
        subjects_for(CHECK, &answered(caps, URL, reply_error(4, 1, -32602))),
        1
    );
    let found = findings_for(CHECK, &answered(caps, URL, reply_error(4, 1, -32601)));
    assert!(found[0].contains("error -32601"), "{found:?}");
    let found = findings_for(
        CHECK,
        &answered(caps, URL, reply(4, 1, r#"{"action":"accept"}"#)),
    );
    assert!(
        found[0].contains("a result") && found[0].contains("url mode"),
        "{found:?}"
    );
    let url_only = r#"{"elicitation":{"url":{}}}"#;
    assert_eq!(
        findings_for(CHECK, &answered(url_only, FORM, reply(4, 1, "{}"))).len(),
        1
    );
    // A declared mode, no elicitation at all, a mode that is no mode, or an
    // MRTR answer: not this clause's subject.
    for trace in [
        answered(caps, FORM, reply(4, 1, "{}")),
        answered("{}", URL, reply_error(4, 1, -32601)),
        answered(caps, r#"{"mode":"x"}"#, reply_error(4, 1, -32602)),
        round(caps, &entry(URL), r#""a":{"action":"decline"}"#, caps),
    ] {
        assert_eq!(subjects_for(CHECK, &trace), 0, "{trace}");
    }
}
