// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How a capture ends when it is asked to stop: termination signals reach the
//! whole server (its process group, so a launcher such as `npx`, `uv run` or
//! `sh -c` does not strand the real server), the server gets a grace period to
//! exit on its own, the trace is closed, and nothing waits forever.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str, extension: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-signals-{name}-{}.{extension}",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// A running capture, killed if a test ends without it having exited — so a
/// regression that hangs fails the test instead of leaking the process.
struct Capture(Child);

impl Drop for Capture {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl Capture {
    fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .args([&format!("-{name}"), &self.0.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// Waits for exit, failing the test after `limit`.
    fn wait_within(&mut self, limit: Duration) -> ExitStatus {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "the capture did not exit within {limit:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// Starts `stdio -- sh -c script` with piped stdio, and waits until the server's
/// first stdout line has come through — so the server is up, and has installed
/// whatever handler the script sets, before the test signals anything.
fn start(trace: &std::path::Path, script: &str) -> (Capture, std::process::ChildStdin) {
    let (capture, stdin, _) = start_relaying(trace, script);
    (capture, stdin)
}

/// [`start`], also returning every later line the client receives, followed —
/// once the capture has exited — by its whole stderr, prefixed `stderr: `.
fn start_relaying(
    trace: &std::path::Path,
    script: &str,
) -> (Capture, std::process::ChildStdin, Receiver<String>) {
    let mut child = binary()
        .args([
            "-o",
            trace.to_str().unwrap(),
            "stdio",
            "--",
            "sh",
            "-c",
            script,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (lines_tx, lines) = std::sync::mpsc::channel();
    let (done_tx, done) = std::sync::mpsc::channel();
    let stderr_tx = lines_tx.clone();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let _ = lines_tx.send(line.unwrap_or_default());
        }
        let _ = done_tx.send(());
    });
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::BufReader::new(stderr).read_to_string(&mut text);
        // After stdout's last line, so the order is fixed.
        let _ = done.recv();
        let _ = stderr_tx.send(format!("stderr: {text}"));
    });
    lines
        .recv_timeout(Duration::from_secs(20))
        .expect("the server announced itself");
    (Capture(child), stdin, lines)
}

fn last_event(trace: &std::path::Path) -> serde_json::Value {
    let text = std::fs::read_to_string(trace).unwrap();
    serde_json::from_str(text.lines().last().unwrap()).unwrap()
}

/// A server reached through a launcher — here `sh -c`, which forks the real
/// server instead of replacing itself — gets the signal, cleans up, and the
/// capture exits promptly with a closed trace. Before the group relay, the
/// launcher alone was killed, the real server kept the stdout pipe open, and the
/// capture waited for it forever, ignoring every further signal.
#[test]
fn sigterm_reaches_a_server_behind_a_launcher_and_the_capture_exits() {
    let trace = scratch("launcher", "jsonl");
    let marker = scratch("launcher", "marker");
    let inner = format!(
        "trap 'echo graceful > {}; exit 0' TERM; echo '{{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}}'; \
         while :; do sleep 0.1; done",
        marker.display()
    );
    // `; true` keeps the outer shell from exec-ing the inner one.
    let script = format!("sh -c \"{}\"; true", inner.replace('"', "\\\""));
    let (mut capture, stdin) = start(&trace, &script);
    capture.signal("TERM");
    let status = capture.wait_within(Duration::from_secs(10));
    drop(stdin);
    assert_eq!(
        std::fs::read_to_string(&marker).ok().as_deref(),
        Some("graceful\n"),
        "the real server ran its own SIGTERM handler"
    );
    // The launcher shell died of the relayed SIGTERM: 128 + 15.
    assert_eq!(status.code(), Some(143), "{status:?}");
    let last = last_event(&trace);
    assert_eq!(last["direction"], "client-to-server", "{last}");
    assert_eq!(last["kind"], "lifecycle", "{last}");
    std::fs::remove_file(&trace).ok();
    std::fs::remove_file(&marker).ok();
}

/// SIGHUP — a closed terminal, a dropped SSH session — is relayed like SIGTERM
/// rather than killing the capture and orphaning the server: the server exits on
/// its own terms, its status is the capture's, and the trace is closed.
#[test]
fn sighup_is_relayed_and_the_servers_clean_exit_closes_the_trace() {
    let trace = scratch("hup", "jsonl");
    let (mut capture, stdin) = start(
        &trace,
        "trap 'exit 0' HUP; echo '{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}'; while :; do sleep 0.1; done",
    );
    capture.signal("HUP");
    let status = capture.wait_within(Duration::from_secs(10));
    drop(stdin);
    assert_eq!(status.code(), Some(0), "{status:?}");
    let last = last_event(&trace);
    assert_eq!(last["event"], "transport-close", "{last}");
    assert_eq!(last["direction"], "client-to-server", "{last}");
    std::fs::remove_file(&trace).ok();
}

/// SIGQUIT (a terminal's Ctrl-\) is a request to stop like the others. Its
/// default action ended the capture on the spot, and the server — in a process
/// group of its own, so the terminal's signal never reached it — kept running
/// with no trace closed.
#[test]
fn sigquit_is_relayed_and_the_trace_is_closed() {
    let trace = scratch("quit", "jsonl");
    let (mut capture, stdin) = start(
        &trace,
        "trap 'exit 0' QUIT; echo '{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}'; while :; do sleep 0.1; done",
    );
    capture.signal("QUIT");
    let status = capture.wait_within(Duration::from_secs(10));
    drop(stdin);
    assert_eq!(status.code(), Some(0), "{status:?}");
    let last = last_event(&trace);
    assert_eq!(last["event"], "transport-close", "{last}");
    std::fs::remove_file(&trace).ok();
}

/// A server that ignores the relayed signal is killed after the grace period —
/// which ends inside the 2 seconds the official SDKs allow between `SIGTERM` and
/// `SIGKILL` — and a second signal skips the rest of the wait. The trace was
/// closed when the stop began, so its last event is that close.
#[test]
fn a_server_that_ignores_the_signal_is_killed_and_a_second_signal_hurries_it() {
    let script = "trap '' TERM INT; echo '{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}'; while :; do sleep 0.1; done";

    let trace = scratch("grace", "jsonl");
    let (mut capture, stdin) = start(&trace, script);
    let started = Instant::now();
    capture.signal("TERM");
    let status = capture.wait_within(Duration::from_secs(15));
    drop(stdin);
    assert!(
        started.elapsed() >= mcp_trace_capture::SERVER_STOP_GRACE,
        "the server had its grace period: {:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the stop ended before an SDK's SIGKILL would: {:?}",
        started.elapsed()
    );
    assert_eq!(status.code(), Some(128 + 9), "{status:?}");
    let last = last_event(&trace);
    assert_eq!(last["event"], "transport-close", "{last}");
    assert_eq!(last["direction"], "client-to-server", "{last}");
    std::fs::remove_file(&trace).ok();

    let trace = scratch("second", "jsonl");
    let (mut capture, stdin) = start(&trace, script);
    let started = Instant::now();
    capture.signal("INT");
    std::thread::sleep(Duration::from_millis(300));
    capture.signal("INT");
    let status = capture.wait_within(Duration::from_secs(15));
    drop(stdin);
    assert!(
        started.elapsed() < mcp_trace_capture::SERVER_STOP_GRACE,
        "the second signal ended the wait: {:?}",
        started.elapsed()
    );
    assert_eq!(status.code(), Some(128 + 9), "{status:?}");
    assert_eq!(last_event(&trace)["event"], "transport-close");
    std::fs::remove_file(&trace).ok();
}

/// The trace is closed the moment a stop begins, not when it ends: a client
/// that kills the wrapper during the grace period (the official SDKs do, 2
/// seconds after `SIGTERM`) still leaves a closed trace.
#[test]
fn the_trace_is_closed_as_soon_as_a_stop_begins() {
    let trace = scratch("early-close", "jsonl");
    let (mut capture, stdin) = start(
        &trace,
        "trap '' TERM; echo '{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}'; while :; do sleep 0.1; done",
    );
    capture.signal("TERM");
    let deadline = Instant::now() + Duration::from_millis(800);
    let closed = loop {
        let closed = std::fs::read_to_string(&trace)
            .unwrap()
            .lines()
            .last()
            .is_some_and(|line| line.contains("transport-close"));
        if closed || Instant::now() > deadline {
            break closed;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(
        matches!(capture.0.try_wait(), Ok(None)),
        "the capture is still in its grace period"
    );
    assert!(closed, "the trace was closed when the stop began");
    capture.wait_within(Duration::from_secs(10));
    drop(stdin);
    std::fs::remove_file(&trace).ok();
}

/// A server that exits while the client is still writing to it closed the
/// session itself. The client's relay then fails writing to the dead pipe, and
/// that failure must not be read as the client closing first — which made the
/// trace's last word depend on which of the two the runtime noticed first.
#[test]
fn a_server_exiting_under_a_writing_client_is_always_the_closer() {
    for round in 0..5 {
        let trace = scratch(&format!("closer-{round}"), "jsonl");
        let mut child = binary()
            .args([
                "-o",
                trace.to_str().unwrap(),
                "stdio",
                "--",
                "sh",
                "-c",
                "sleep 0.3; exit 0",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let line = format!(
            "{{\"jsonrpc\":\"2.0\",\"method\":\"n\",\"p\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let writer = std::thread::spawn(move || {
            // More than the pipe holds; the server never reads it. Then hold
            // stdin open, as a client does, until the test drops it.
            for _ in 0..1000 {
                if stdin.write_all(line.as_bytes()).is_err() {
                    break;
                }
            }
            stdin
        });
        let mut capture = Capture(child);
        let status = capture.wait_within(Duration::from_secs(20));
        drop(writer.join().unwrap());
        assert!(status.success(), "{status:?}");
        let last = last_event(&trace);
        assert_eq!(
            last["direction"], "server-to-client",
            "round {round}: {last}"
        );
        assert_eq!(last["event"], "transport-close", "round {round}: {last}");
        std::fs::remove_file(&trace).ok();
    }
}

/// After a stop request, what still holds the server's stdout once the server
/// exits — a child it started on the way out — has the rest of the grace period
/// to finish, and its last line is relayed rather than killed mid-flight. The
/// trace was closed when the stop began, so the line is counted, not recorded —
/// and not reported as a non-JSON finding the trace does not hold.
#[test]
fn output_written_after_the_server_exits_on_a_stop_is_relayed_and_counted() {
    let trace = scratch("late", "jsonl");
    let script = r#"trap '(sleep 0.5; echo late) & exit 0' TERM; echo '{"jsonrpc":"2.0","method":"ready"}'; while :; do sleep 0.1; done"#;
    let (mut capture, stdin, lines) = start_relaying(&trace, script);
    capture.signal("TERM");
    capture.wait_within(Duration::from_secs(10));
    drop(stdin);
    let received: Vec<String> = lines.iter().collect();
    assert_eq!(
        received.first().map(String::as_str),
        Some("late"),
        "{received:?}"
    );
    let stderr = received.last().unwrap();
    assert!(
        stderr.contains("1 event(s) came after the trace was closed"),
        "{stderr}"
    );
    assert!(!stderr.contains("not JSON"), "{stderr}");
    let text = std::fs::read_to_string(&trace).unwrap();
    assert!(!text.contains("late"), "{text}");
    assert!(
        text.lines().last().unwrap().contains("transport-close"),
        "{text}"
    );
    std::fs::remove_file(&trace).ok();
}
