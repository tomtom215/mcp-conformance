// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `http` subcommand as a user meets it: what it checks before it creates a
//! trace, what it says when it starts, and what it says when it stops.

#![cfg(all(unix, feature = "cli"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-capture"))
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-capture-cli-http-{name}-{}",
        std::process::id()
    ));
    std::fs::remove_file(&path).ok();
    path
}

/// A running proxy, killed if a test ends without it having exited.
struct Proxy(Child);

impl Drop for Proxy {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

impl Proxy {
    /// Starts `command` (an `http` invocation) and waits for it to announce its
    /// address; returns it, the address and its stderr lines so far and to come.
    fn start(mut command: Command) -> (Self, String, Receiver<String>) {
        let mut child = command.stderr(Stdio::piped()).spawn().unwrap();
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
            .recv_timeout(Duration::from_secs(30))
            .expect("the proxy announces its address");
        (Self(child), address, lines_rx)
    }

    /// Sends SIGINT and waits for the exit status.
    fn interrupt(&mut self) -> std::process::ExitStatus {
        let status = Command::new("kill")
            .args(["-INT", &self.0.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "the proxy did not stop");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

/// `http` with `-o trace`, listening on a free loopback port.
fn http(trace: &Path, upstream: &str) -> Command {
    let mut command = binary();
    command.args([
        "-o",
        trace.to_str().unwrap(),
        "http",
        "--listen",
        "127.0.0.1:0",
        "--upstream",
        upstream,
    ]);
    command
}

/// A directory and file holding no certificate, for `SSL_CERT_DIR` and
/// `SSL_CERT_FILE`: a platform with no root certificates to load.
fn no_root_certificates(command: &mut Command) -> PathBuf {
    let dir = scratch("no-certs");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("empty.pem");
    std::fs::write(&file, "").unwrap();
    command
        .env("SSL_CERT_FILE", &file)
        .env("SSL_CERT_DIR", &dir);
    dir
}

/// Root certificates are an `https` upstream's concern: a plain `http` one
/// works where none can be loaded (a minimal container), and an `https` one
/// that needs them is refused before a trace file exists. Both used to fail
/// after the trace was created and the address announced, leaving an empty file
/// the next run refused to overwrite.
#[test]
fn root_certificates_are_loaded_for_an_https_upstream_only_and_before_the_trace() {
    let trace = scratch("plain.jsonl");
    let mut command = http(&trace, "http://127.0.0.1:9/mcp");
    let dir = no_root_certificates(&mut command);
    let (mut proxy, _, lines) = Proxy::start(command);
    assert_eq!(proxy.interrupt().code(), Some(0));
    std::thread::sleep(Duration::from_millis(100));
    let stderr: Vec<String> = lines.try_iter().collect();
    assert!(
        !stderr.iter().any(|line| line.contains("certificates")),
        "{stderr:?}"
    );
    std::fs::remove_file(&trace).ok();

    if cfg!(feature = "tls") {
        let trace = scratch("tls.jsonl");
        let mut command = http(&trace, "https://127.0.0.1:9/mcp");
        no_root_certificates(&mut command);
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("root certificates"), "{stderr}");
        assert!(!stderr.contains("listening on"), "{stderr}");
        assert!(!trace.exists(), "the refused run left {}", trace.display());
    }
    std::fs::remove_dir_all(&dir).ok();
}
