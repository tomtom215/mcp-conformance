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
    std::fs::remove_file(&trace).ok();
}
