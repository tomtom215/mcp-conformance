// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for `elicitation.declared-mode-handled`: a client that answers
//! "method not found" in every mode it declared supports none.

use super::super::super::testkit::{
    ask, call, findings_for, input_required, reply, reply_error, session, subjects_for,
};
use super::{BARE_FORM, FORM, URL, entry};

const CHECK: &str = "elicitation.declared-mode-handled";
const FORM_ONLY: &str = r#"{"elicitation":{"form":{}}}"#;
const URL_ONLY: &str = r#"{"elicitation":{"url":{}}}"#;
const BOTH: &str = r#"{"elicitation":{"form":{},"url":{}}}"#;
const ACCEPT: &str = r#"{"action":"accept","content":{"name":"x"}}"#;

/// A session declaring `capabilities` in which the server asks each of
/// `exchanges` — `(params, error code, or 0 for a result)` — in turn.
fn asked_each(capabilities: &str, exchanges: &[(&str, i64)]) -> String {
    let mut rest = Vec::new();
    for (index, (params, code)) in (0u64..).zip(exchanges) {
        rest.push(ask(3 + 2 * index, index, "elicitation/create", params));
        rest.push(if *code == 0 {
            reply(4 + 2 * index, index, ACCEPT)
        } else {
            reply_error(4 + 2 * index, index, *code)
        });
    }
    session(capabilities, &rest)
}

#[test]
fn method_not_found_in_the_only_declared_mode_is_reported() {
    for (capabilities, params, mode) in [
        (FORM_ONLY, FORM, "form"),
        (FORM_ONLY, BARE_FORM, "form"),
        // Naming neither mode declares form mode.
        (r#"{"elicitation":{}}"#, FORM, "form"),
        (URL_ONLY, URL, "url"),
    ] {
        let trace = asked_each(capabilities, &[(params, -32601)]);
        let found = findings_for(CHECK, &trace);
        assert_eq!(found.len(), 1, "{capabilities} {params}: {found:?}");
        assert!(
            found[0].contains(&format!("({mode} at seq 4)")),
            "{found:?}"
        );
        assert_eq!(subjects_for(CHECK, &trace), 1);
    }
}

#[test]
fn any_other_answer_shows_no_missing_support() {
    for code in [0, -32602, -32603, -32600, 32601] {
        let trace = asked_each(FORM_ONLY, &[(FORM, code)]);
        assert_eq!(findings_for(CHECK, &trace), Vec::<String>::new(), "{code}");
        assert_eq!(subjects_for(CHECK, &trace), 1);
    }
    // A decline is a result: the client handled the request.
    let declined = session(
        FORM_ONLY,
        &[
            ask(3, 1, "elicitation/create", FORM),
            reply(4, 1, r#"{"action":"decline"}"#),
        ],
    );
    assert_eq!(findings_for(CHECK, &declined), Vec::<String>::new());
}

#[test]
fn a_result_anywhere_shows_a_supported_mode() {
    // Refused once and served once, in either order.
    for exchanges in [
        [(FORM, -32601), (FORM, 0)],
        [(FORM, 0), (FORM, -32601)],
        // Both modes declared: one refused, the other served.
        [(URL, -32601), (FORM, 0)],
    ] {
        let trace = asked_each(BOTH, &exchanges);
        assert_eq!(findings_for(CHECK, &trace), Vec::<String>::new());
        assert_eq!(subjects_for(CHECK, &trace), 2);
    }
}

#[test]
fn every_declared_mode_must_be_refused() {
    // Only one of two declared modes was ever asked for: the other may work.
    for params in [FORM, URL] {
        let trace = asked_each(BOTH, &[(params, -32601)]);
        assert_eq!(
            findings_for(CHECK, &trace),
            Vec::<String>::new(),
            "{params}"
        );
        assert_eq!(subjects_for(CHECK, &trace), 1);
    }
    // One refused as not found, the other with another error: unknown.
    let mixed = asked_each(BOTH, &[(FORM, -32601), (URL, -32603)]);
    assert_eq!(findings_for(CHECK, &mixed), Vec::<String>::new());
    // Both refused: reported once, at the answer that completes the picture.
    let trace = asked_each(BOTH, &[(URL, -32601), (FORM, -32601)]);
    let found = findings_for(CHECK, &trace);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("(form at seq 6, url at seq 4)"),
        "{found:?}"
    );
    let events =
        crate::reader::parse_trace(&trace, &crate::reader::Limits::default()).unwrap_or_default();
    let context = crate::context::TraceContext::new(&events);
    let seqs: Vec<Option<u64>> = crate::checks::find(CHECK)
        .map(|check| check.run(&context).findings)
        .unwrap_or_default()
        .iter()
        .map(|finding| finding.seq)
        .collect();
    assert_eq!(seqs, [Some(6)]);
}

#[test]
fn an_undeclared_or_unknowable_mode_is_not_a_subject() {
    for (capabilities, params) in [
        // The mode was not declared: `-32602` territory, another clause.
        (FORM_ONLY, URL),
        (URL_ONLY, FORM),
        // No elicitation capability at all: `-32601` is the right answer.
        ("{}", FORM),
        // A mode that is no mode.
        (BOTH, r#"{"mode":"x","message":"m"}"#),
    ] {
        let trace = asked_each(capabilities, &[(params, -32601)]);
        assert_eq!(subjects_for(CHECK, &trace), 0, "{capabilities} {params}");
        assert_eq!(findings_for(CHECK, &trace), Vec::<String>::new());
    }
    // An undeclared mode's refusal does not count toward a declared one's.
    let trace = asked_each(URL_ONLY, &[(FORM, -32601)]);
    assert_eq!(findings_for(CHECK, &trace), Vec::<String>::new());
    // No `initialize` in the recording: nothing was declared that can be read.
    let headless = super::super::super::testkit::trace(&[
        ask(0, 1, "elicitation/create", FORM),
        reply_error(1, 1, -32601),
    ]);
    assert_eq!(subjects_for(CHECK, &headless), 0);
    // Another method's `-32601` is not an elicitation answer.
    let roots = session(
        FORM_ONLY,
        &[ask(3, 1, "roots/list", "{}"), reply_error(4, 1, -32601)],
    );
    assert_eq!(subjects_for(CHECK, &roots), 0);
}

#[test]
fn an_input_response_is_neither_subject_nor_evidence() {
    // A handshake, a direct form elicitation refused `-32601`, and an MRTR
    // round answered with a result: the round is not a JSON-RPC answer, so
    // it neither counts nor excuses the refusal.
    let trace = session(
        FORM_ONLY,
        &[
            ask(3, 1, "elicitation/create", FORM),
            reply_error(4, 1, -32601),
            call(5, 7, FORM_ONLY, ""),
            input_required(6, 7, &entry(FORM)),
            call(
                7,
                8,
                FORM_ONLY,
                r#""inputResponses":{"a":{"action":"accept","content":{"name":"x"}}},"requestState":"st","#,
            ),
        ],
    );
    assert_eq!(subjects_for(CHECK, &trace), 1);
    assert_eq!(findings_for(CHECK, &trace).len(), 1);
}
