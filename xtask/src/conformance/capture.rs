// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The capture leg of `cargo xtask conformance`: `mcp-trace-capture` between two
//! real endpoints, proving the claims its README makes.
//!
//! 1. **stdio, both revisions** — the reference host launches
//!    `mcp-trace-capture stdio -- mcp-everything-server` in place of the server. The
//!    session must complete, and the recorded trace must judge clean against the
//!    revision it declares — the path an outside user takes, end to end.
//! 2. **HTTP, the official suite** — the pinned runner drives the everything server
//!    *through* the proxy, and must still score green: the proxy is transparent to
//!    every scenario, `dns-rebinding-protection` included. The trace interleaves the
//!    suite's sessions, so it is checked for well-formedness, not judged.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::io::BufRead as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::Duration;

use super::{SPEC_VERSION, run_suite, start_server};

/// Where this leg writes its traces and results, under the workspace root.
const CAPTURE_DIR: &str = "target/conformance/capture";

/// The revisions the stdio leg records, one session each.
const REVISIONS: [&str; 2] = ["2025-11-25", "2026-07-28"];

pub(crate) fn run(root: &Path, suite: &str) -> ExitCode {
    eprintln!("xtask: conformance — building mcp-trace-capture");
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-trace-capture"])
        .current_dir(root)
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        eprintln!("xtask: conformance — capture build failed");
        return ExitCode::FAILURE;
    }
    let dir = root.join(CAPTURE_DIR);
    if let Err(error) = std::fs::remove_dir_all(&dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "xtask: conformance — cannot clear {}: {error}",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!(
            "xtask: conformance — cannot create {}: {error}",
            dir.display()
        );
        return ExitCode::FAILURE;
    }
    for revision in REVISIONS {
        if let Err(message) = stdio_session(root, &dir, revision) {
            eprintln!("xtask: capture — stdio {revision}: {message}");
            return ExitCode::FAILURE;
        }
    }
    if let Err(message) = suite_through_proxy(root, &dir, suite) {
        eprintln!("xtask: capture — {message}");
        return ExitCode::FAILURE;
    }
    eprintln!(
        "xtask: conformance — capture leg green: stdio sessions at {} judge clean, and \
         the suite passes through the HTTP proxy",
        REVISIONS.join(" and ")
    );
    ExitCode::SUCCESS
}

fn binary(root: &Path, name: &str) -> PathBuf {
    root.join(format!(
        "target/debug/{name}{}",
        std::env::consts::EXE_SUFFIX
    ))
}

/// One host session through the stdio wrapper, then its trace judged.
fn stdio_session(root: &Path, dir: &Path, revision: &str) -> Result<(), String> {
    let trace = dir.join(format!("stdio-{revision}.jsonl"));
    let server_command = format!(
        "{} --force -o {} stdio -- {} --transport stdio --protocol-version {revision}",
        binary(root, "mcp-trace-capture").display(),
        trace.display(),
        binary(root, "mcp-everything-server").display()
    );
    let status = Command::new(binary(root, "mcp-reference-host"))
        .args([
            "--protocol-version",
            revision,
            "--turn-limit",
            "40",
            "--error-budget",
            "5",
        ])
        .arg("--server-cmd")
        .arg(&server_command)
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run the host: {error}"))?;
    if !status.success() {
        return Err(format!(
            "the host session through the wrapper failed ({status})"
        ));
    }
    judge(&trace, revision)
}

/// The trace must declare `revision` and pass every judged clause of it.
fn judge(trace: &Path, revision: &str) -> Result<(), String> {
    let document = std::fs::read_to_string(trace)
        .map_err(|error| format!("cannot read {}: {error}", trace.display()))?;
    let events = mcp_trace_validator::reader::parse_trace(
        &document,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .map_err(|error| format!("{} is malformed: {error}", trace.display()))?;
    let set = mcp_conformance_core::requirement::RegistrySet::builtin()
        .map_err(|error| format!("registry set: {error}"))?;
    let selection = mcp_trace_validator::declared::select(set.revisions(), &events)
        .map_err(|error| error.to_string())?;
    let expected: mcp_conformance_core::revision::ProtocolRevision = revision
        .parse()
        .map_err(|_| format!("{revision} is not a protocol revision"))?;
    if selection.revisions != [expected] {
        return Err(format!(
            "the trace declares {:?}, not {revision}",
            selection.revisions
        ));
    }
    let registry = set
        .registry(expected)
        .ok_or_else(|| format!("no registry for {revision}"))?;
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    if report.has_errors() {
        return Err(format!(
            "{} does not judge clean:\n{}",
            trace.display(),
            report.render_human()
        ));
    }
    eprintln!("xtask: capture — {}: {}", trace.display(), report.totals);
    Ok(())
}

/// The pinned suite against the everything server, through the HTTP proxy.
fn suite_through_proxy(root: &Path, dir: &Path, suite: &str) -> Result<(), String> {
    let Some((mut server, address)) = start_server(root, &dir.join("tap"), SPEC_VERSION) else {
        return Err("the everything server did not start".to_owned());
    };
    let trace = dir.join("http-suite.jsonl");
    let proxy = start_proxy(root, &trace, &address);
    let outcome = proxy.and_then(|(mut proxy, proxy_address)| {
        let verdict = run_suite(root, &dir.join("suite"), &proxy_address, suite);
        let _ = proxy.kill();
        let _ = proxy.wait();
        if verdict == ExitCode::SUCCESS {
            Ok(())
        } else {
            Err("the official suite failed through the proxy".to_owned())
        }
    });
    let _ = server.kill();
    let _ = server.wait();
    outcome?;
    let document = std::fs::read_to_string(&trace)
        .map_err(|error| format!("cannot read {}: {error}", trace.display()))?;
    let events = mcp_trace_validator::reader::parse_trace(
        &document,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .map_err(|error| format!("{} is malformed: {error}", trace.display()))?;
    if events.is_empty() {
        return Err(format!("{} recorded nothing", trace.display()));
    }
    eprintln!(
        "xtask: capture — the suite passed through the proxy; {} event(s) recorded to {}",
        events.len(),
        trace.display()
    );
    Ok(())
}

/// Starts the proxy in front of `upstream` on an OS-assigned port, and returns it
/// with the address from its `listening on` line.
fn start_proxy(root: &Path, trace: &Path, upstream: &str) -> Result<(Child, String), String> {
    let mut proxy = Command::new(binary(root, "mcp-trace-capture"))
        .arg("--force")
        .arg("-o")
        .arg(trace)
        .args([
            "http",
            "--preserve-host",
            "--listen",
            "127.0.0.1:0",
            "--upstream",
        ])
        .arg(format!("http://{upstream}"))
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot start the proxy: {error}"))?;
    let stderr = proxy
        .stderr
        .take()
        .ok_or_else(|| "the proxy's stderr was not captured".to_owned())?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut sender = Some(sender);
        for line in std::io::BufReader::new(stderr).lines() {
            let Ok(line) = line else { break };
            if let Some(address) = line
                .split("listening on http://")
                .nth(1)
                .and_then(|rest| rest.split(',').next())
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(address.to_owned());
            }
            eprintln!("proxy: {line}");
        }
    });
    if let Ok(address) = receiver.recv_timeout(Duration::from_secs(30)) {
        return Ok((proxy, address));
    }
    let _ = proxy.kill();
    let _ = proxy.wait();
    Err("no `listening on` line from the proxy within 30s".to_owned())
}
