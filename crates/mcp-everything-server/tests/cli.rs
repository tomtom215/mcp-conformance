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
