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
