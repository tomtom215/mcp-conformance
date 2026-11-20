// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `check` at its edges: the paths the first tests reached only by reading —
//! a server that dies after the handshake, a revision named and not served,
//! a result asking for input, an annotation that binds another transport, an
//! answer over the limit, the private directory of a default recording. Each
//! is here because a mutation of the code it exercises survived without it.

use mcp_everything_server::server::ServedRevision;
use serde_json::Value;

use super::check::{SERVER, runtime, serve, stand_in, stdout};
use super::*;

#[test]
fn a_server_that_dies_mid_session_is_still_judged_on_what_was_recorded() {
    let flags = ["--spec-version", "2025-11-25"];
    let (output, summary, _) = stand_in("check-dies-later", "dies-later", &flags);
    // A session was held, so this is a verdict (the validator's code), not a
    // run that could not be made.
    assert_ne!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("the session ended early"),
        "{}",
        stderr(&output)
    );
    assert!(
        stdout(&output).contains("MCP trace validation"),
        "{}",
        stdout(&output)
    );
    assert_eq!(summary["revision"], "2025-11-25");
    let first = summary["unanswered"][0].as_str().unwrap();
    assert!(first.starts_with("tools/list"), "{summary}");
}

#[test]
fn naming_the_modern_revision_does_not_fall_back_to_the_handshake() {
    let flags = ["--spec-version", "2026-07-28"];
    let (output, summary, _) = stand_in("check-modern-only", "plain", &flags);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("--spec-version 2026-07-28 asks for that revision alone"),
        "{}",
        stderr(&output)
    );
    assert_eq!(summary["revision"], Value::Null);
    assert_eq!(summary["requests"], 1, "the probe and nothing after it");
}

#[test]
fn a_result_asking_for_client_input_is_noted() {
    let flags = ["--spec-version", "2025-11-25"];
    let (output, summary, _) = stand_in("check-wants-input", "wants-input", &flags);
    assert_eq!(
        summary["needed_input"],
        serde_json::json!(["tools/call look"]),
        "{summary}"
    );
    assert!(
        stderr(&output).contains("asked the client for input"),
        "{}",
        stderr(&output)
    );
    // And a result that asks for nothing is not.
    let (_, summary, _) = stand_in("check-wants-nothing", "plain", &flags);
    assert_eq!(summary["needed_input"], serde_json::json!([]));
}

#[test]
fn header_annotations_bind_only_the_transport_that_has_headers() {
    // An `x-mcp-header` no chain of `properties` reaches makes a tool invalid
    // for a client on Streamable HTTP at 2026-07-28. Over stdio a client may
    // ignore the annotation, and this one does: the tool is called.
    // At either revision: it is the transport, not the era, that has headers.
    for (name, mode, revision) in [
        ("check-annotated", "annotated", "2025-11-25"),
        ("check-modern-annotated", "modern-annotated", "2026-07-28"),
    ] {
        let (_, summary, calls) = stand_in(name, mode, &[]);
        assert_eq!(summary["revision"], revision, "{summary}");
        assert_eq!(summary["tools"]["exercised"], 1, "{summary}");
        assert_eq!(summary["skipped"], serde_json::json!([]));
        assert!(calls.contains(r#""name":"mirrors""#), "{calls}");
    }
}

#[test]
fn an_answer_larger_than_the_limit_is_left_and_the_session_goes_on() {
    // `initialize` and `ping` fit in 200 bytes; the tool listing does not.
    let flags = [
        "--spec-version",
        "2025-11-25",
        "--max-response-bytes",
        "200",
    ];
    let (output, summary, calls) = stand_in("check-too-large", "plain", &flags);
    let unanswered = summary["unanswered"][0].as_str().unwrap();
    assert!(
        unanswered.starts_with("tools/list") && unanswered.contains("--max-response-bytes"),
        "{summary}"
    );
    assert_eq!(summary["unanswered"].as_array().unwrap().len(), 1);
    assert_eq!(summary["tools"]["listed"], 0);
    assert_eq!(calls, "");
    assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
}

#[test]
fn an_http_answer_larger_than_the_limit_is_not_read() {
    let runtime = runtime();
    for revision in [ServedRevision::V2025_11_25, ServedRevision::V2026_07_28] {
        let address = serve(&runtime, revision);
        let output = binary()
            .args(["check", "--max-response-bytes", "60", "--url"])
            .arg(format!("http://{address}/mcp"))
            .output()
            .unwrap();
        // Nothing the server says fits, so no session is held.
        assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
        assert!(
            stderr(&output).contains("--max-response-bytes"),
            "{}",
            stderr(&output)
        );
    }
}

#[test]
fn the_default_recording_is_in_a_directory_only_its_owner_can_enter() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = scratch("check-private");
    let script = dir.join("server.sh");
    std::fs::write(&script, SERVER).unwrap();
    let output = binary()
        .args(["check", "--spec-version", "2025-11-25", "--timeout", "2"])
        .args(["--", "sh"])
        .arg(&script)
        .arg("plain")
        .arg(dir.join("noted"))
        .output()
        .unwrap();
    let account = stderr(&output);
    let trace = account
        .lines()
        .find_map(|line| line.trim().strip_prefix("trace "))
        .map_or_else(|| panic!("{account}"), str::trim);
    let trace = Path::new(trace);
    assert!(trace.is_file(), "{account}");
    let mode = std::fs::metadata(trace.parent().unwrap())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o700, "{mode:o}");
    std::fs::remove_dir_all(trace.parent().unwrap()).ok();
}

