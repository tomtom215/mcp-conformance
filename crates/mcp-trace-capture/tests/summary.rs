// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The stdio wrapper's exit summary: what it says the trace holds, and what it
//! says is missing from it or recorded only as a line.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-summary-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// Runs `stdio -- server…` recording to `trace` with `input` on stdin.
fn wrap(trace: &Path, server: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
        .args(["-o", trace.to_str().unwrap(), "stdio", "--"])
        .args(server)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let input = input.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap().unwrap();
    output
}

/// A message the trace cannot hold as sent is named in the summary, for each
/// side that sent one — and not mentioned when there was none.
#[test]
fn json_the_trace_cannot_hold_is_reported_per_side() {
    let trace = scratch("altered.jsonl");
    let output = wrap(
        &trace,
        &["cat"],
        b"{\"jsonrpc\":\"2.0\",\"id\":12345678901234567890123,\"method\":\"ping\"}\n",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    for side in ["client", "server"] {
        assert!(
            stderr.contains(&format!(
                "1 {side} line(s) were JSON the trace cannot hold as sent"
            )),
            "{stderr}"
        );
    }
    std::fs::remove_file(&trace).ok();

    let output = wrap(
        &trace,
        &["cat"],
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("cannot hold"), "{stderr}");
    assert!(
        !stderr.contains("came after the trace was closed"),
        "nothing came after the close: {stderr}"
    );
    std::fs::remove_file(&trace).ok();
}

/// A server that exits before any message is exchanged leaves a trace of two
/// transport events and no session: the summary says so, with the server's
/// status, instead of suggesting a validation that can only refuse it.
#[test]
fn a_server_that_exits_before_any_message_is_not_offered_for_validation() {
    let trace = scratch("silent.jsonl");
    for (script, code, status) in [
        ("exit 0", 0, "exit status: 0"),
        ("exit 7", 7, "exit status: 7"),
    ] {
        let output = wrap(&trace, &["sh", "-c", script], b"");
        assert_eq!(output.status.code(), Some(code), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!(
                "the server exited ({status}) before any message was exchanged"
            )),
            "{stderr}"
        );
        assert!(stderr.contains("2 transport event(s)"), "{stderr}");
        assert!(!stderr.contains("validate with"), "{stderr}");
        std::fs::remove_file(&trace).ok();
    }
}

/// The suggested command is one a shell runs as shown: a path with a space in
/// it is quoted.
#[test]
fn the_suggested_validation_command_quotes_the_path() {
    let dir = scratch("dir with space");
    std::fs::create_dir_all(&dir).unwrap();
    let trace = dir.join("t.jsonl");
    let output = wrap(
        &trace,
        &["cat"],
        b"{\"jsonrpc\":\"2.0\",\"method\":\"x\"}\n",
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "validate with `mcp-trace-validator validate '{}'`",
            trace.display()
        )),
        "{stderr}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// A client that goes away is said once: the wrapper's write to it fails, and
/// the server's output is neither relayed nor recorded from then on. Writing to
/// a server that has exited — a client's normal race with it — is not.
#[test]
fn a_client_that_went_away_is_reported_once() {
    let trace = scratch("gone.jsonl");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
        .args(["-o", trace.to_str().unwrap(), "stdio", "--", "sh", "-c"])
        .arg("sleep 0.3; echo '{}'; sleep 0.3; echo '{}'")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // The client's read end is gone before the server writes.
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.matches("cannot write to the client").count(),
        1,
        "{stderr}"
    );
    std::fs::remove_file(&trace).ok();

    let output = wrap(&trace, &["true"], &b"{}\n".repeat(20_000));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("cannot write"), "{stderr}");
    std::fs::remove_file(&trace).ok();
}
