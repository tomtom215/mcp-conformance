// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

use super::super::testkit::{Session, ping, pong};
use super::is_web_origin;

const CHECK: &str = "transport.invalid-origin-403";

const EVIL: &str = r#"{"host":"localhost:3000","origin":"http://evil.example.com"}"#;
const LOCAL: &str = r#"{"host":"localhost:3000","origin":"http://localhost:3000"}"#;
const NO_ORIGIN: &str = r#"{"host":"localhost:3000"}"#;

/// A `ping` from `EVIL` answered `status`, then the same `ping` under
/// `twin_headers` served.
fn refused(status: u16, twin_headers: &str) -> Session {
    Session::default()
        .post(EVIL, &ping(1), status)
        .post(twin_headers, &ping(2), 200)
        .answers(&pong(2))
}

#[test]
fn an_origin_refused_403_passes() {
    assert_eq!(refused(403, LOCAL).clean(CHECK), 1);
    // The twin may equally carry no Origin at all.
    assert_eq!(refused(403, NO_ORIGIN).clean(CHECK), 1);
}

#[test]
fn an_origin_refused_with_another_status_is_a_violation() {
    let finding = refused(400, LOCAL).one(CHECK);
    assert!(finding.contains("request at seq 0"), "{finding}");
    assert!(
        finding.contains(r#"Origin "http://evil.example.com""#),
        "{finding}"
    );
    assert!(finding.contains("refused HTTP 400"), "{finding}");
    assert!(
        finding.contains("answered HTTP 200 at seq 5"),
        "names the twin: {finding}"
    );
    refused(404, NO_ORIGIN).one(CHECK);
}

#[test]
fn a_refusal_the_origin_does_not_explain_is_not_judged() {
    // Served: the origin was not invalid to this server.
    assert_eq!(refused(200, LOCAL).clean(CHECK), 0);
    // Statuses with a cause of their own.
    for unexplained in [401, 408, 429, 500] {
        assert_eq!(refused(unexplained, LOCAL).clean(CHECK), 0, "{unexplained}");
    }
    // No twin: nothing says the origin was the reason.
    let alone = Session::default().post(EVIL, &ping(1), 400);
    assert_eq!(alone.clean(CHECK), 0);
    // The twin changes Host as well, as the DNS-rebinding traces do.
    let other_host = r#"{"host":"example.org","origin":"http://localhost:3000"}"#;
    assert_eq!(refused(400, other_host).clean(CHECK), 0);
    // The "twin" carries the very same origin.
    let same = Session::default()
        .post(EVIL, &ping(1), 400)
        .post(EVIL, &ping(2), 200);
    assert_eq!(same.clean(CHECK), 0);
}

#[test]
fn a_request_without_a_well_formed_origin_is_outside_the_clause() {
    // No Origin on the refused request: "present" is the clause's antecedent.
    let absent = Session::default()
        .post(NO_ORIGIN, &ping(1), 400)
        .post(LOCAL, &ping(2), 200);
    assert_eq!(absent.clean(CHECK), 0);
    // A malformed value may be refused as malformed HTTP.
    let malformed = Session::default()
        .post(
            r#"{"host":"localhost:3000","origin":"not an origin"}"#,
            &ping(1),
            400,
        )
        .post(NO_ORIGIN, &ping(2), 200);
    assert_eq!(malformed.clean(CHECK), 0);
}

#[test]
fn web_origin_syntax_is_exact() {
    for valid in [
        "http://localhost",
        "https://example.com:8443",
        "http://[::1]:3000",
        "http://a-b.example",
    ] {
        assert!(is_web_origin(valid), "{valid:?}");
    }
    for invalid in [
        "",
        "null",
        "http://",
        "https://",
        "ftp://example.com",
        "example.com",
        "http://example.com/path",
        "http://exa mple.com",
        "http://example.com?x",
    ] {
        assert!(!is_web_origin(invalid), "{invalid:?}");
    }
}