/// The `initialize` result the raw server gives.
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"raw","version":"1"}}}"#;

/// An HTTP server that answers by hand, so a test knows every byte: an
/// `initialize` gets `first` (a whole HTTP response), `server/discover` gets
/// `probe` when there is one, a `ping` an empty result, anything else `202`. It serves until the test ends, and reports
/// each request body it was sent.
fn raw_server(
    first: String,
    probe: Option<String>,
) -> (std::net::SocketAddr, std::sync::mpsc::Receiver<String>) {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (bodies, seen) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let body = loop {
                let Ok(read) = stream.read(&mut buffer) else {
                    break String::new();
                };
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request).into_owned();
                let Some((head, body)) = text.split_once("\r\n\r\n") else {
                    if read == 0 {
                        break String::new();
                    }
                    continue;
                };
                let expected = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())?
                    })
                    .unwrap_or(0);
                if body.len() >= expected || read == 0 {
                    break body.to_owned();
                }
            };
            let response = if body.contains("\"initialize\"") {
                first.clone()
            } else if let (true, Some(probe)) = (body.contains("server/discover"), &probe) {
                probe.clone()
            } else if body.contains("\"ping\"") {
                json_response(r#"{"jsonrpc":"2.0","id":2,"result":{}}"#)
            } else {
                "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
            };
            let _ = stream.write_all(response.as_bytes());
            let _ = bodies.send(body);
        }
    });
    (address, seen)
}

fn json_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// `check` against the raw server with `limit` bytes allowed per answer.
fn check_raw(address: std::net::SocketAddr, limit: usize, name: &str) -> (Output, Value) {
    let summary = scratch(name).join("s.json");
    let output = binary()
        .args(["check", "--spec-version", "2025-11-25", "--url"])
        .arg(format!("http://{address}/mcp"))
        .args(["--max-response-bytes", &limit.to_string(), "--summary"])
        .arg(&summary)
        .output()
        .unwrap();
    let summary = serde_json::from_str(&std::fs::read_to_string(&summary).unwrap()).unwrap();
    (output, summary)
}

