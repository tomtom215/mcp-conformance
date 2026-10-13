// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The bridge end to end over loopback, in front of a stand-in stdio server
//! written in `sh` (Unix only, as the recorder's own process tests are).

use super::*;

/// A stdio "server" that answers `initialize`, and answers `tools/call` with a
/// progress notification and a sampling request, then — once the client has
/// answered the sampling request — the tool's result.
const SERVER: &str = r#"while IFS= read -r line; do
  case "$line" in
    *'"initialize"'*)
      printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"sh","version":"0"}}}' ;;
    *'"tools/call"'*)
      printf '%s\n' 'log: not json'
      printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":1,"progress":1}}'
      printf '%s\n' '{"jsonrpc":"2.0","id":"s1","method":"sampling/createMessage","params":{}}'
      IFS= read -r answer
      printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"content":[]}}' ;;
  esac
done"#;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcp-conformance-bridge-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `sh -c '<SERVER>'` as a whitespace-split command: the script goes in a file.
fn command(dir: &Path) -> String {
    let script = dir.join("server.sh");
    std::fs::write(&script, SERVER).unwrap();
    format!("sh {}", script.display())
}

async fn bounded(body: impl std::future::Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(30), body)
        .await
        .expect("the test finished within 30 s");
}

// One session's whole life, in order; splitting it would hide the sequence.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn a_session_runs_end_to_end_and_is_recorded_as_stdio() {
    bounded(async {
        let dir = scratch("session");
        let traces = dir.join("traces");
        std::fs::create_dir_all(&traces).unwrap();
        let bridge = start(&command(&dir), &traces).await.unwrap();
        let url = format!("http://{}/mcp", bridge.address);
        let client = reqwest::Client::new();

        let initialized = client
            .post(&url)
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(initialized.status(), 200);
        assert_eq!(
            initialized.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let session = initialized.headers()[SESSION_ID]
            .to_str()
            .unwrap()
            .to_owned();
        let body = initialized.text().await.unwrap();
        assert!(
            body.starts_with("event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\""),
            "{body}"
        );

        let notified = client
            .post(&url)
            .header(SESSION_ID, &session)
            // A body over several lines reaches the server as one.
            .body("{\"jsonrpc\":\"2.0\",\r\n \"method\":\"notifications/initialized\"}")
            .send()
            .await
            .unwrap();
        assert_eq!(notified.status(), 202);

        let mut called = client
            .post(&url)
            .header(SESSION_ID, &session)
            .body(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"t"}}"#)
            .send()
            .await
            .unwrap();
        let mut seen = String::new();
        while !seen.contains("sampling/createMessage") {
            let chunk = called
                .chunk()
                .await
                .unwrap()
                .expect("the stream stays open");
            seen.push_str(&String::from_utf8_lossy(&chunk));
        }
        assert!(seen.contains("notifications/progress"), "{seen}");
        assert!(!seen.contains("not json"), "{seen}");
        let answered = client
            .post(&url)
            .header(SESSION_ID, &session)
            .body(r#"{"jsonrpc":"2.0","id":"s1","result":{"role":"assistant"}}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(answered.status(), 202);
        let rest = called.text().await.unwrap();
        assert!(
            rest.contains(r#""id":2,"result""#),
            "the stream ends with the result: {rest}"
        );

        let deleted = client
            .delete(&url)
            .header(SESSION_ID, &session)
            .send()
            .await
            .unwrap();
        assert_eq!(deleted.status(), 200);
        let gone = client
            .post(&url)
            .header(SESSION_ID, &session)
            .body(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(gone.status(), 404);
        bridge.stop().await;

        let trace = std::fs::read_to_string(traces.join("001.jsonl")).unwrap();
        let events: Vec<serde_json::Value> = trace
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(
            events.iter().all(|event| event["transport"] == "stdio"),
            "{trace}"
        );
        let messages: Vec<&serde_json::Value> = events
            .iter()
            .filter(|event| event["kind"] == "message")
            .map(|event| &event["payload"])
            .collect();
        assert_eq!(messages[0]["method"], "initialize");
        assert_eq!(
            messages[2]["method"], "notifications/initialized",
            "{trace}"
        );
        assert!(
            messages.iter().any(|message| *message == "log: not json"),
            "a non-JSON line is in the trace: {trace}"
        );
        assert_eq!(messages.last().unwrap()["id"], 2);
        std::fs::remove_dir_all(&dir).ok();
    })
    .await;
}

// Every refusal the bridge makes, one request each; a list reads best whole.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn what_is_not_the_suite_talking_to_its_session_is_refused() {
    bounded(async {
        let dir = scratch("refused");
        let bridge = start(&command(&dir), &dir).await.unwrap();
        let url = format!("http://{}/mcp", bridge.address);
        let client = reqwest::Client::new();
        let status = |request: reqwest::RequestBuilder| async move {
            request.send().await.unwrap().status().as_u16()
        };
        let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
        assert_eq!(
            status(client.post(&url).body(ping)).await,
            400,
            "no session"
        );
        assert_eq!(
            status(client.post(&url).header(SESSION_ID, "made-up").body(ping)).await,
            404
        );
        assert_eq!(status(client.get(&url)).await, 400);
        assert_eq!(
            status(client.delete(&url).header(SESSION_ID, "x")).await,
            404
        );
        assert_eq!(status(client.put(&url).body(ping)).await, 405);
        let bad = client.post(&url).body("{oops").send().await.unwrap();
        assert_eq!(bad.status(), 400);
        assert!(bad.text().await.unwrap().contains("-32700"));
        assert_eq!(
            status(
                client
                    .post(&url)
                    .header(header::HOST, "evil.example")
                    .body(ping)
            )
            .await,
            403,
            "a rebound name"
        );
        assert_eq!(
            status(
                client
                    .post(&url)
                    .header(header::ORIGIN, "http://evil.example")
                    .body(ping)
            )
            .await,
            403,
            "a cross-site page"
        );
        assert_eq!(
            status(
                client
                    .post(&url)
                    .header(header::ORIGIN, "http://localhost.evil")
                    .body(ping)
            )
            .await,
            403,
            "a name that only begins like loopback"
        );
        assert_eq!(
            status(
                client
                    .post(&url)
                    .header(header::ORIGIN, "http://localhost:1")
                    .body(ping)
            )
            .await,
            400,
            "a loopback origin is let through to the session check"
        );
        bridge.stop().await;
        assert!(
            std::fs::read_dir(&dir).unwrap().all(|entry| {
                !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".jsonl")
            }),
            "no session began, so no trace"
        );
        std::fs::remove_dir_all(&dir).ok();
    })
    .await;
}

