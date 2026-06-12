// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Integration tests for the `mcp-everything-server` binary.
//!
//! The stdio transport is a documented interface: a subprocess speaking
//! newline-delimited JSON-RPC on stdin/stdout, diagnostics never on stdout.
//! That contract is pinned by executing the real binary and performing a real
//! `initialize` handshake over its pipes — which is also what keeps the
//! binary's entry points inside the mutation gate.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};

#[test]
fn stdio_serves_a_real_initialize_handshake() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .arg("--transport")
        .arg("stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("binary spawns");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-11-25","capabilities":{{}},"clientInfo":{{"name":"cli-test","version":"0.0.0"}}}}}}"#
    )
    .expect("write initialize");

    let mut line = String::new();
    reader
        .read_line(&mut line)
        .expect("read initialize response");
    let response: serde_json::Value =
        serde_json::from_str(&line).expect("response is one JSON line");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        response["result"]["serverInfo"]["name"],
        "mcp-everything-server"
    );

    // EOF on stdin is the stdio shutdown signal; the server must exit cleanly.
    drop(stdin);
    let status = child.wait().expect("child exits");
    assert!(status.success(), "clean shutdown after stdin EOF: {status}");
}

#[test]
fn help_exits_zero_and_documents_the_transport_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .arg("--help")
        .output()
        .expect("binary runs");
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--transport"), "help mentions transport");
}

#[test]
fn unknown_flags_exit_with_the_clap_usage_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .arg("--no-such-flag")
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2), "clap usage-error convention");
}

#[test]
fn http_transport_binds_and_enforces_the_403_policy() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .args(["--transport", "http", "--bind", "127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");

    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).expect("readiness line");
    let addr = line
        .trim()
        .strip_prefix("listening on ")
        .unwrap_or_else(|| panic!("unexpected readiness line: {line:?}"))
        .to_owned();

    // Loopback socket to our own subprocess: the binary's HTTP contract is
    // pinned the same way the stdio contract is — against the real process.
    let mut stream = std::net::TcpStream::connect(&addr).expect("connect");
    let request = "POST /mcp HTTP/1.1\r\nHost: evil.example\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
    std::io::Write::write_all(&mut stream, request.as_bytes()).expect("send");
    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response).expect("read");
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "rebinding Host must 403: {response:?}"
    );

    child.kill().expect("stop server");
    let _ = child.wait();
}

/// TRAN-008: "servers SHOULD bind only to localhost" — the binary's default,
/// with no `--bind` given, must be a loopback listener. The registry's
/// TRAN-008 exclusion names this test as the enforcement; every other test
/// passes `--bind` explicitly and would never notice a widened default.
#[test]
fn default_bind_is_loopback() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .args(["--transport", "http"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");

    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).expect("readiness line");
    child.kill().expect("stop server");
    let _ = child.wait();

    let addr = line
        .trim()
        .strip_prefix("listening on ")
        .unwrap_or_else(|| panic!("unexpected readiness line: {line:?}"));
    assert!(
        addr.starts_with("127.0.0.1:"),
        "default bind must be loopback, got {addr:?}"
    );
}

