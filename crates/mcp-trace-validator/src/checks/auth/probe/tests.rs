// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the invalid-token probe checks.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use mcp_conformance_core::trace::PROBE_AUTHORIZATION;

use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

const ACCEPTED: &str = "auth.invalid-token-not-accepted";
const CHALLENGED: &str = "auth.invalid-token-challenged";
const BEARER: &str =
    r#"Bearer resource_metadata="https://mcp.example/.well-known/oauth-protected-resource""#;
const BASIC: &str = r#"Basic realm="x""#;

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

/// One exchange starting at `seq`: a POST of `method` (JSON-RPC id `id`),
/// with `authorization` when given, answered `code` with `challenge`.
fn exchange(
    seq: u64,
    id: u64,
    method: &str,
    authorization: Option<&str>,
    code: u16,
    challenge: Option<&str>,
) -> Vec<String> {
    let mut headers = serde_json::json!({"content-type": "application/json"});
    if let Some(value) = authorization {
        headers["authorization"] = value.into();
    }
    let mut answered = serde_json::json!({});
    if let Some(value) = challenge {
        answered["www-authenticate"] = value.into();
    }
    vec![
        serde_json::json!({"seq": seq, "direction": "client-to-server",
            "transport": "streamable-http", "kind": "http", "method": "POST",
            "headers": headers})
        .to_string(),
        serde_json::json!({"seq": seq + 1, "direction": "client-to-server",
            "transport": "streamable-http", "kind": "message",
            "payload": {"jsonrpc": "2.0", "id": id, "method": method}})
        .to_string(),
        serde_json::json!({"seq": seq + 2, "direction": "server-to-client",
            "transport": "streamable-http", "kind": "http", "status": code,
            "headers": answered})
        .to_string(),
    ]
}

/// The unauthenticated `ping` refused with a challenge, then the probe.
fn probed(code: u16, challenge: Option<&str>) -> Vec<String> {
    let mut lines = exchange(0, 1, "ping", None, 401, Some(BEARER));
    lines.extend(exchange(
        3,
        2,
        "ping",
        Some(PROBE_AUTHORIZATION),
        code,
        challenge,
    ));
    lines
}

#[test]
fn a_refused_probe_passes_both_and_an_accepted_one_fails_validation() {
    let refused = probed(401, Some(BEARER));
    assert_eq!(run(ACCEPTED, &refused), (vec![], 1));
    assert_eq!(run(CHALLENGED, &refused), (vec![], 1));

    for served in [200, 202, 299] {
        let (found, subjects) = run(ACCEPTED, &probed(served, None));
        assert_eq!(subjects, 1);
        assert_eq!(found.len(), 1, "{served}: {found:?}");
        assert!(
            found[0].contains(&format!("HTTP {served}")) && found[0].contains("without validating"),
            "{found:?}"
        );
        // An accepted token is not a refusal to judge the form of.
        assert_eq!(run(CHALLENGED, &probed(served, None)), (vec![], 0));
    }
    // Just outside success on either side.
    assert_eq!(run(ACCEPTED, &probed(199, None)).0.len(), 0);
    assert_eq!(run(ACCEPTED, &probed(300, None)).0.len(), 0);
}

#[test]
fn a_refusal_is_401_with_a_bearer_challenge() {
    for (code, challenge, expected) in [
        (403, Some(BEARER), Some("with HTTP 403")),
        (400, None, Some("with HTTP 400")),
        (499, None, Some("with HTTP 499")),
        (401, None, Some("no `WWW-Authenticate: Bearer` challenge")),
        (
            401,
            Some(BASIC),
            Some("no `WWW-Authenticate: Bearer` challenge"),
        ),
        (401, Some(BEARER), None),
    ] {
        let (found, subjects) = run(CHALLENGED, &probed(code, challenge));
        assert_eq!(subjects, 1, "{code}");
        match expected {
            Some(text) => {
                assert_eq!(found.len(), 1, "{code}: {found:?}");
                assert!(found[0].contains(text), "{found:?}");
            }
            None => assert!(found.is_empty(), "{found:?}"),
        }
        // A refusal of any kind is not an acceptance.
        assert_eq!(run(ACCEPTED, &probed(code, challenge)), (vec![], 1));
    }
    // The server could not answer: nothing about how it refuses.
    for code in [408, 429, 500, 503, 399] {
        assert_eq!(run(CHALLENGED, &probed(code, None)), (vec![], 0), "{code}");
    }
}

#[test]
fn only_a_protected_operation_carrying_exactly_the_probe_is_a_subject() {
    let accepted = |lines: &[String]| run(ACCEPTED, lines);
    // No twin at all: the server may serve this to anybody.
    let alone = exchange(0, 1, "ping", Some(PROBE_AUTHORIZATION), 200, None);
    assert_eq!(accepted(&alone), (vec![], 0));
    // A twin that was served, refused with another status, or challenged
    // with another scheme does not make the operation a protected one.
    for (code, challenge) in [
        (200, None),
        (403, Some(BEARER)),
        (401, None),
        (401, Some(BASIC)),
    ] {
        let mut lines = exchange(0, 1, "ping", None, code, challenge);
        lines.extend(exchange(3, 2, "ping", Some(PROBE_AUTHORIZATION), 200, None));
        assert_eq!(accepted(&lines), (vec![], 0), "{code} {challenge:?}");
    }
    // The 401 was for another request.
    let mut other = exchange(0, 1, "tools/list", None, 401, Some(BEARER));
    other.extend(exchange(3, 2, "ping", Some(PROBE_AUTHORIZATION), 200, None));
    assert_eq!(accepted(&other), (vec![], 0));
    // Any other credential is never recorded; were one, it is not the probe.
    let mut real = exchange(0, 1, "ping", None, 401, Some(BEARER));
    real.extend(exchange(
        3,
        2,
        "ping",
        Some("Bearer something-else"),
        200,
        None,
    ));
    assert_eq!(accepted(&real), (vec![], 0));
    // The twin may come after the probe.
    let mut later = exchange(0, 1, "ping", Some(PROBE_AUTHORIZATION), 200, None);
    later.extend(exchange(3, 2, "ping", None, 401, Some(BEARER)));
    assert_eq!(accepted(&later).1, 1);
}

#[test]
fn the_twin_differs_only_in_the_credential_and_the_id() {
    // Another recorded header on one side only: not the same request.
    let mut lines = exchange(0, 1, "ping", None, 401, Some(BEARER));
    let mut probe = exchange(3, 2, "ping", Some(PROBE_AUTHORIZATION), 200, None);
    probe[0] = probe[0].replace(
        r#""content-type":"application/json""#,
        r#""content-type":"application/json","mcp-session-id":"s""#,
    );
    lines.extend(probe);
    assert_eq!(run(ACCEPTED, &lines), (vec![], 0));
    // A different HTTP method: not the same request.
    let mut lines = exchange(0, 1, "ping", None, 401, Some(BEARER));
    lines[0] = lines[0].replace(r#""method":"POST""#, r#""method":"GET""#);
    lines.extend(exchange(3, 2, "ping", Some(PROBE_AUTHORIZATION), 200, None));
    assert_eq!(run(ACCEPTED, &lines), (vec![], 0));
}
