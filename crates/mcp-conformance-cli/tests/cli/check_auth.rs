// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check` against a server that protects itself with a bearer token: the
//! authorization probe, and what the recording keeps of it.

#![allow(clippy::expect_used, clippy::unwrap_used)]
// `unreachable_pub` asks for and clippy's `redundant_pub_crate` objects to;
// this follows the rustc lint, as the sibling modules do.
#![allow(clippy::redundant_pub_crate)]

use std::io::{Read as _, Write as _};
use std::net::SocketAddr;
use std::path::Path;

use mcp_conformance_core::trace::PROBE_AUTHORIZATION;
use serde_json::{Value, json};

use super::*;

/// The credential the stand-in server accepts.
pub(crate) const GOOD: &str = "Bearer the-real-credential";

const CHALLENGE: &str =
    r#"Bearer resource_metadata="http://127.0.0.1/.well-known/oauth-protected-resource""#;

/// One whole HTTP response.
pub(crate) fn response(status: &str, challenge: Option<&str>, body: &str) -> String {
    let challenge = challenge.map_or_else(String::new, |value| {
        format!("WWW-Authenticate: {value}\r\n")
    });
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{challenge}Content-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )
}

/// What a legacy server with nothing to offer answers `body` with.
fn served(body: &str) -> String {
    let message: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let id = message.get("id").cloned();
    let answer = match (message.get("method").and_then(Value::as_str), id) {
        (Some("initialize"), Some(id)) => json!({"jsonrpc": "2.0", "id": id, "result": {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "serverInfo": {"name": "guarded", "version": "1"}}}),
        (Some("ping"), Some(id)) => json!({"jsonrpc": "2.0", "id": id, "result": {}}),
        (Some(_), Some(id)) => json!({"jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": "Method not found"}}),
        _ => return response("202 Accepted", None, ""),
    };
    response("200 OK", None, &answer.to_string())
}

/// Reads one request: its head and its body.
pub(crate) fn read_request(stream: &mut std::net::TcpStream) -> (String, String) {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).unwrap_or(0);
        request.extend_from_slice(&buffer[..read]);
        let text = String::from_utf8_lossy(&request).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let expected = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if body.len() >= expected || read == 0 {
                return (head.to_owned(), body.to_owned());
            }
        } else if read == 0 {
            return (text, String::new());
        }
    }
}

/// A server that wants [`GOOD`]: no credential is refused `401` with a
/// challenge, and the probe credential is refused as an invalid token —
/// or, when `accepts_anything`, served.
fn guarded(accepts_anything: bool) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (head, body) = read_request(&mut stream);
            let credential = head.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("authorization")
                    .then(|| value.trim().to_owned())
            });
            let answer = match credential.as_deref() {
                None => response("401 Unauthorized", Some(CHALLENGE), ""),
                Some(GOOD) => served(&body),
                Some(_) if accepts_anything => served(&body),
                Some(_) => response(
                    "401 Unauthorized",
                    Some(&format!(r#"{CHALLENGE}, error="invalid_token""#)),
                    "",
                ),
            };
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    address
}

/// Runs `check` against `address` with `flags`; the output, the summary and
/// the recording's text. Five seconds an answer unless `flags` say otherwise.
pub(crate) fn check(address: SocketAddr, name: &str, flags: &[&str]) -> (Output, Value, String) {
    let dir = scratch(name);
    let (trace, summary) = (dir.join("t.jsonl"), dir.join("s.json"));
    let mut command = binary();
    command.arg("check").args(flags);
    if !flags.contains(&"--timeout") {
        command.args(["--timeout", "5"]);
    }
    let output = command
        .arg("--url")
        .arg(format!("http://{address}/mcp"))
        .arg("-o")
        .arg(&trace)
        .arg("--summary")
        .arg(&summary)
        .output()
        .unwrap();
    let summary = serde_json::from_slice(&std::fs::read(&summary).unwrap()).unwrap();
    (output, summary, std::fs::read_to_string(&trace).unwrap())
}

/// The outcome of the first of `ids` the report of `trace` has a row for.
fn outcome(trace: &Path, ids: &[&str]) -> Option<String> {
    let report = binary()
        .args(["validate", "--party", "server", "--format", "json"])
        .arg(trace)
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&report.stdout).unwrap();
    report["requirements"]
        .as_array()?
        .iter()
        .find(|row| ids.contains(&row["id"].as_str().unwrap_or_default()))
        .map(|row| row["outcome"].as_str().unwrap().to_owned())
}

pub(crate) fn probes(summary: &Value) -> Vec<String> {
    summary["probes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|probe| {
            format!(
                "{} — {}",
                probe["what"].as_str().unwrap(),
                probe["answer"].as_str().unwrap()
            )
        })
        .collect()
}

const KNOCKS: [&str; 2] = [
    "a request with no credential — refused with HTTP 401",
    "the same request with a token nobody issued — refused with HTTP 401",
];

