// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The client leg of `cargo xtask conformance`: the reference host as the
//! pinned suite's client SUT (roadmap M3; ADR-0009 §Amendment).
//!
//! Three phases, after the server leg is green:
//!
//! 1. **Child-process smoke** — the host binary spawns the everything-server
//!    binary over stdio (`--server-cmd`), completes the handshake, exits
//!    clean, and its captured trace replays through the validator with the
//!    client leg's agreement baseline. This is the one place two real
//!    binaries meet over a real pipe — `cargo test` cannot reference a
//!    sibling crate's executable.
//! 2. **The four `2025-11-25` protocol scenarios**, sequentially: parallel
//!    suite mode would put the `sse-retry` clock under load noise, and
//!    client runs fail on WARNINGs, so the timing window is load-bearing.
//!    The `auth/*` set is deferred (registry TRAN-009; deferral ledger row
//!    `auth-client-scenarios`).
//! 3. **Client-side agreement** — every host-captured trace replays through
//!    `mcp-trace-validator` against
//!    `conformance/client-agreement-divergences.json`. The `sse-retry`
//!    dance records no trace (it runs below the `Transport` seam), so the
//!    replay set is the smoke session plus the three agent scenarios.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::Path;
use std::process::{Command, ExitCode};

use super::{EXPECTED_FAILURES, SPEC_VERSION};

/// The four protocol scenarios the pinned suite defines for `2025-11-25`.
const CLIENT_SCENARIOS: [&str; 4] = [
    "initialize",
    "tools_call",
    "elicitation-sep1034-client-defaults",
    "sse-retry",
];

/// Committed client-side divergence baseline, relative to the workspace root.
const CLIENT_DIVERGENCE_BASELINE: &str = "conformance/client-agreement-divergences.json";

/// Where the host records its traces, under the workspace root.
const CLIENT_TAP_DIR: &str = "target/conformance/client-tap";

/// Where the client runner writes per-scenario results.
const CLIENT_RESULTS_DIR: &str = "target/conformance/client";

pub(crate) fn run(root: &Path, suite: &str) -> ExitCode {
    eprintln!("xtask: conformance — building mcp-reference-host (client SUT)");
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-reference-host", "--features", "cli"])
        .current_dir(root)
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        eprintln!("xtask: conformance — host build failed");
        return ExitCode::FAILURE;
    }
    let host = root.join(format!(
        "target/debug/mcp-reference-host{}",
        std::env::consts::EXE_SUFFIX
    ));
    let tap_dir = root.join(CLIENT_TAP_DIR);
    let results_dir = root.join(CLIENT_RESULTS_DIR);
    // Fresh artifacts every run, like the server leg.
    for dir in [&tap_dir, &results_dir] {
        if let Err(error) = std::fs::remove_dir_all(dir)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "xtask: conformance — cannot clear {}: {error}",
                dir.display()
            );
            return ExitCode::FAILURE;
        }
    }

    if stdio_smoke(root, &host, &tap_dir) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    if run_scenarios(root, &host, &tap_dir, &results_dir, suite) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }

    match crate::agreement::run_with(
        root,
        &tap_dir,
        &results_dir,
        suite,
        CLIENT_DIVERGENCE_BASELINE,
        false,
    ) {
        Ok(()) => {
            eprintln!(
                "xtask: conformance — client leg green: stdio smoke, {} scenarios, \
                 agreement over the host-captured traces",
                CLIENT_SCENARIOS.len()
            );
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("xtask: client agreement — {message}");
            ExitCode::FAILURE
        }
    }
}

/// The four scenarios, sequentially, against the freshly built host.
fn run_scenarios(
    root: &Path,
    host: &Path,
    tap_dir: &Path,
    results_dir: &Path,
    suite: &str,
) -> ExitCode {
    // `--command` is split on spaces by the runner, so the flags survive and
    // the scenario URL lands as the final argument.
    let command = format!("{} --trace-dir {}", host.display(), tap_dir.display());
    for scenario in CLIENT_SCENARIOS {
        eprintln!("xtask: conformance — client scenario {scenario} (suite {suite})");
        let status = Command::new("npx")
            .arg("-y")
            .arg(format!("@modelcontextprotocol/conformance@{suite}"))
            .arg("client")
            .arg("--scenario")
            .arg(scenario)
            .arg("--command")
            .arg(&command)
            .arg("--spec-version")
            .arg(SPEC_VERSION)
            .arg("--expected-failures")
            .arg(root.join(EXPECTED_FAILURES))
            .arg("--output-dir")
            .arg(results_dir)
            .current_dir(root)
            .status();
        if !matches!(status, Ok(status) if status.success()) {
            eprintln!(
                "xtask: conformance — client scenario {scenario} failed; inspect {} \
                 (per-scenario checks.json plus the host's own stderr in stderr.txt). \
                 Whole-scenario baselines go under the `client:` key of \
                 {EXPECTED_FAILURES}",
                results_dir.display()
            );
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// Two real binaries over a real pipe: the host spawns the everything-server
/// over stdio, completes the handshake under the `initialize` plan, exits
/// clean, and leaves a captured trace for the agreement replay.
fn stdio_smoke(root: &Path, host: &Path, tap_dir: &Path) -> ExitCode {
    let server = root.join(format!(
        "target/debug/mcp-everything-server{}",
        std::env::consts::EXE_SUFFIX
    ));
    eprintln!(
        "xtask: conformance — stdio smoke: {} --server-cmd {}",
        host.display(),
        server.display()
    );
    let status = Command::new(host)
        .env("MCP_CONFORMANCE_SCENARIO", "initialize")
        .arg("--server-cmd")
        .arg(format!("{} --transport stdio", server.display()))
        .arg("--trace-dir")
        .arg(tap_dir)
        .current_dir(root)
        .status();
    match status {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!(
                "xtask: conformance — stdio smoke failed ({status}): the host could \
                 not complete a handshake against the everything-server binary over \
                 a child-process pipe"
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("xtask: conformance — cannot run the host binary: {error}");
            ExitCode::FAILURE
        }
    }
}
