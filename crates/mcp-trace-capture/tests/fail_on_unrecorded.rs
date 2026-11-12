// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `--fail-on-unrecorded`: a run that forwarded something its trace does not
//! hold exits 3 instead of 0, so a CI job that records a session and then
//! judges it cannot read a `pass` for a trace that left out the worst of it.
//! Without the flag the exit codes are what they always were.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-unrecorded-{name}-{}.jsonl",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// Runs `stdio -- server…` with `flags` recording to `trace`, `input` on stdin.
fn wrap(trace: &Path, flags: &[&str], server: &[&str], input: &[u8]) -> Output {
    let mut child = binary()
        .args(["-o", trace.to_str().unwrap()])
        .args(flags)
        .args(["stdio", "--"])
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

fn closing_event(trace: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(trace).unwrap();
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

const SHORT: &[u8] = b"{\"id\":1}\n";
/// Over the 20-byte message limit the tests set.
const LONG: &[u8] =
    b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"a-long-name\"}}\n";
const LIMIT: [&str; 2] = ["--max-message-bytes", "20"];

#[test]
fn a_session_that_left_nothing_out_exits_0_with_the_flag_and_says_so() {
    let trace = scratch("clean");
    let output = wrap(&trace, &["--fail-on-unrecorded"], &["cat"], SHORT);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        closing_event(&trace)["capture"],
        serde_json::json!({"unrecorded": {}})
    );
    std::fs::remove_file(&trace).ok();
}

#[test]
fn what_the_trace_leaves_out_changes_no_exit_code_by_default() {
    let trace = scratch("default");
    let output = wrap(&trace, &LIMIT, &["cat"], LONG);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(0), "{output:?}\n{stderr}");
    assert!(stderr.contains("over the size limit"), "{stderr}");
    assert_eq!(
        closing_event(&trace)["capture"],
        serde_json::json!({"unrecorded": {"oversized": 2}}),
        "both directions' copies of the line"
    );
    std::fs::remove_file(&trace).ok();
}

#[test]
fn what_the_trace_leaves_out_exits_3_with_the_flag() {
    let trace = scratch("flag");
    let mut flags = LIMIT.to_vec();
    flags.push("--fail-on-unrecorded");
    let output = wrap(&trace, &flags, &["cat"], LONG);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(3), "{output:?}\n{stderr}");
    assert!(
        stderr.contains("--fail-on-unrecorded") && stderr.contains("oversized"),
        "says why it exited 3: {stderr}"
    );
    assert_eq!(output.stdout, LONG, "the session itself was relayed whole");
    std::fs::remove_file(&trace).ok();
}

/// The flag turns a 0 into a 3, as a failed write does, and nothing else: a
/// server that failed keeps the code it failed with.
#[test]
fn the_flag_does_not_replace_a_servers_own_failure() {
    let trace = scratch("server-failed");
    let mut flags = LIMIT.to_vec();
    flags.push("--fail-on-unrecorded");
    let output = wrap(&trace, &flags, &["sh", "-c", "cat; exit 7"], LONG);
    assert_eq!(output.status.code(), Some(7), "{output:?}");
    std::fs::remove_file(&trace).ok();
}

/// A proxy under test: its address, and the lines it has said on stderr.
struct Proxy {
    child: Child,
    address: String,
}

fn start_proxy(trace: &Path, flags: &[&str]) -> Proxy {
    let mut child = binary()
        .args(["-o", trace.to_str().unwrap()])
        .args(flags)
        .args([
            "http",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
            "http://127.0.0.1:9",
        ])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
        {
            if let Some(rest) = line.split("listening on http://").nth(1) {
                let _ = tx.send(rest.split(',').next().unwrap().to_owned());
            }
        }
    });
    let address = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the proxy announced its address");
    Proxy { child, address }
}

impl Proxy {
    /// Sends a request the proxy refuses on its own (a `..` path segment), and
    /// stops it with SIGINT; returns its exit code.
    fn refuse_one_and_stop(mut self) -> Option<i32> {
        let mut stream = std::net::TcpStream::connect(&self.address).unwrap();
        stream
            .write_all(b"GET /mcp/../x HTTP/1.1\r\nhost: localhost\r\nconnection: close\r\n\r\n")
            .unwrap();
        let mut response = Vec::new();
        let _ = stream.read_to_end(&mut response);
        assert!(response.starts_with(b"HTTP/1.1 400"), "{response:?}");
        let status = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        self.child.wait().unwrap().code()
    }
}

/// A request the proxy refuses never reaches a server, so it is in no
/// session's trace; the run still did not record it.
#[test]
fn a_request_the_proxy_refused_exits_3_with_the_flag_and_0_without() {
    let trace = scratch("refused-flag");
    let proxy = start_proxy(&trace, &["--fail-on-unrecorded"]);
    assert_eq!(proxy.refuse_one_and_stop(), Some(3));
    std::fs::remove_file(&trace).ok();

    let trace = scratch("refused-default");
    let proxy = start_proxy(&trace, &[]);
    assert_eq!(proxy.refuse_one_and_stop(), Some(0));
    std::fs::remove_file(&trace).ok();
}