#[test]
fn a_json_answer_of_exactly_the_limit_is_read_and_one_byte_more_is_not() {
    let (address, _seen) = raw_server(json_response(INITIALIZED), None);
    let (output, summary) = check_raw(address, INITIALIZED.len(), "check-raw-json-fits");
    assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
    assert_eq!(summary["unanswered"], serde_json::json!([]));
    let (output, summary) = check_raw(address, INITIALIZED.len() - 1, "check-raw-json-over");
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(summary["revision"], Value::Null);
    assert!(
        stderr(&output).contains("--max-response-bytes"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn an_event_stream_is_bounded_alike_and_a_request_on_it_is_refused_and_counted() {
    // The legacy era lets a server put a request of its own on the stream
    // that answers the client's; this one asks for roots before it answers.
    let asked = r#"{"jsonrpc":"2.0","id":"s-1","method":"roots/list"}"#;
    let events = format!("data: {asked}\n\ndata: {INITIALIZED}\n\n");
    let (address, seen) = raw_server(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{events}"
        ),
        None,
    );
    let (output, summary) = check_raw(address, events.len(), "check-raw-sse-fits");
    assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
    assert_eq!(summary["server_requests"], 1, "{summary}");
    let bodies: Vec<String> = seen.try_iter().collect();
    let refusal = bodies
        .iter()
        .find(|body| body.contains("\"s-1\""))
        .unwrap_or_else(|| panic!("{bodies:?}"));
    assert!(refusal.contains("-32601"), "{refusal}");
    let (output, summary) = check_raw(address, events.len() - 1, "check-raw-sse-over");
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert_eq!(summary["revision"], Value::Null);
}

#[test]
fn a_probe_whose_stream_ends_unanswered_falls_back_on_the_same_endpoint() {
    // An event stream that closes without an answer: over HTTP that is one
    // exchange lost, and the endpoint is asked again with `initialize` — not
    // "started again", which is what happens to a stdio server that exits.
    let silent = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n: nothing\n\n";
    let (address, _seen) = raw_server(
        json_response(&INITIALIZED.replace("\"id\":1", "\"id\":2")),
        Some(silent.to_owned()),
    );
    let summary = scratch("check-raw-probe").join("s.json");
    let output = binary()
        .args(["check", "--url"])
        .arg(format!("http://{address}/mcp"))
        .arg("--summary")
        .arg(&summary)
        .output()
        .unwrap();
    let summary: Value = serde_json::from_str(&std::fs::read_to_string(&summary).unwrap()).unwrap();
    assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
    let negotiation = summary["negotiation"].as_str().unwrap();
    assert!(
        negotiation.contains("ended without an answer") && !negotiation.contains("started again"),
        "{negotiation}"
    );
    assert_eq!(
        summary["requests"], 3,
        "the probe, initialize and ping: {summary}"
    );
}

#[test]
fn a_last_line_without_its_newline_is_still_an_answer_up_to_the_limit() {
    // The server writes its `initialize` answer without the newline and
    // exits. A line cut short by the limit also has no newline, and the two
    // must not be confused: this one is whole, and exactly as long as allowed.
    let answer = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"sh-server","version":"1"}}}"#;
    for (name, limit) in [
        ("check-unterminated-exact", answer.len().to_string()),
        ("check-unterminated-roomy", "100000".to_owned()),
    ] {
        let flags = [
            "--spec-version",
            "2025-11-25",
            "--max-response-bytes",
            &limit,
        ];
        let (output, summary, _) = stand_in(name, "unterminated", &flags);
        assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
        assert_eq!(summary["server"], "sh-server 1");
    }
    // One byte less and it is over the limit.
    let limit = (answer.len() - 1).to_string();
    let flags = [
        "--spec-version",
        "2025-11-25",
        "--max-response-bytes",
        &limit,
    ];
    let (output, summary, _) = stand_in("check-unterminated-over", "unterminated", &flags);
    assert_eq!(summary["revision"], Value::Null, "{}", stderr(&output));
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_server_that_outlives_its_stdin_is_stopped_before_check_returns() {
    // The stand-in sleeps for ten seconds once its stdin closes. `check`
    // gives a server the grace the SDKs' clients give, then stops it: it does
    // not return with the process still running.
    let flags = ["--spec-version", "2025-11-25"];
    let (output, summary, pid) = stand_in("check-lingers", "lingers", &flags);
    assert_eq!(summary["revision"], "2025-11-25", "{}", stderr(&output));
    // The stand-in's own notes follow its process id in the same file.
    let pid: String = pid.chars().take_while(char::is_ascii_digit).collect();
    assert!(!pid.is_empty(), "the stand-in wrote no process id");
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!alive.success(), "server {pid} is still running");
    // And the recording was closed, not abandoned: its last event is the
    // transport's end, written when the server was told to stop.
    let trace = std::fs::read_to_string(scratch_path("check-lingers").join("t.jsonl")).unwrap();
    let last = trace.lines().last().unwrap_or_default();
    assert!(last.contains(r#""kind":"lifecycle""#), "{last}");
}
