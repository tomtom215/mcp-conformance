// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the authorization checks and the challenge reader.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::challenge::{Challenge, parse};
use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

fn run(check: &str, lines: &[String]) -> (Vec<String>, u32) {
    let trace = lines.join("\n");
    let events = parse_trace(&trace, &Limits::default()).expect("test trace parses");
    let context = TraceContext::new(&events);
    let outcome = checks::find(check).expect("registered").run(&context);
    (
        outcome.findings.into_iter().map(|f| f.detail).collect(),
        outcome.subjects,
    )
}

fn post(seq: u64) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{{"content-type":"application/json"}}}}"#
    )
}

fn request(seq: u64, id: u64, method: &str) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{{"jsonrpc":"2.0","id":{id},"method":"{method}"}}}}"#
    )
}

/// A response status, with a `WWW-Authenticate` value when given.
fn status(seq: u64, code: u16, challenge: Option<&str>) -> String {
    let headers = challenge.map_or_else(String::new, |value| {
        format!(
            r#","headers":{{"www-authenticate":{}}}"#,
            serde_json::to_string(value).unwrap()
        )
    });
    format!(
        r#"{{"seq":{seq},"direction":"server-to-client","transport":"streamable-http","kind":"http","status":{code}{headers}}}"#
    )
}

/// One refused exchange: `ping` (id `id`), sent by POST, answered with `code`.
fn refused(first: u64, id: u64, code: u16, challenge: Option<&str>) -> Vec<String> {
    vec![
        post(first),
        request(first + 1, id, "ping"),
        status(first + 2, code, challenge),
    ]
}

