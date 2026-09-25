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
