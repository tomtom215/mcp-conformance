// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! How a capture ends when it is asked to stop: termination signals reach the
//! whole server (its process group, so a launcher such as `npx`, `uv run` or
//! `sh -c` does not strand the real server), the server gets a grace period to
//! exit on its own, the trace is closed, and nothing waits forever.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead as _, Write as _};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
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
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (ready, is_ready) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = std::io::BufReader::new(stdout).lines();
        if lines.next().is_some() {
            let _ = ready.send(());
        }
        for _ in lines {}
    });
    is_ready
        .recv_timeout(Duration::from_secs(20))
        .expect("the server announced itself");
    (Capture(child), stdin)
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

/// A server that ignores the relayed signal is killed after the grace period, and
/// a second signal skips the rest of the wait.
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
        started.elapsed() >= Duration::from_secs(2),
        "the server had its grace period: {:?}",
        started.elapsed()
    );
    assert_eq!(status.code(), Some(128 + 9), "{status:?}");
    assert_eq!(last_event(&trace)["event"], "transport-abort");
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
        started.elapsed() < Duration::from_secs(2),
        "the second signal ended the wait: {:?}",
        started.elapsed()
    );
    assert_eq!(status.code(), Some(128 + 9), "{status:?}");
    assert_eq!(last_event(&trace)["event"], "transport-abort");
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
/// exits — a child it started on the way out — gets the grace period to finish,
/// and its last line is relayed and recorded rather than killed mid-flight.
#[test]
fn output_written_after_the_server_exits_on_a_stop_is_kept() {
    let trace = scratch("late", "jsonl");
    let script = r#"trap '(sleep 0.5; echo "{\"jsonrpc\":\"2.0\",\"method\":\"late\"}") & exit 0' TERM; echo '{"jsonrpc":"2.0","method":"ready"}'; while :; do sleep 0.1; done"#;
    let (mut capture, stdin) = start(&trace, script);
    capture.signal("TERM");
    capture.wait_within(Duration::from_secs(10));
    drop(stdin);
    let text = std::fs::read_to_string(&trace).unwrap();
    assert!(
        text.lines().any(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["payload"]["method"] == "late"
        }),
        "{text}"
    );
    std::fs::remove_file(&trace).ok();
}
