// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check` against a protected server: the fetch of its Protected Resource
//! Metadata, what the recording keeps of it, and how it is judged.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write as _;
use std::net::SocketAddr;

use serde_json::Value;

use super::check_auth::{GOOD, check, outcome, probes, read_request, response, served};
use super::*;

const AT_ENDPOINT: &str = "/.well-known/oauth-protected-resource/mcp";
const AT_ROOT: &str = "/.well-known/oauth-protected-resource";
const DOCUMENT: &str =
    r#"{"resource":"http://guarded.example/mcp","authorization_servers":["https://as.example"]}"#;

/// How the stand-in serves its metadata.
#[derive(Clone, Copy)]
struct Metadata {
    /// Whether its challenge names the endpoint's well-known URL.
    named: bool,
    /// What it answers at the endpoint's well-known path, and at the root's:
    /// a document, or `None` for `404`.
    at_endpoint: Option<&'static str>,
    at_root: Option<&'static str>,
}

/// The answer to a request for `path`, when it is one for metadata. A
/// request that brings a credential there is answered `400`: none should.
fn metadata_answer(metadata: Metadata, head: &str, path: &str) -> Option<String> {
    let document = match path {
        AT_ENDPOINT => metadata.at_endpoint,
        AT_ROOT => metadata.at_root,
        _ if path.starts_with("/.well-known/") => None,
        _ => return None,
    };
    if head.to_ascii_lowercase().contains("\nauthorization:") {
        return Some(response("400 Bad Request", None, ""));
    }
    Some(document.map_or_else(
        || response("404 Not Found", None, ""),
        |document| response("200 OK", None, document),
    ))
}

/// A server that wants [`GOOD`] on its endpoint and serves `metadata`.
fn guarded(metadata: Metadata) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let challenge = if metadata.named {
        format!(r#"Bearer resource_metadata="http://{address}{AT_ENDPOINT}""#)
    } else {
        "Bearer".to_owned()
    };
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (head, body) = read_request(&mut stream);
            let path = head.split(' ').nth(1).unwrap_or_default();
            let credential = format!("authorization: {GOOD}");
            let authorized = head
                .lines()
                .any(|line| line.eq_ignore_ascii_case(&credential));
            let answer = metadata_answer(metadata, &head, path).unwrap_or_else(|| {
                if authorized {
                    served(&body)
                } else {
                    response("401 Unauthorized", Some(&challenge), "")
                }
            });
            let _ = stream.write_all(answer.as_bytes());
        }
    });
    address
}

/// Runs `check` with the good credential; the output, the probes and the
/// recording.
fn run(metadata: Metadata, name: &str) -> (Output, Vec<String>, Value, String) {
    let header = format!("Authorization: {GOOD}");
    let (output, summary, recorded) = check(guarded(metadata), name, &["--header", &header]);
    (output, probes(&summary), summary, recorded)
}

fn rows(name: &str, ids: &[&str]) -> Option<String> {
    outcome(&scratch_path(name).join("t.jsonl"), ids)
}

#[test]
fn the_metadata_a_challenge_names_is_fetched_recorded_and_judged() {
    let metadata = Metadata {
        named: true,
        at_endpoint: Some(DOCUMENT),
        at_root: None,
    };
    let (output, probes, summary, recorded) = run(metadata, "check-metadata-named");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let fetched =
        format!("the Protected Resource Metadata the challenge names ({AT_ENDPOINT}) — served");
    assert!(probes.contains(&fetched), "{probes:?}");
    // The fetch is a request sent, and is counted as one.
    assert_eq!(summary["requests"], 7, "{summary}");
    // The recording holds the path asked for, on the request and its answer,
    // the document, and no other path: not the endpoint's.
    let path = format!(r#""path":"{AT_ENDPOINT}""#);
    assert_eq!(recorded.matches(&path).count(), 2, "{recorded}");
    assert_eq!(recorded.matches(r#""path":"#).count(), 2, "{recorded}");
    assert!(
        recorded.contains(r#""document":{"#) && recorded.contains("https://as.example"),
        "{recorded}"
    );
    for ids in [["AUTH-006", "AUTH-091"], ["AUTH-011", "AUTH-142"]] {
        assert_eq!(
            rows("check-metadata-named", &ids).as_deref(),
            Some("pass"),
            "{ids:?}"
        );
    }
}

#[test]
fn a_named_url_that_serves_nothing_fails_the_server() {
    let metadata = Metadata {
        named: true,
        at_endpoint: None,
        at_root: Some(DOCUMENT),
    };
    let (output, probes, _, _) = run(metadata, "check-metadata-missing");
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    // The URL the server named is the one asked; the root is not tried.
    let asked: Vec<_> = probes
        .iter()
        .filter(|probe| probe.contains("Protected Resource Metadata"))
        .collect();
    assert_eq!(asked.len(), 1, "{probes:?}");
    assert!(asked[0].ends_with("— answered HTTP 404"), "{probes:?}");
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        report.contains("FAIL  AUTH-006") && report.contains("answered HTTP 404"),
        "{report}"
    );
    assert_eq!(
        rows("check-metadata-missing", &["AUTH-011"]).as_deref(),
        Some("not-observed")
    );
}

#[test]
fn a_document_that_names_no_authorization_server_fails_the_server() {
    let metadata = Metadata {
        named: true,
        at_endpoint: Some(r#"{"resource":"http://guarded.example/mcp"}"#),
        at_root: None,
    };
    let (output, _, _, _) = run(metadata, "check-metadata-empty");
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        report.contains("FAIL  AUTH-011") && report.contains("no `authorization_servers` member"),
        "{report}"
    );
    // It is served, which is the other clause.
    assert_eq!(
        rows("check-metadata-empty", &["AUTH-006"]).as_deref(),
        Some("pass")
    );
}

#[test]
fn with_no_url_named_the_well_known_locations_are_tried_until_one_serves() {
    let metadata = Metadata {
        named: false,
        at_endpoint: None,
        at_root: Some(DOCUMENT),
    };
    let (output, probes, summary, _) = run(metadata, "check-metadata-well-known");
    assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
    let tried: Vec<_> = probes
        .iter()
        .filter(|probe| probe.contains("well-known Protected Resource Metadata"))
        .cloned()
        .collect();
    assert_eq!(
        tried,
        [
            format!(
                "the well-known Protected Resource Metadata ({AT_ENDPOINT}) — answered HTTP 404"
            ),
            format!("the well-known Protected Resource Metadata ({AT_ROOT}) — served"),
        ]
    );
    // One fetch more than when the first location serves.
    assert_eq!(summary["requests"], 8, "{summary}");
    // A document came back and is judged; no URL was named to hold it to.
    let name = "check-metadata-well-known";
    assert_eq!(rows(name, &["AUTH-011"]).as_deref(), Some("pass"));
    assert_eq!(rows(name, &["AUTH-006"]).as_deref(), Some("not-observed"));
}
