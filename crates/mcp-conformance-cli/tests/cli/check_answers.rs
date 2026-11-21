// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check` against HTTP servers that answer by hand, for the answers a
//! conforming server gives rarely or never: a listing that already names the
//! probe's scheme, a refusal of a missing credential as a JSON-RPC error, a
//! gateway's `502`, a stream closed again each time the client comes back.
//! Each is here because a mutation of the code it exercises survived without
//! it.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::io::Write as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};

use super::check_auth::{GOOD, check, probes, read_request, response};
use super::*;

/// An HTTP server whose every response is `answer(head, body)` of the
/// request, for as long as the test runs.
fn answering(answer: impl Fn(&str, &str) -> String + Send + 'static) -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let (head, body) = read_request(&mut stream);
            let _ = stream.write_all(answer(&head, &body).as_bytes());
        }
    });
    address
}

/// What a legacy server answers the JSON-RPC message `body` with: the result
/// `results` has for its method, or "Method not found"; `202` to anything
/// that is not a request.
fn rpc(body: &str, results: impl Fn(&str) -> Option<Value>) -> String {
    let message: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let (Some(method), Some(id)) = (
        message.get("method").and_then(Value::as_str),
        message.get("id"),
    ) else {
        return response("202 Accepted", None, "");
    };
    let answer = results(method).map_or_else(
        || {
            json!({"jsonrpc": "2.0", "id": id,
                "error": {"code": -32601, "message": "Method not found"}})
        },
        |result| json!({"jsonrpc": "2.0", "id": id, "result": result}),
    );
    response("200 OK", None, &answer.to_string())
}

/// The `initialize` result of a legacy server declaring `capabilities`.
fn initialized(capabilities: &Value) -> Value {
    json!({"protocolVersion": "2025-11-25", "capabilities": capabilities,
        "serverInfo": {"name": "by-hand", "version": "1"}})
}

/// A server with resources: one listed at `uri`, one template at `template`.
fn with_resources(uri: &'static str, template: &'static str) -> SocketAddr {
    answering(move |_, body| {
        rpc(body, |method| match method {
            "initialize" => Some(initialized(&json!({"resources": {}}))),
            "ping" => Some(json!({})),
            "resources/list" => Some(json!({"resources": [{"uri": uri, "name": "one"}]})),
            "resources/templates/list" => Some(json!({"resourceTemplates": [
                {"uriTemplate": template, "name": "some"}]})),
            "resources/read" => Some(json!({"contents": [{"uri": uri, "text": "here"}]})),
            _ => None,
        })
    })
}

const RESOURCE_PROBE: &str = "resources/read for a resource the server does not have";

#[test]
fn a_server_that_names_the_probes_scheme_is_not_asked_for_a_resource_under_it() {
    // The probe asks for a resource the server cannot have, which is only
    // known while the server names its scheme nowhere: not on a resource, and
    // not on a template. Either alone is enough to leave the probe out.
    for (name, uri, template, sent) in [
        (
            "check-probe-scheme-free",
            "file:///one",
            "file:///{n}",
            true,
        ),
        (
            "check-probe-scheme-resource",
            "mcp-conformance-probe://listed/one",
            "file:///{n}",
            false,
        ),
        (
            "check-probe-scheme-template",
            "file:///one",
            "mcp-conformance-probe://listed/{n}",
            false,
        ),
    ] {
        let address = with_resources(uri, template);
        let flags = ["--spec-version", "2025-11-25", "--timeout", "2"];
        let (output, summary, recorded) = check(address, name, &flags);
        assert_eq!(output.status.code(), Some(0), "{}", stderr(&output));
        // The listed resource was read either way, whatever its scheme.
        assert_eq!(summary["resources"]["exercised"], 1, "{summary}");
        let sent_probes = probes(&summary);
        let probed = sent_probes
            .iter()
            .any(|probe| probe.starts_with(RESOURCE_PROBE));
        assert_eq!(probed, sent, "{name}: {sent_probes:?}");
        assert_eq!(recorded.contains("no-such-resource"), sent, "{name}");
    }
}