/// `--tap-dir` is an HTTP-transport feature; combining it with stdio must
/// fail fast (exit 2, the invocation-error convention) before any serving.
#[cfg(feature = "tap")]
#[test]
fn tap_dir_with_stdio_is_rejected_before_serving() {
    let dir = std::env::temp_dir().join(format!("cli-tap-stdio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .args(["--transport", "stdio", "--tap-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .output()
        .expect("binary runs");
    assert_eq!(
        output.status.code(),
        Some(2),
        "invocation error: {output:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--tap-dir requires --transport http"),
        "the rejection names the fix: {stderr}"
    );
    assert!(!dir.exists(), "no tap directory for a rejected invocation");
}

/// The full binary contract of `--tap-dir`: HTTP serving starts (readiness
/// line), a real initialize round-trips on a loopback socket, and the session
/// trace appears on disk.
#[cfg(feature = "tap")]
#[test]
fn tap_dir_with_http_serves_and_records_the_session() {
    let dir = std::env::temp_dir().join(format!("cli-tap-http-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .args(["--transport", "http", "--bind", "127.0.0.1:0", "--tap-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");

    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).expect("readiness line");
    let addr = line
        .trim()
        .strip_prefix("listening on ")
        .unwrap_or_else(|| panic!("unexpected readiness line: {line:?}"))
        .to_owned();

    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"cli-tap","version":"0.0.0"}}}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = std::net::TcpStream::connect(&addr).expect("connect");
    std::io::Write::write_all(&mut stream, request.as_bytes()).expect("send");
    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response).expect("read");
    assert!(
        response.starts_with("HTTP/1.1 200"),
        "initialize succeeds through the tapped app: {response:?}"
    );

    // The tap's writer is asynchronous; poll (bounded) for the trace.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let recorded = loop {
        let found = std::fs::read_dir(&dir).ok().and_then(|entries| {
            entries.flatten().find_map(|entry| {
                let text = std::fs::read_to_string(entry.path()).ok()?;
                text.contains(r#""method":"initialize""#).then_some(text)
            })
        });
        if let Some(text) = found {
            break text;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "no initialize trace appeared in {} within 10s",
            dir.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    assert!(
        recorded.contains(r#""seq":0"#),
        "the trace starts at seq 0: {recorded}"
    );

    child.kill().expect("stop server");
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&dir);
}

/// One raw-socket POST to the spawned server, optionally session-scoped;
/// returns the full HTTP response text.
#[cfg(feature = "tap")]
fn raw_post(addr: &str, body: &str, session: Option<&str>) -> String {
    let session_header = session
        .map(|id| format!("Mcp-Session-Id: {id}\r\nMCP-Protocol-Version: 2025-11-25\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{session_header}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    std::io::Write::write_all(&mut stream, request.as_bytes()).expect("send");
    let mut response = String::new();
    std::io::Read::read_to_string(&mut stream, &mut response).expect("read");
    response
}

/// Asserts one file is a valid trace prefix: every complete line parses,
/// `seq` is contiguous from 0, and at most the final line may be torn.
#[cfg(feature = "tap")]
fn assert_valid_trace_prefix(path: &std::path::Path) {
    let text = std::fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(!lines.is_empty(), "started file cannot be empty");
    let mut expected_seq = 0u64;
    for (index, raw) in lines.iter().enumerate() {
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(event) => {
                assert_eq!(
                    event["seq"].as_u64(),
                    Some(expected_seq),
                    "persisted prefix has contiguous seq: {}",
                    path.display()
                );
                expected_seq += 1;
            }
            Err(error) => {
                assert_eq!(
                    index,
                    lines.len() - 1,
                    "only the final line may be torn by the kill: line {index}: {error}"
                );
            }
        }
    }
}

/// Spawns the tapped HTTP server, completes the initialize handshake over a
/// raw socket, and waits until the tap's writer has demonstrably persisted
/// its first bytes. Returns the child, address, and session id.
#[cfg(feature = "tap")]
fn spawn_initialized_tapped_server(dir: &std::path::Path) -> (std::process::Child, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .args(["--transport", "http", "--bind", "127.0.0.1:0", "--tap-dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).expect("readiness line");
    let addr = line
        .trim()
        .strip_prefix("listening on ")
        .unwrap_or_else(|| panic!("unexpected readiness line: {line:?}"))
        .to_owned();

    let init = raw_post(
        &addr,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"kill-test","version":"0.0.0"}}}"#,
        None,
    );
    assert!(init.starts_with("HTTP/1.1 200"), "{init:?}");
    let session_id = init
        .lines()
        .find_map(|l| {
            l.to_lowercase()
                .strip_prefix("mcp-session-id:")
                .map(str::trim)
                .map(ToOwned::to_owned)
        })
        .expect("session id header");
    let _ = raw_post(
        &addr,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        Some(&session_id),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let started = std::fs::read_dir(dir).ok().is_some_and(|entries| {
            entries
                .flatten()
                .any(|e| std::fs::metadata(e.path()).is_ok_and(|m| m.len() > 0))
        });
        if started {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "writer never persisted anything before the kill"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    (child, addr, session_id)
}

/// L3 durability proof for the tap's write-behind design, against the real
/// process: after a SIGKILL with traffic still in flight, whatever the
/// writer persisted is a valid trace prefix — every complete line parses,
/// `seq` is contiguous from 0, and at most the final line may be torn
/// (`src/tap.rs` §Write-behind documents exactly these semantics; events
/// still queued at kill time die with the process, deliberately).
#[cfg(feature = "tap")]
#[test]
fn tap_survives_sigkill_with_a_parseable_prefix() {
    let dir = std::env::temp_dir().join(format!("cli-tap-kill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut child, addr, session_id) = spawn_initialized_tapped_server(&dir);

    // A burst of echo calls with the kill racing the writer.
    for index in 0..20 {
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"echo","arguments":{{"message":"burst-{index:02}"}}}}}}"#,
            index + 2
        );
        let _ = raw_post(&addr, &body, Some(&session_id));
    }
    child.kill().expect("SIGKILL");
    let _ = child.wait();

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("tap dir")
        .flatten()
        .collect();
    assert_eq!(entries.len(), 1, "exactly the one session's trace exists");
    assert_valid_trace_prefix(&entries[0].path());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The tap's "recorded an exchange without its request message" stderr note
/// fires exactly when it should: once for one session-scoped non-JSON body,
/// never for clean traffic. The note is the only observable of its guard
/// condition, so this is the test that kills mutations of that guard
/// (`&&`→`||` would note every clean exchange; dropping the `!` would note
/// none of the bad ones).
#[cfg(feature = "tap")]
#[test]
fn tap_notes_unrecordable_request_bodies_exactly_once() {
    let dir = std::env::temp_dir().join(format!("cli-tap-note-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-everything-server"))
        .args(["--transport", "http", "--bind", "127.0.0.1:0", "--tap-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    let stderr = child.stderr.take().unwrap();
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    reader.read_line(&mut line).expect("readiness line");
    let addr = line
        .trim()
        .strip_prefix("listening on ")
        .unwrap_or_else(|| panic!("unexpected readiness line: {line:?}"))
        .to_owned();

    let init = raw_post(
        &addr,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"note-test","version":"0.0.0"}}}"#,
        None,
    );
    assert!(init.starts_with("HTTP/1.1 200"), "{init:?}");
    let session_id = init
        .lines()
        .find_map(|l| {
            l.to_lowercase()
                .strip_prefix("mcp-session-id:")
                .map(str::trim)
                .map(ToOwned::to_owned)
        })
        .expect("session id header");
    let _ = raw_post(
        &addr,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        Some(&session_id),
    );
    let _ = raw_post(
        &addr,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"message":"clean"}}}"#,
        Some(&session_id),
    );
    // One session-scoped POST whose body is not JSON: the only exchange that
    // may produce the note.
    let _ = raw_post(&addr, "definitely not json", Some(&session_id));

    // Collect everything the server said before dying; the note (when the
    // guard is correct) is already flushed before the bad POST's response.
    child.kill().expect("stop server");
    let _ = child.wait();
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut reader, &mut rest).expect("drain stderr");
    let notes = rest
        .lines()
        .filter(|l| l.contains("without its request message"))
        .count();
    assert_eq!(
        notes, 1,
        "exactly the one bad body is noted; clean traffic is not:\n{rest}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