#[test]
fn a_server_that_wants_a_credential_is_knocked_on_and_how_it_refuses_is_judged() {
    let address = guarded(false);
    let (output, summary, recorded) = check(address, "check-auth-none", &[]);
    let said = stderr(&output);
    // Still a run that could not be made, and it says what to pass.
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(
        said.contains("the server requires authorization")
            && said.contains("--header 'Authorization: Bearer <token>'"),
        "{said}"
    );
    assert!(!said.contains("no session was established:"), "{said}");
    assert_eq!(probes(&summary), KNOCKS, "{summary}");
    // Each knock is a request sent, and is counted as one.
    assert_eq!(summary["requests"], 4, "{summary}");
    // The probe credential is in the recording, whole, exactly once.
    assert_eq!(
        recorded.matches(PROBE_AUTHORIZATION).count(),
        1,
        "{recorded}"
    );
    // How it refused is judged, not left unobserved.
    let trace = scratch_path("check-auth-none").join("t.jsonl");
    for ids in [["AUTH-042", "AUTH-121"], ["AUTH-044", "AUTH-123"]] {
        assert_eq!(outcome(&trace, &ids).as_deref(), Some("pass"), "{ids:?}");
    }
}

#[test]
fn with_a_credential_the_session_runs_and_the_probe_is_sent_beside_it() {
    let address = guarded(false);
    let header = format!("Authorization: {GOOD}");
    let (output, summary, recorded) = check(address, "check-auth-good", &["--header", &header]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(summary["revision"], "2025-11-25");
    let sent = probes(&summary);
    for knock in KNOCKS {
        assert!(sent.contains(&knock.to_owned()), "{sent:?}");
    }
    // The user's credential reached the server and no file: only the public
    // probe value is ever recorded.
    assert!(!recorded.contains("the-real-credential"), "{recorded}");
    assert!(!stderr(&output).contains("the-real-credential"));
    assert_eq!(
        recorded.matches(PROBE_AUTHORIZATION).count(),
        1,
        "{recorded}"
    );
    let trace = scratch_path("check-auth-good").join("t.jsonl");
    assert_eq!(outcome(&trace, &["AUTH-042"]).as_deref(), Some("pass"));
    assert_eq!(outcome(&trace, &["AUTH-044"]).as_deref(), Some("pass"));

    // Without probes nothing but the session is sent.
    let (output, summary, recorded) = check(
        address,
        "check-auth-quiet",
        &["--no-probes", "--header", &header],
    );
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert_eq!(summary["probes"], json!([]));
    assert!(!recorded.contains("authorization"), "{recorded}");
}

#[test]
fn a_server_that_serves_a_token_nobody_issued_fails() {
    let address = guarded(true);
    let header = format!("Authorization: {GOOD}");
    let (output, summary, _) = check(address, "check-auth-lax", &["--header", &header]);
    let said = stderr(&output);
    assert_eq!(output.status.code(), Some(1), "{said}");
    assert!(
        probes(&summary)
            .contains(&"the same request with a token nobody issued — served".to_owned()),
        "{summary}"
    );
    // The run says so in one plain sentence.
    assert!(
        said.contains("result: the server broke at least one rule")
            && !said.contains("broke no rule"),
        "{said}"
    );
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        report.contains("FAIL  AUTH-042") && report.contains("without validating the token"),
        "{report}"
    );
    let trace = scratch_path("check-auth-lax").join("t.jsonl");
    assert_eq!(outcome(&trace, &["AUTH-042"]).as_deref(), Some("fail"));
    // An accepted token is no refusal to judge the form of.
    assert_eq!(
        outcome(&trace, &["AUTH-044"]).as_deref(),
        Some("not-observed")
    );
}

#[test]
fn without_probes_a_server_that_wants_a_credential_is_only_a_run_that_could_not_be_made() {
    let address = guarded(false);
    let (output, summary, recorded) = check(address, "check-auth-none-quiet", &["--no-probes"]);
    let said = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{said}");
    // Nothing was knocked on, so there is no refusal to report on: the plain
    // failure, and no report.
    assert_eq!(summary["probes"], json!([]));
    assert!(
        said.contains("no session was established: the server answered HTTP 401"),
        "{said}"
    );
    assert!(!said.contains("requires authorization"), "{said}");
    assert!(output.stdout.is_empty(), "no report: {:?}", output.stdout);
    assert!(!recorded.contains("authorization"), "{recorded}");
}

#[test]
fn a_server_that_serves_everybody_is_sent_no_credential_probe() {
    let runtime = super::check::runtime();
    let address = super::check::serve(
        &runtime,
        mcp_everything_server::server::ServedRevision::V2025_11_25,
    );
    let (output, summary, recorded) = check(address, "check-auth-open", &[]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    assert!(
        !probes(&summary)
            .iter()
            .any(|probe| probe.contains("credential")),
        "{summary}"
    );
    assert!(!recorded.contains("authorization"), "{recorded}");
}
