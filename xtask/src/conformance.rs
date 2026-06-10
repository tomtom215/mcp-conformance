// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask conformance` — the official suite against the everything server.
//!
//! Orchestration, not a test: this task may use the network (npm registry for
//! the pinned runner) and real sockets, which `cargo test` never does — that
//! boundary is the reason it lives here and in its own CI job rather than in
//! any test target. Steps:
//!
//! 1. build `mcp-everything-server` (all features);
//! 2. serve it on an OS-assigned loopback port (`--transport http`);
//! 3. run `npx @modelcontextprotocol/conformance@<PIN> server` against it
//!    with the registry's spec revision and the committed expected-failures
//!    baseline;
//! 4. fail unless the runner exits green.
//!
//! Results land in `target/conformance/` (machine-readable, one JSON per
//! scenario) for the agreement check to consume.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::io::BufRead as _;
use std::process::{Command, ExitCode, Stdio};

/// The pinned official suite version. Bumps are deliberate changes: review
/// the upstream diff, refresh `conformance/expected-failures.yaml`, and
/// update register row 2.4 in the same commit.
pub(crate) const SUITE_VERSION: &str = "0.1.16";

/// Spec revision under test — the registry's revision.
const SPEC_VERSION: &str = "2025-11-25";

/// Committed baseline of explained failures (suite-native YAML). Empty today;
/// every entry must name the upstream issue explaining the divergence.
const EXPECTED_FAILURES: &str = "conformance/expected-failures.yaml";

pub(crate) fn run() -> ExitCode {
    eprintln!("xtask: conformance — building mcp-everything-server");
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-everything-server", "--all-features"])
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        eprintln!("xtask: conformance — server build failed");
        return ExitCode::FAILURE;
    }
    let Some((mut server, address)) = start_server() else {
        return ExitCode::FAILURE;
    };
    let outcome = run_suite(&address);
    let _ = server.kill();
    let _ = server.wait();
    outcome
}

/// Spawns the freshly built server on an OS-assigned port and returns the
/// child plus the address from its readiness line.
fn start_server() -> Option<(std::process::Child, String)> {
    let binary = format!(
        "target/debug/mcp-everything-server{}",
        std::env::consts::EXE_SUFFIX
    );
    eprintln!("xtask: conformance — starting {binary} on 127.0.0.1:0");
    let Ok(mut server) = Command::new(&binary)
        .args(["--transport", "http", "--bind", "127.0.0.1:0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    else {
        eprintln!("xtask: conformance — cannot spawn the server binary");
        return None;
    };
    let address = server.stderr.take().and_then(|stderr| {
        let mut line = String::new();
        std::io::BufReader::new(stderr).read_line(&mut line).ok()?;
        line.trim()
            .strip_prefix("listening on ")
            .map(ToOwned::to_owned)
    });
    let Some(address) = address else {
        eprintln!("xtask: conformance — no readiness line from the server");
        let _ = server.kill();
        return None;
    };
    Some((server, address))
}

/// Runs the pinned npx runner against the served address.
fn run_suite(address: &str) -> ExitCode {
    eprintln!(
        "xtask: conformance — running @modelcontextprotocol/conformance@{SUITE_VERSION} \
         (spec {SPEC_VERSION}) against http://{address}/mcp"
    );
    let status = Command::new("npx")
        .args([
            "-y",
            &format!("@modelcontextprotocol/conformance@{SUITE_VERSION}"),
            "server",
            "--url",
            &format!("http://{address}/mcp"),
            "--spec-version",
            SPEC_VERSION,
            "--expected-failures",
            EXPECTED_FAILURES,
            "--output-dir",
            "target/conformance",
        ])
        .status();
    match status {
        Ok(status) if status.success() => {
            eprintln!(
                "xtask: conformance — green against suite {SUITE_VERSION}; \
                 results in target/conformance/"
            );
            ExitCode::SUCCESS
        }
        Ok(status) => {
            eprintln!(
                "xtask: conformance — runner reported failures ({status}); \
                 inspect target/conformance/ and either fix the server or \
                 record an explained divergence in {EXPECTED_FAILURES}"
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("xtask: conformance — cannot run npx (Node 18+ required): {error}");
            ExitCode::FAILURE
        }
    }
}
