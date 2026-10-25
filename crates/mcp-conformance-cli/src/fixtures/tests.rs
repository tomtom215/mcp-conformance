// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use mcp_trace_validator::reader::{Limits, parse_trace};

use super::*;

/// A session of `(direction, payload)` messages, numbered in order.
fn session(messages: &[(&str, &str)]) -> Vec<TraceEvent> {
    let lines: Vec<String> = messages
        .iter()
        .enumerate()
        .map(|(seq, (direction, payload))| {
            format!(
                r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
            )
        })
        .collect();
    parse_trace(&lines.join("\n"), &Limits::default()).unwrap()
}

const C: &str = "client-to-server";
const S: &str = "server-to-client";

fn request(id: u32, method: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

fn result(id: u32, body: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"result":{body}}}"#)
}

fn find_in(sessions: &[Vec<TraceEvent>]) -> Option<Fixtures> {
    let views: Vec<&[TraceEvent]> = sessions.iter().map(Vec::as_slice).collect();
    find(&views)
}

/// A server whose own listings hold `echo`, `greet` and `file:///a`, driven by
/// a suite that asks for its fixtures instead.
fn fixture_run() -> Vec<Vec<TraceEvent>> {
    let listing = session(&[
        (C, &request(1, "tools/list", "{}")),
        (S, &result(1, r#"{"tools":[{"name":"echo"}]}"#)),
        (C, &request(2, "prompts/list", "{}")),
        (S, &result(2, r#"{"prompts":[{"name":"greet"}]}"#)),
        (C, &request(3, "resources/list", "{}")),
        (S, &result(3, r#"{"resources":[{"uri":"file:///a"}]}"#)),
        (C, &request(4, "resources/templates/list", "{}")),
        (S, &result(4, r#"{"resourceTemplates":[]}"#)),
    ]);
    let asking = session(&[
        (
            C,
            &request(1, "tools/call", r#"{"name":"test_simple_text"}"#),
        ),
        (C, &request(2, "tools/call", r#"{"name":"echo"}"#)),
        (
            C,
            &request(3, "prompts/get", r#"{"name":"test_simple_prompt"}"#),
        ),
        (
            C,
            &request(4, "resources/read", r#"{"uri":"test://static-text"}"#),
        ),
        (
            C,
            &request(5, "resources/subscribe", r#"{"uri":"test://watched"}"#),
        ),
        (
            C,
            &request(6, "resources/unsubscribe", r#"{"uri":"test://watched"}"#),
        ),
        (C, &request(7, "resources/read", r#"{"uri":"file:///a"}"#)),
    ]);
    let other = session(&[(
        C,
        &request(1, "tools/call", r#"{"name":"test_image_content"}"#),
    )]);
    let clean = session(&[(C, &request(1, "tools/call", r#"{"name":"echo"}"#))]);
    vec![listing, asking, other, clean]
}

#[test]
fn a_name_no_listing_holds_is_reported_with_the_sessions_that_asked() {
    let found = find_in(&fixture_run()).unwrap();
    assert_eq!(
        found.unlisted.tools,
        ["test_image_content", "test_simple_text"]
    );
    assert_eq!(found.unlisted.prompts, ["test_simple_prompt"]);
    assert_eq!(
        found.unlisted.resources,
        ["test://static-text", "test://watched"],
        "read, subscribe and unsubscribe all name a resource, and each name is counted once"
    );
    assert_eq!(found.unlisted.len(), 5);
    assert_eq!(found.judged_sessions, 4);
    assert_eq!(
        found.sessions, 2,
        "the asking session and the one for the image"
    );
}

#[test]
fn a_run_that_asks_for_nothing_missing_reports_nothing() {
    let listing = session(&[
        (C, &request(1, "tools/list", "{}")),
        (S, &result(1, r#"{"tools":[{"name":"echo"}]}"#)),
        (C, &request(2, "tools/call", r#"{"name":"echo"}"#)),
    ]);
    assert_eq!(find_in(&[listing]), None);
    assert_eq!(find_in(&[]), None);
}

/// A name the listing of *another* session holds is the server's: the
/// listing is the server's, not the session's.
#[test]
fn a_listing_in_any_session_of_the_run_counts() {
    let run = fixture_run();
    let listing = run[0].clone();
    let late = session(&[
        (C, &request(1, "tools/list", "{}")),
        (
            S,
            &result(1, r#"{"tools":[{"name":"test_image_content"}]}"#),
        ),
    ]);
    let found = find_in(&[listing, run[1].clone(), run[2].clone(), late]).unwrap();
    assert_eq!(found.unlisted.tools, ["test_simple_text"]);
    assert_eq!(found.sessions, 1);
}

/// Without the server's own listing, a request for a name shows nothing.
#[test]
fn a_kind_with_no_listing_answer_is_not_assessed() {
    let run = fixture_run();
    // Drop the listing session: nothing the server listed is known.
    assert_eq!(find_in(&run[1..]), None);
    // A listing request the server answered with an error is no listing.
    let refused = session(&[
        (C, &request(1, "tools/list", "{}")),
        (
            S,
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#,
        ),
        (
            C,
            &request(2, "tools/call", r#"{"name":"test_simple_text"}"#),
        ),
    ]);
    assert_eq!(find_in(&[refused]), None);
    // Nor is a result carrying another request's id a listing.
    let mismatched = session(&[
        (C, &request(1, "tools/list", "{}")),
        (S, &result(9, r#"{"tools":[]}"#)),
        (
            C,
            &request(2, "tools/call", r#"{"name":"test_simple_text"}"#),
        ),
    ]);
    assert_eq!(find_in(&[mismatched]), None);
    // But an empty listing is an answer: nothing is offered.
    let empty = session(&[
        (C, &request(1, "tools/list", "{}")),
        (S, &result(1, r#"{"tools":[]}"#)),
        (
            C,
            &request(2, "tools/call", r#"{"name":"test_simple_text"}"#),
        ),
    ]);
    assert_eq!(
        find_in(&[empty]).unwrap().unlisted.tools,
        ["test_simple_text"]
    );
}

/// A listing that says there is more may not hold the name.
#[test]
fn a_kind_whose_listing_was_paged_is_not_assessed() {
    let paged = |cursor: &str| {
        session(&[
            (C, &request(1, "tools/list", "{}")),
            (
                S,
                &result(1, &format!(r#"{{"tools":[],"nextCursor":{cursor}}}"#)),
            ),
            (
                C,
                &request(2, "tools/call", r#"{"name":"test_simple_text"}"#),
            ),
        ])
    };
    assert_eq!(find_in(&[paged(r#""page2""#)]), None);
    assert!(
        find_in(&[paged(r#""""#)]).is_some(),
        "an empty cursor is the end"
    );
    assert!(find_in(&[paged("null")]).is_some(), "so is a null one");
}

#[test]
fn a_resource_is_unlisted_only_when_no_listed_uri_or_template_could_serve_it() {
    let run = |templates: &str| {
        vec![
            session(&[
                (C, &request(1, "resources/list", "{}")),
                (S, &result(1, r#"{"resources":[{"uri":"file:///a"}]}"#)),
                (C, &request(2, "resources/templates/list", "{}")),
                (
                    S,
                    &result(2, &format!(r#"{{"resourceTemplates":[{templates}]}}"#)),
                ),
            ]),
            session(&[
                (
                    C,
                    &request(1, "resources/read", r#"{"uri":"test://template/123/data"}"#),
                ),
                (C, &request(2, "resources/read", r#"{"uri":"file:///a"}"#)),
            ]),
        ]
    };
    let unlisted = |templates: &str| find_in(&run(templates)).map(|found| found.unlisted.resources);
    // No template: the asked-for URI is not served.
    assert_eq!(
        unlisted(""),
        Some(vec!["test://template/123/data".to_owned()])
    );
    // A template that the URI expands: served, so nothing is missing.
    assert_eq!(
        unlisted(r#"{"uriTemplate":"test://template/{id}/data"}"#),
        None
    );
    // One it does not expand.
    assert_eq!(
        unlisted(r#"{"uriTemplate":"test://dynamic/{id}"}"#),
        Some(vec!["test://template/123/data".to_owned()])
    );
    // A template with an operator is not interpreted, so it might serve it.
    assert_eq!(unlisted(r#"{"uriTemplate":"test://{+path}"}"#), None);
    // Any one template serving it is enough.
    assert_eq!(
        unlisted(
            r#"{"uriTemplate":"test://dynamic/{id}"},{"uriTemplate":"test://template/{id}/data"}"#
        ),
        None
    );
}

// The templates below are RFC 6570 text, not format strings.
#[allow(clippy::literal_string_with_formatting_args)]
#[test]
fn templates_are_taken_apart_only_when_they_are_plain() {
    use Piece::{Literal, Variable};
    let lit = |text: &str| Literal(text.to_owned());
    assert_eq!(pieces("a://b"), Some(vec![lit("a://b")]));
    assert_eq!(
        pieces("a://{x}/c/{y}"),
        Some(vec![lit("a://"), Variable, lit("/c/"), Variable])
    );
    assert_eq!(pieces("{x}"), Some(vec![Variable]));
    assert_eq!(pieces(""), Some(vec![]));
    for not_plain in [
        "a://{+x}",
        "a://{x*}",
        "a://{x:3}",
        "a://{?x,y}",
        "a://{/x}",
        "a://{}",
        "a://{x",
        "a://x}",
        "a://{{x}}",
    ] {
        assert_eq!(pieces(not_plain), None, "{not_plain}");
    }
}

#[test]
fn a_variable_stands_for_one_or_more_characters() {
    let template = |text: &str| pieces(text).unwrap();
    assert!(matches(&template("a://{x}/d"), "a://1/d"));
    assert!(
        matches(&template("a://{x}/d"), "a://1/2/d"),
        "any text, slashes too"
    );
    assert!(
        !matches(&template("a://{x}/d"), "a:///d"),
        "but not nothing"
    );
    assert!(!matches(&template("a://{x}/d"), "a://1/e"));
    assert!(!matches(&template("a://{x}/d"), "b://1/d"));
    assert!(matches(&template("{x}{y}"), "ab"));
    assert!(!matches(&template("{x}{y}"), "a"));
    assert!(matches(&template("same"), "same"));
    assert!(!matches(&template("same"), "same!"));
    assert!(
        matches(&template("a://{x}"), "a://é"),
        "characters, not bytes"
    );
}