#[tokio::test]
async fn a_server_that_cannot_start_fails_the_start() {
    let dir = scratch("missing");
    let error = start("/nonexistent/server --flag", &dir).await.unwrap_err();
    assert!(error.contains("cannot start the server"), "{error}");
    assert_eq!(start("   ", &dir).await.unwrap_err(), "--command is empty");
    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn a_server_ignoring_end_of_input_is_stopped() {
    bounded(async {
        let dir = scratch("stubborn");
        let script = dir.join("stubborn.sh");
        // Answers initialize, then ignores its closed stdin.
        std::fs::write(
            &script,
            "IFS= read -r line; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'; exec sleep 60",
        )
        .unwrap();
        let bridge = start(&format!("sh {}", script.display()), &dir).await.unwrap();
        let url = format!("http://{}/mcp", bridge.address);
        let response = reqwest::Client::new()
            .post(&url)
            .body(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .send()
            .await
            .unwrap();
        assert!(response.text().await.unwrap().contains("\"result\""));
        let started = std::time::Instant::now();
        bridge.stop().await;
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "stopped within the grace periods, not after sleep 60"
        );
        let trace = std::fs::read_to_string(dir.join("001.jsonl")).unwrap();
        assert!(trace.lines().count() >= 3, "{trace}");
        std::fs::remove_dir_all(&dir).ok();
    })
    .await;
}

#[test]
fn a_body_becomes_one_line_with_nothing_else_changed() {
    assert_eq!(
        line(&Bytes::from_static(b"{\"a\":\r\n1}")),
        b"{\"a\":  1}\n".to_vec()
    );
    assert_eq!(line(&Bytes::new()), b"\n".to_vec());
    assert!(is_initialize(
        &serde_json::json!({"id":1,"method":"initialize"})
    ));
    assert!(!is_initialize(&serde_json::json!({"method":"initialize"})));
    assert!(!is_initialize(&serde_json::json!({"id":1,"method":"ping"})));
}
