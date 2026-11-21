// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the Protected Resource Metadata checks.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::{Value, json};

use super::url_path;
use crate::checks;
use crate::context::TraceContext;
use crate::reader::{Limits, parse_trace};

const SERVED: &str = "auth.resource-metadata-served";
const SERVERS: &str = "auth.resource-metadata-names-servers";
const PATH: &str = "/.well-known/oauth-protected-resource/mcp";

fn run(check: &str, lines: &[Value]) -> (Vec<String>, u32) {
    let text: Vec<String> = lines.iter().map(Value::to_string).collect();
    let events = parse_trace(&text.join("\n"), &Limits::default()).expect("test trace parses");
    let (rest, fetches) = crate::fetches::split(&events);
    let context = TraceContext::new(&rest).with_fetches(fetches);
    let outcome = checks::find(check).expect("registered").run(&context);
    (
        outcome.findings.into_iter().map(|f| f.detail).collect(),
        outcome.subjects,
    )
}

/// A `401` whose challenge is `challenge`, at `seq`.
fn refused(seq: u64, challenge: &str) -> Value {
    json!({"seq": seq, "direction": "server-to-client", "transport": "streamable-http",
        "kind": "http", "status": 401, "headers": {"www-authenticate": challenge}})
}

fn naming(url: &str) -> String {
    format!(r#"Bearer resource_metadata="{url}""#)
}

/// A fetch of `path` at `seq` and `seq + 1`, answered `status` with `document`.
fn fetch(seq: u64, path: &str, status: u16, document: Option<Value>) -> [Value; 2] {
    let mut answer = json!({"seq": seq + 1, "direction": "server-to-client",
        "transport": "streamable-http", "kind": "http", "status": status, "path": path});
    if let Some(document) = document {
        answer["document"] = document;
    }
    [
        json!({"seq": seq, "direction": "client-to-server", "transport": "streamable-http",
            "kind": "http", "method": "GET", "path": path}),
        answer,
    ]
}

fn trace(challenge: &str, path: &str, status: u16, document: Option<Value>) -> Vec<Value> {
    let mut lines = vec![refused(0, challenge)];
    lines.extend(fetch(1, path, status, document));
    lines
}

fn good() -> Value {
    json!({"resource": "https://mcp.example/mcp",
        "authorization_servers": ["https://as.example"]})
}

#[test]
fn a_url_gives_up_its_path_and_nothing_else() {
    for (url, path) in [
        ("https://h/.well-known/x", Some("/.well-known/x")),
        ("http://h:8080/a/b?q=1#f", Some("/a/b")),
        ("https://h", Some("/")),
        ("https://h?q", Some("/")),
        ("https://h/#f", Some("/")),
        ("https:///path", None),
        ("/.well-known/x", None),
        ("ftp://h/x", None),
        ("", None),
    ] {
        assert_eq!(url_path(url), path, "{url}");
    }
}

#[test]
fn the_named_url_serving_a_json_object_passes_and_anything_else_fails() {
    let named = naming("https://mcp.example/.well-known/oauth-protected-resource/mcp");
    assert_eq!(
        run(SERVED, &trace(&named, PATH, 200, Some(good()))),
        (vec![], 1)
    );
    for (status, document, said) in [
        (404, None, "answered HTTP 404"),
        (401, None, "answered HTTP 401"),
        (400, Some(good()), "answered HTTP 400"),
        (204, None, "answered HTTP 204"),
        (299, Some(good()), "answered HTTP 299"),
        (499, None, "answered HTTP 499"),
        (200, None, "HTTP 200 without a JSON object"),
        (200, Some(json!(["x"])), "HTTP 200 without a JSON object"),
    ] {
        let (found, subjects) = run(SERVED, &trace(&named, PATH, status, document));
        assert_eq!(subjects, 1, "{status}");
        assert_eq!(found.len(), 1, "{status}: {found:?}");
        assert!(
            found[0].contains(said) && found[0].contains(PATH),
            "{found:?}"
        );
    }
}

#[test]
fn a_server_that_could_not_answer_is_not_judged_on_how_it_answers() {
    let named = naming("https://mcp.example/.well-known/oauth-protected-resource/mcp");
    for status in [199, 300, 302, 399, 408, 429, 500, 503] {
        assert_eq!(
            run(SERVED, &trace(&named, PATH, status, None)),
            (vec![], 0),
            "{status}"
        );
    }
}

#[test]
fn only_a_fetch_of_the_path_a_bearer_challenge_names_is_a_subject() {
    let named = naming("https://mcp.example/.well-known/oauth-protected-resource/mcp");
    // Another path was fetched.
    let other = trace(&named, "/.well-known/oauth-protected-resource", 404, None);
    assert_eq!(run(SERVED, &other), (vec![], 0));
    // No challenge names a URL, or the one that does is not Bearer's.
    for challenge in [
        "Bearer scope=\"a\"".to_owned(),
        format!(r#"Basic resource_metadata="https://mcp.example{PATH}""#),
        naming("not a url"),
    ] {
        assert_eq!(
            run(SERVED, &trace(&challenge, PATH, 404, None)),
            (vec![], 0)
        );
    }
    // Asked and never answered.
    let mut unanswered = trace(&named, PATH, 404, None);
    unanswered.pop();
    assert_eq!(run(SERVED, &unanswered), (vec![], 0));
    // Two challenges naming one URL, fetched twice: each fetch once.
    let mut twice = trace(&named, PATH, 200, Some(good()));
    twice.push(refused(3, &named));
    twice.extend(fetch(4, PATH, 404, None));
    let (found, subjects) = run(SERVED, &twice);
    assert_eq!((found.len(), subjects), (1, 2), "{found:?}");
}

#[test]
fn a_returned_document_names_at_least_one_authorization_server() {
    let none = "Bearer";
    assert_eq!(
        run(SERVERS, &trace(none, PATH, 200, Some(good()))),
        (vec![], 1)
    );
    for (document, said) in [
        (
            json!({"resource": "r"}),
            "has no `authorization_servers` member",
        ),
        (json!({"authorization_servers": []}), "has an empty"),
        (
            json!({"authorization_servers": "https://as"}),
            "that is not an array",
        ),
        (
            json!({"authorization_servers": null}),
            "that is not an array",
        ),
        (
            json!({"authorization_servers": ["https://as", 1]}),
            "not a non-empty string",
        ),
        (
            json!({"authorization_servers": [""]}),
            "not a non-empty string",
        ),
    ] {
        let (found, subjects) = run(SERVERS, &trace(none, PATH, 200, Some(document)));
        assert_eq!(subjects, 1);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains(said), "{found:?}");
    }
}

#[test]
fn only_a_document_returned_from_the_well_known_metadata_path_is_a_subject() {
    let bad = json!({"resource": "r"});
    let servers = |path: &str, status: u16, document: Option<Value>| {
        run(SERVERS, &trace("Bearer", path, status, document))
    };
    // The bare well-known path is one too.
    assert_eq!(
        servers(
            "/.well-known/oauth-protected-resource",
            200,
            Some(bad.clone())
        )
        .1,
        1
    );
    // Another well-known document, and a name that only starts the same.
    for path in [
        "/.well-known/oauth-authorization-server",
        "/.well-known/oauth-protected-resources",
        "/.well-known/oauth-protected-resource-x/mcp",
    ] {
        assert_eq!(servers(path, 200, Some(bad.clone())), (vec![], 0), "{path}");
    }
    // Not returned: another status, no document, or JSON that is no object.
    assert_eq!(servers(PATH, 404, Some(bad.clone())), (vec![], 0));
    assert_eq!(servers(PATH, 201, Some(bad)), (vec![], 0));
    assert_eq!(servers(PATH, 200, None), (vec![], 0));
    assert_eq!(servers(PATH, 200, Some(json!([]))), (vec![], 0));
}
