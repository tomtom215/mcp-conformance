// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The binary as a client launches it: stdio relayed byte for byte through a real
//! child process, the server's exit code passed through, and the refusals.
//!
//! Unix-only where a child is needed: `cat` and `sh` are the portable stand-ins for
//! a server. The wrapper around the real everything server runs in
//! `cargo xtask conformance`.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-{name}-{}.jsonl",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

fn run_with_stdin(mut command: Command, input: &[u8]) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
#[test]
fn stdio_relays_bytes_unchanged_and_records_both_directions() {
    let trace = scratch("echo");
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n";
    let mut command = binary();
    command.args(["-o", trace.to_str().unwrap(), "stdio", "--", "cat"]);
    let output = run_with_stdin(command, input);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout, input,
        "the client sees exactly what the server wrote"
    );

    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    let events: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    // open, two requests, two echoes, close — and every line a valid trace event.
    assert_eq!(events.len(), 6, "{text}");
    assert_eq!(events[0]["event"], "transport-open");
    assert_eq!(events[5]["event"], "transport-close");
    let directions: Vec<&str> = events[1..5]
        .iter()
        .map(|event| event["direction"].as_str().unwrap())
        .collect();
    assert_eq!(
        directions
            .iter()
            .filter(|d| **d == "client-to-server")
            .count(),
        2
    );
    for event in &events {
        serde_json::from_value::<mcp_conformance_core::trace::TraceEvent>(event.clone()).unwrap();
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("recorded 6 event(s)"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn the_servers_exit_code_passes_through_and_its_failure_is_an_abort() {
    let trace = scratch("exit");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "stdio",
        "--",
        "sh",
        "-c",
        "exit 7",
    ]);
    let output = run_with_stdin(command, b"");
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(7), "{output:?}");
    assert!(text.contains(r#""event":"transport-abort""#), "{text}");
}

#[cfg(unix)]
#[test]
fn non_json_server_output_is_forwarded_and_reported_not_recorded() {
    let trace = scratch("notjson");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "stdio",
        "--",
        "sh",
        "-c",
        "echo 'log line on stdout'; echo '{\"jsonrpc\":\"2.0\",\"method\":\"x\"}'",
    ]);
    let output = run_with_stdin(command, b"");
    let text = std::fs::read_to_string(&trace).unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "log line on stdout\n{\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n"
    );
    assert!(!text.contains("log line"));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("1 server message(s) were not JSON"),
        "{stderr}"
    );
}

#[test]
fn an_existing_trace_is_not_overwritten_without_force() {
    let trace = scratch("exists");
    std::fs::write(&trace, "keep me\n").unwrap();
    let output = binary()
        .args(["-o", trace.to_str().unwrap(), "stdio", "--", "true"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(std::fs::read_to_string(&trace).unwrap(), "keep me\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--force"));
    std::fs::remove_file(&trace).ok();
}

#[test]
fn a_server_that_cannot_start_is_a_usage_error() {
    let trace = scratch("missing");
    let output = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "stdio",
            "--",
            "definitely-not-a-real-mcp-server-binary",
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot start"));
}

#[cfg(unix)]
#[test]
fn a_server_killed_by_a_signal_exits_128_plus_the_signal() {
    let trace = scratch("signal");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "stdio",
        "--",
        "sh",
        "-c",
        "kill -TERM $$",
    ]);
    let output = run_with_stdin(command, b"");
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(128 + 15), "{output:?}");
}

#[cfg(unix)]
#[test]
fn oversized_and_non_json_counts_are_reported_only_when_non_zero() {
    let trace = scratch("oversized");
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "--max-message-bytes",
        "16",
        "stdio",
        "--",
        "cat",
    ]);
    let output = run_with_stdin(
        command,
        b"{\"a\":\"far more than sixteen bytes\"}\n{\"b\":1}\n",
    );
    std::fs::remove_file(&trace).ok();
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The long line crossed in both directions; nothing was non-JSON.
    assert!(
        stderr.contains("1 client message(s) exceeded --max-message-bytes"),
        "{stderr}"
    );
    assert!(
        stderr.contains("1 server message(s) exceeded --max-message-bytes"),
        "{stderr}"
    );
    assert!(!stderr.contains("were not JSON"), "{stderr}");
}

#[cfg(target_os = "linux")]
#[test]
fn an_incomplete_trace_exits_3_unless_the_server_already_failed() {
    // /dev/full accepts the open and fails every write, as a full disk does.
    let mut command = binary();
    command.args(["--force", "-o", "/dev/full", "stdio", "--", "true"]);
    let output = run_with_stdin(command, b"");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("is incomplete"));

    let mut command = binary();
    command.args([
        "--force",
        "-o",
        "/dev/full",
        "stdio",
        "--",
        "sh",
        "-c",
        "exit 7",
    ]);
    let output = run_with_stdin(command, b"");
    assert_eq!(
        output.status.code(),
        Some(7),
        "the server's failure outranks the trace's"
    );
}

/// Starts the proxy with `args`, returns it and its listening address.
#[cfg(unix)]
fn start_proxy(
    trace: &std::path::Path,
    upstream: &str,
) -> (
    std::process::Child,
    String,
    std::sync::mpsc::Receiver<String>,
) {
    use std::io::BufRead as _;
    let mut child = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "http",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
            upstream,
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (address_tx, address_rx) = std::sync::mpsc::channel();
    let (lines_tx, lines_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(rest) = line.split("listening on http://").nth(1) {
                let _ = address_tx.send(rest.split(',').next().unwrap().to_owned());
            }
            let _ = lines_tx.send(line);
        }
    });
    let address = address_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("the proxy announces its address");
    (child, address, lines_rx)
}

#[cfg(unix)]
fn interrupt(child: &mut std::process::Child) -> std::process::ExitStatus {
    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    child.wait().unwrap()
}

#[cfg(unix)]
#[test]
fn http_mode_stops_cleanly_and_reports_unreachable_upstreams() {
    use std::io::{Read as _, Write as _};
    // Nothing listens on this port.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let upstream = format!("http://127.0.0.1:{port}");

    // A run with no traffic: a clean stop, exit 0, and no failure summary.
    let trace = scratch("http-idle");
    let (mut child, _, lines) = start_proxy(&trace, &upstream);
    assert_eq!(interrupt(&mut child).code(), Some(0));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        !stderr.iter().any(|line| line.contains("could not reach")),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();

    // A request the upstream cannot answer: 502 from the proxy, and the summary.
    let trace = scratch("http-dead");
    let (mut child, address, lines) = start_proxy(&trace, &upstream);
    let mut stream = std::net::TcpStream::connect(&address).unwrap();
    stream
        .write_all(b"POST /mcp HTTP/1.1\r\nhost: localhost\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 502"), "{response}");
    assert_eq!(interrupt(&mut child).code(), Some(0));
    std::thread::sleep(std::time::Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        stderr
            .iter()
            .any(|line| line.contains("1 request(s) could not reach the upstream")),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();
}

#[test]
fn http_mode_rejects_an_upstream_that_is_not_an_absolute_http_url() {
    let trace = scratch("http-bad");
    let output = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "http",
            "--upstream",
            "ftp://example.com",
        ])
        .output()
        .unwrap();
    std::fs::remove_file(&trace).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not an absolute http"));
}