#[test]
fn a_missing_credential_refused_with_a_json_rpc_error_is_not_called_served() {
    // This server answers a request without the credential under HTTP 200,
    // with a JSON-RPC error: a refusal, and no status to hold it to.
    let address = answering(|head, body| {
        let credential = head.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value.trim() == GOOD
            })
        });
        rpc(body, |method| match method {
            "initialize" if credential => Some(initialized(&json!({}))),
            "ping" if credential => Some(json!({})),
            _ => None,
        })
    });
    let header = format!("Authorization: {GOOD}");
    let flags = ["--timeout", "2", "--header", &header];
    let (output, summary, _) = check(address, "check-auth-rpc-error", &flags);
    assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
    // The last probe is the knock, and it was not followed by the second:
    // that one is for a server that answered `401`.
    assert_eq!(
        probes(&summary).last().map(String::as_str),
        Some("a request with no credential — refused with an error"),
        "{summary}"
    );
    // `server/discover`, `initialize`, `ping`, the completion probe, the knock.
    assert_eq!(summary["requests"], 5, "{summary}");
}

/// A whole HTTP response that is no JSON-RPC message: `status` and `text`.
fn plain(status: &str, text: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{text}",
        text.len()
    )
}

#[test]
fn a_status_is_the_servers_unless_it_is_the_proxys_own_502() {
    // The recording proxy answers `502` with a body that opens with its own
    // name when nothing is at the URL, and `check` then says the server
    // cannot be reached. A server that is there is held to what it answered:
    // a `502` of its own (a gateway's, say), or the proxy's words under
    // another status.
    const PROXYS_WORDS: &str =
        "mcp-trace-capture: cannot reach the upstream http://127.0.0.1:1/mcp: refused";
    for (name, status, text) in [
        (
            "check-own-502",
            "502 Bad Gateway",
            "the gateway in front of this server could not reach it, and says so at length",
        ),
        (
            "check-borrowed-words",
            "500 Internal Server Error",
            PROXYS_WORDS,
        ),
    ] {
        let address = answering(move |_, _| plain(status, text));
        let flags = ["--spec-version", "2025-11-25", "--timeout", "2"];
        let (output, summary, _) = check(address, name, &flags);
        let said = stderr(&output);
        assert_eq!(output.status.code(), Some(2), "{said}");
        assert_eq!(summary["revision"], Value::Null);
        let code = &status[..3];
        assert!(
            said.contains(&format!("the server answered HTTP {code}")),
            "{name}: {said}"
        );
        assert!(!said.contains("cannot reach the server"), "{name}: {said}");
    }
    // And the proxy's own, for a URL nothing answers at.
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = free.local_addr().unwrap();
    drop(free);
    let flags = ["--spec-version", "2025-11-25", "--timeout", "2"];
    let (output, _, _) = check(address, "check-nobody-home", &flags);
    let said = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(said.contains("cannot reach the server at"), "{said}");
    assert!(!said.contains("the server answered HTTP"), "{said}");
}

#[test]
fn a_stream_closed_each_time_the_client_returns_is_resumed_three_times_and_no_more() {
    // Every stream this server opens carries one event with an id and no
    // answer, and closes; a client may come back for the rest, and this one
    // does — a bounded number of times, not for as long as it is let in.
    let resumed = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&resumed);
    let address = answering(move |head, _| {
        if head.starts_with("GET ") {
            counted.fetch_add(1, Ordering::SeqCst);
        }
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
         id: e-1\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\
         \"params\":{\"progressToken\":1,\"progress\":1}}\n\n"
            .to_owned()
    });
    let flags = ["--spec-version", "2025-11-25", "--timeout", "2"];
    let (output, summary, _) = check(address, "check-resumed-forever", &flags);
    let said = stderr(&output);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert_eq!(summary["revision"], Value::Null);
    // It gave up on its own count, well inside the time an answer is allowed.
    assert!(
        said.contains("the server ended the session without answering"),
        "{said}"
    );
    assert!(!said.contains("--timeout"), "{said}");
    assert_eq!(resumed.load(Ordering::SeqCst), 3, "{said}");
}
