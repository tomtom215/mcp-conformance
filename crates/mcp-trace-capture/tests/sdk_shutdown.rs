// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The official SDKs' stdio shutdown, replayed against the wrapper: close the
//! server's stdin, wait 2 seconds, `SIGTERM`, wait 2 seconds, `SIGKILL`.
//! TypeScript's `StdioClientTransport` signals the wrapper's pid; Python's
//! `stdio_client` starts it in a session of its own and signals that process
//! group. Neither reaches the server's own group, so a wrapper still waiting
//! when the `SIGKILL` lands strands a server that ignores `SIGTERM` — and its
//! trace never gets its closing event.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::BufRead as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-sdk-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// Whether a process with this pid exists (`kill -0`).
fn exists(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn signal(name: &str, target: &str) {
    let status = Command::new("kill")
        .args([&format!("-{name}"), "--", target])
        .status()
        .unwrap();
    assert!(status.success(), "kill -{name} {target}");
}

/// Polls for `child` to exit within `limit`; whether it did.
fn exited_within(child: &mut std::process::Child, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Starts the wrapper in front of a server that ignores both its stdin closing
/// and `SIGTERM`, as an SDK would — in a process group of its own when `group`
/// — and returns it with the server's pid once the server is up.
fn start(trace: &Path, pid_file: &Path, group: bool) -> (Child, String) {
    let script = format!(
        "trap '' TERM; echo $$ > '{}'; echo '{{\"jsonrpc\":\"2.0\",\"method\":\"ready\"}}'; \
         while :; do sleep 0.1; done",
        pid_file.display()
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"));
    command
        .args([
            "-o",
            trace.to_str().unwrap(),
            "stdio",
            "--",
            "sh",
            "-c",
            &script,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if group {
        command.process_group(0);
    }
    let mut wrapper = command.spawn().unwrap();
    let mut stdout = std::io::BufReader::new(wrapper.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert!(line.contains("ready"), "{line}");
    let server = std::fs::read_to_string(pid_file).unwrap().trim().to_owned();
    (wrapper, server)
}

/// Runs the SDK sequence. `group` signals the wrapper's process group (Python)
/// rather than its pid (TypeScript).
fn sdk_shutdown(name: &str, group: bool) {
    let trace = scratch(&format!("{name}.jsonl"));
    let pid_file = scratch(&format!("{name}.pid"));
    let (mut wrapper, server) = start(&trace, &pid_file, group);
    let target = if group {
        format!("-{}", wrapper.id())
    } else {
        wrapper.id().to_string()
    };

    drop(wrapper.stdin.take());
    assert!(
        !exited_within(&mut wrapper, Duration::from_secs(2)),
        "the server ignores its stdin closing"
    );
    signal("TERM", &target);
    let exited = exited_within(&mut wrapper, Duration::from_secs(2));
    if !exited {
        signal("KILL", &target);
        wrapper.wait().unwrap();
    }
    let stranded = exists(&server);
    if stranded {
        // Not left running for the rest of the suite.
        signal("KILL", &format!("-{server}"));
    }
    assert!(
        exited,
        "the wrapper finished its stop before the SDK's SIGKILL"
    );
    assert!(!stranded, "the server was left running");
    let text = std::fs::read_to_string(&trace).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["event"], "transport-close", "{text}");
    assert_eq!(last["direction"], "client-to-server", "{text}");
    std::fs::remove_file(&trace).ok();
    std::fs::remove_file(&pid_file).ok();
}

#[test]
fn the_typescript_sdks_shutdown_neither_strands_the_server_nor_loses_the_close() {
    sdk_shutdown("ts", false);
}

#[test]
fn the_python_sdks_shutdown_neither_strands_the_server_nor_loses_the_close() {
    sdk_shutdown("py", true);
}