fn names(challenges: &[Challenge]) -> Vec<(&str, Vec<(&str, &str)>)> {
    challenges
        .iter()
        .map(|challenge| {
            (
                challenge.scheme.as_str(),
                challenge
                    .params
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str()))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn challenges_parse_with_parameters_quoted_strings_and_token68() {
    let parsed = parse(
        r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource", scope="files:read""#,
    );
    assert_eq!(
        names(&parsed),
        [(
            "Bearer",
            vec![
                (
                    "resource_metadata",
                    "https://mcp.example.com/.well-known/oauth-protected-resource"
                ),
                ("scope", "files:read")
            ]
        )]
    );
    // Several challenges in one list, a token68, case-insensitive names,
    // whitespace around `=`, unquoted values and an escaped quote.
    let parsed = parse(
        r#"Basic abc123==, Bearer Error = "insufficient_scope", Scope=a, realm="say \"hi\"", Negotiate"#,
    );
    assert_eq!(
        names(&parsed),
        [
            ("Basic", vec![]),
            (
                "Bearer",
                vec![
                    ("error", "insufficient_scope"),
                    ("scope", "a"),
                    ("realm", "say \"hi\"")
                ]
            ),
            ("Negotiate", vec![])
        ]
    );
    assert!(parsed[1].is_bearer());
    assert_eq!(parsed[1].param("scope"), Some("a"));
    assert_eq!(parsed[1].param("resource_metadata"), None);
    // A malformed stretch is skipped, never turned into a parameter.
    assert_eq!(names(&parse(r#"=x, "y", Bearer"#)), [("Bearer", vec![])]);
    assert_eq!(parse("").len(), 0);
}

#[test]
fn challenges_parse_tabs_padding_and_unterminated_strings() {
    // Tabs are whitespace around `=` and after a scheme; a padded token68
    // with trailing whitespace still ends its element.
    assert_eq!(
        names(&parse("Basic\tab/c+==\t, Bearer\tscope\t=\tx\t,realm=r")),
        [
            ("Basic", vec![]),
            ("Bearer", vec![("scope", "x"), ("realm", "r")])
        ]
    );
    // A token68 followed by more than a comma is not one: the token is a
    // parameter name.
    assert_eq!(
        names(&parse("Bearer abc= def")),
        [("Bearer", vec![("abc", "def")])]
    );
    // An unterminated quoted string runs to the end; a trailing backslash
    // escapes nothing.
    assert_eq!(
        names(&parse(r#"Bearer realm="a,b\"#)),
        [("Bearer", vec![("realm", "a,b")])]
    );
    assert_eq!(
        names(&parse(r#"Bearer realm="x\y", error="e""#)),
        [("Bearer", vec![("realm", "xy"), ("error", "e")])]
    );
    // A stray non-token byte skips only to the next comma.
    assert_eq!(
        names(&parse("Bearer a=1, @junk x, error=e")),
        [("Bearer", vec![("a", "1"), ("error", "e")])]
    );
}

#[test]
fn a_401_challenge_names_its_scope() {
    const CHECK: &str = "auth.challenge-scope";
    for (code, challenge, subjects, findings) in [
        (
            401,
            Some(r#"Bearer resource_metadata="u", scope="a b""#),
            1,
            0,
        ),
        (401, Some(r#"Bearer resource_metadata="u""#), 1, 1),
        (401, Some(r#"bearer error="invalid_token""#), 1, 1),
        // No challenge, another scheme, another status: not this clause's.
        (401, None, 0, 0),
        (401, Some(r#"Basic realm="x""#), 0, 0),
        (403, Some(r#"Bearer error="insufficient_scope""#), 0, 0),
    ] {
        let (found, seen) = run(CHECK, &refused(0, 1, code, challenge));
        assert_eq!((seen, found.len()), (subjects, findings), "{challenge:?}");
    }
}

#[test]
fn an_insufficient_scope_response_carries_every_listed_part() {
    const CHECK: &str = "auth.insufficient-scope-response";
    let good = r#"Bearer error="insufficient_scope", scope="a", resource_metadata="u""#;
    assert_eq!(run(CHECK, &refused(0, 1, 403, Some(good))), (vec![], 1));
    let (found, seen) = run(
        CHECK,
        &refused(0, 1, 401, Some(r#"Bearer error="insufficient_scope""#)),
    );
    assert_eq!(seen, 1);
    assert_eq!(
        found.len(),
        1,
        "one finding per response, listing each part"
    );
    for part in ["HTTP 401, not 403", "no `scope`", "no `resource_metadata`"] {
        assert!(found[0].contains(part), "{found:?}");
    }
    let (found, _) = run(
        CHECK,
        &refused(
            0,
            1,
            403,
            Some(r#"DPoP error="insufficient_scope", scope="a", resource_metadata="u""#),
        ),
    );
    assert!(found[0].contains(r#""DPoP" scheme"#), "{found:?}");
    // A challenge that declares another error, or none, is not a subject.
    assert_eq!(
        run(
            CHECK,
            &refused(0, 1, 401, Some(r#"Bearer error="invalid_token""#))
        )
        .1,
        0
    );
}

#[test]
fn an_insufficient_scope_challenge_names_scopes() {
    const CHECK: &str = "auth.insufficient-scope-names-scopes";
    for (challenge, findings) in [
        (
            r#"Bearer error="insufficient_scope", scope="files:write""#,
            0,
        ),
        (r#"Bearer error="insufficient_scope", scope="  ""#, 1),
        (r#"Bearer error="insufficient_scope""#, 1),
    ] {
        let (found, seen) = run(CHECK, &refused(0, 1, 403, Some(challenge)));
        assert_eq!((seen, found.len()), (1, findings), "{challenge}");
    }
}

#[test]
fn each_declared_error_gets_its_table_status() {
    const CHECK: &str = "auth.error-status";
    for (code, error, subjects, findings) in [
        (400, "invalid_request", 1, 0),
        (401, "invalid_token", 1, 0),
        (403, "insufficient_scope", 1, 0),
        (401, "invalid_request", 1, 1),
        (403, "invalid_token", 1, 1),
        (401, "insufficient_scope", 1, 1),
        // An error the table does not name is not a subject.
        (401, "something_else", 0, 0),
    ] {
        let challenge = format!(r#"Bearer error="{error}""#);
        let (found, seen) = run(CHECK, &refused(0, 1, code, Some(&challenge)));
        assert_eq!((seen, found.len()), (subjects, findings), "{code} {error}");
    }
    // No `error` at all: authorization required, no token sent.
    assert_eq!(run(CHECK, &refused(0, 1, 401, Some("Bearer"))).1, 0);
}

#[test]
fn an_invalid_token_gets_a_401() {
    const CHECK: &str = "auth.invalid-token-401";
    let invalid = Some(r#"Bearer error="invalid_token", scope="a""#);
    assert_eq!(run(CHECK, &refused(0, 1, 401, invalid)), (vec![], 1));
    let (found, seen) = run(CHECK, &refused(0, 1, 403, invalid));
    assert_eq!((seen, found.len()), (1, 1));
    assert!(found[0].contains("HTTP 403"), "{found:?}");
    assert_eq!(run(CHECK, &refused(0, 1, 401, Some("Bearer"))).1, 0);
}

#[test]
fn offline_access_is_never_challenged_for() {
    const CHECK: &str = "auth.no-offline-access-challenge";
    let clean = Some(r#"Bearer scope="files:read""#);
    assert_eq!(run(CHECK, &refused(0, 1, 401, clean)), (vec![], 1));
    let (found, seen) = run(
        CHECK,
        &refused(
            0,
            1,
            401,
            Some(r#"Bearer scope="files:read offline_access""#),
        ),
    );
    assert_eq!((seen, found.len()), (1, 1));
    // `offline_access_extra` is another scope.
    assert_eq!(
        run(
            CHECK,
            &refused(0, 1, 401, Some(r#"Bearer scope="offline_access_extra""#))
        ),
        (vec![], 1)
    );
    assert_eq!(run(CHECK, &refused(0, 1, 401, Some("Bearer"))).1, 0);
}

#[test]
fn a_retried_request_is_not_challenged_for_a_new_scope() {
    const CHECK: &str = "auth.single-challenge";
    let first = Some(r#"Bearer error="insufficient_scope", scope="a b", resource_metadata="u""#);
    let mut lines = refused(0, 7, 403, first);
    // Retried with the scopes asked for, and answered.
    lines.extend([post(3), request(4, 7, "ping"), status(5, 200, None)]);
    assert_eq!(
        run(CHECK, &lines),
        (vec![], 1),
        "a retry that succeeds is judged"
    );

    let mut lines = refused(0, 7, 403, first);
    lines.extend(refused(
        3,
        7,
        403,
        Some(r#"Bearer error="insufficient_scope", scope="a c""#),
    ));
    let (found, seen) = run(CHECK, &lines);
    assert_eq!(seen, 1);
    assert_eq!(found.len(), 1);
    assert!(found[0].contains("for c that"), "{found:?}");

    // The same scopes again (the grant fell short) is not incremental, and a
    // different request is not a retry.
    let mut lines = refused(0, 7, 403, first);
    lines.extend(refused(
        3,
        7,
        403,
        Some(r#"Bearer error="insufficient_scope", scope="b""#),
    ));
    lines.extend(refused(
        6,
        8,
        403,
        Some(r#"Bearer error="insufficient_scope", scope="z""#),
    ));
    assert_eq!(run(CHECK, &lines), (vec![], 1));
}

#[test]
fn a_request_refused_by_authorization_is_not_delivered() {
    // The retry of a refused request reuses its id, as the SDKs' clients do;
    // the refused copy never reached the protocol layer, so neither id rule
    // fires — while a request answered by a JSON-RPC error still counts.
    let mut lines = refused(0, 1, 401, Some(r#"Bearer scope="a""#));
    lines.extend([post(3), request(4, 1, "ping"), status(5, 200, None)]);
    for check in ["base.request-id-unique", "base.request-id-unique-in-flight"] {
        let (found, seen) = run(check, &lines);
        assert!(found.is_empty(), "{check}: {found:?}");
        assert_eq!(seen, 1, "{check}: the retry is the one request judged");
    }
    let mut answered = refused(0, 1, 400, None);
    answered.insert(
        3,
        r#"{"seq":3,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"bad"}}}"#
            .to_owned(),
    );
    answered.extend([post(4), request(5, 1, "ping"), status(6, 200, None)]);
    assert_eq!(run("base.request-id-unique", &answered).0.len(), 1);
    // A 403 without a challenge is not an authorization refusal.
    let mut forbidden = refused(0, 1, 403, None);
    forbidden.extend([post(3), request(4, 1, "ping"), status(5, 200, None)]);
    assert_eq!(run("base.request-id-unique", &forbidden).0.len(), 1);
}

#[test]
fn a_refusal_body_is_not_a_protocol_message() {
    // A resource server's own error object in the body of its 401, as some
    // middleware sends: recorded as a message event, it is neither a JSON-RPC
    // error nor an answer to the request, which is still refused.
    let mut lines = refused(0, 1, 401, Some(r#"Bearer error="invalid_token""#));
    lines.push(
        r#"{"seq":3,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"error":"invalid_token","error_description":"expired"}}"#
            .to_owned(),
    );
    lines.extend([post(4), request(5, 1, "ping"), status(6, 200, None)]);
    for check in [
        "base.request-id-unique",
        "base.error-shape",
        "base.jsonrpc-version",
        "base.error-id-matches",
    ] {
        assert!(run(check, &lines).0.is_empty(), "{check}");
    }
    assert_eq!(run("auth.invalid-token-401", &lines), (vec![], 1));
    // The same object after a success status is a server message like any
    // other, and fails as one.
    let mut answered = vec![post(0), request(1, 1, "ping"), status(2, 200, None)];
    answered.push(lines[3].clone());
    assert_eq!(run("base.jsonrpc-version", &answered).0.len(), 1);
}

#[test]
fn a_jsonrpc_answer_to_a_401_is_no_refusal_body() {
    // A JSON-RPC error in the body of a 401 answers the request: the server
    // read it, so its retry reuses the id like any other.
    let mut lines = refused(0, 1, 401, Some(r#"Bearer error="invalid_token""#));
    lines.push(
        r#"{"seq":3,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"unauthorized"}}}"#
            .to_owned(),
    );
    lines.extend([post(4), request(5, 1, "ping"), status(6, 200, None)]);
    assert_eq!(run("base.request-id-unique", &lines).0.len(), 1);
}
