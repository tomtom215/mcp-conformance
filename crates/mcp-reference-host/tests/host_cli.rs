// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The host binary's observable contract, against the real executable: exit
//! codes per stop reason, the run record on stderr, trace recording via
//! `--trace-dir`, and the deadline watchdog. These are the only tests that
//! reach `main.rs`'s dispatch/exit logic — the diff-scoped mutation gate
//! demands them (its first run on this slice left `agent_run`'s exit
//! calculation and `render` unobserved).

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

use mcp_everything_server::http::router;
use mcp_everything_server::policy::HttpSecurityPolicy;

/// Serves the everything-server app on an OS-assigned loopback port from a
/// background thread with its own runtime (the spawned binary needs a live
/// server for the whole run, independent of this test's executor).
fn serve_everything() -> String {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let app = router(HttpSecurityPolicy::default());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            sender
                .send(listener.local_addr().expect("addr"))
                .expect("send addr");
            let _ = axum::serve(listener, app).await;
        });
    });
    let addr = receiver.recv().expect("server starts");
    format!("http://{addr}/mcp")
}

#[test]
fn completed_run_exits_zero_and_renders_the_record() {
    // The initialize scenario's empty plan completes against any healthy
    // server (the tool-calling plans are exercised against the suite's own
    // scenario servers, whose surfaces they fit — the everything server's
    // test_error_handling would rightly exhaust a zero error budget).
    let url = serve_everything();
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"))
        .env("MCP_CONFORMANCE_SCENARIO", "initialize")
        .arg(&url)
        .output()
        .expect("binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "the empty plan completes against the everything server: {stderr}"
    );
    // The run record names the stop reason — `render`'s output is the
    // contract, not decoration.
    assert!(stderr.contains("Completed"), "{stderr}");
}

#[test]
fn exhausted_error_budget_exits_one() {
    // The generic plan calls every tool once with a zero error budget;
    // test_error_handling fails by design, so the run must stop exhausted
    // and the binary must exit 1 — the `Completed && clean_shutdown`
    // calculation, observed from outside.
    let url = serve_everything();
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"))
        .arg(&url)
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ErrorBudgetExhausted"), "{stderr}");
    assert!(
        stderr.contains("err  "),
        "the failing call is rendered: {stderr}"
    );
}

#[test]
fn trace_dir_records_a_validator_ready_trace() {
    let url = serve_everything();
    let dir = std::env::temp_dir().join(format!("host-cli-trace-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"))
        .env("MCP_CONFORMANCE_SCENARIO", "initialize")
        .arg("--trace-dir")
        .arg(&dir)
        .arg(&url)
        .output()
        .expect("binary runs");
    assert!(output.status.success(), "{output:?}");
    let trace = std::fs::read_dir(&dir)
        .expect("trace dir exists")
        .next()
        .expect("one trace file")
        .unwrap()
        .path();
    let bytes = std::fs::read_to_string(&trace).unwrap();
    let events = mcp_trace_validator::reader::parse_trace(
        &bytes,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .expect("binary-recorded trace parses through the validator's reader");
    assert!(
        events.len() >= 3,
        "the recorded handshake: {}",
        events.len()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn deadline_fires_against_a_server_that_never_answers() {
    // A listener that accepts and then says nothing: initialization can
    // never complete, so the watchdog must end the run with exit 1 and a
    // diagnostic naming the deadline — not hang until something kills us.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            // Hold the connection open, never respond.
            std::mem::forget(stream);
        }
    });
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"))
        .arg("--deadline-secs")
        .arg("1")
        .arg(format!("http://{addr}/mcp"))
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("deadline"), "{stderr}");
}

#[test]
fn sse_retry_scenario_runs_the_dance_through_the_binary() {
    // Against the everything server the dance's `test_reconnection` call is
    // answered immediately (as an unknown-tool error on the call stream), so
    // no reconnect happens — what this pins is the binary's sse-retry
    // dispatch and report, which only the real executable exercises. The
    // full timed dance is proven by the suite gate and the seam tests.
    let url = serve_everything();
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"))
        .env("MCP_CONFORMANCE_SCENARIO", "sse-retry")
        .arg(&url)
        .output()
        .expect("binary runs");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(stderr.contains("sse-retry dance completed"), "{stderr}");
}

#[test]
fn missing_url_and_command_is_an_invocation_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-reference-host"))
        .output()
        .expect("binary runs");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--server-cmd") && stderr.contains("URL"),
        "the rejection names both fixes: {stderr}"
    );
}
