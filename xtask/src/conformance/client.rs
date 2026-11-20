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
//! 3. **The `auth/*` scenarios** (`suite::AUTH_CLIENT_SCENARIOS`), with the
//!    host's `--oauth-conformance`: the suite's own authorization server, the
//!    host authorizing headlessly. Not recorded and not part of the agreement
//!    replay — the flow runs between the client and the authorization server,
//!    off the message seam a host trace is taken at (the registry judges what
//!    a recording does show of authorization, the server's challenge, under
//!    `AUTH-*`). First the pinned suite's `list` is checked against the two
//!    lists in `suite.rs`, so a scenario the suite adds cannot go unrun
//!    unnoticed. Only at the pinned suite version: the set is a property of it.
//! 4. **Client-side agreement** — every host-captured trace replays through
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

/// The four protocol scenarios the pinned suite defines for `2025-11-25`:
/// the set `mcp-conformance test --client-command` runs by default too.
const CLIENT_SCENARIOS: [&str; 4] = mcp_conformance_cli::suite::CLIENT_SCENARIOS;

/// Committed client-side divergence baseline, relative to the workspace root.
const CLIENT_DIVERGENCE_BASELINE: &str = "conformance/client-agreement-divergences.json";

/// Where the host records its traces, under the workspace root.
const CLIENT_TAP_DIR: &str = "target/conformance/client-tap";

/// Where the client runner writes per-scenario results.
const CLIENT_RESULTS_DIR: &str = "target/conformance/client";

/// The `auth/*` scenarios the host is run against, the expected failure among
/// them (`conformance/expected-failures.yaml`).
const AUTH_SCENARIOS: [&str; 20] = mcp_conformance_cli::suite::AUTH_CLIENT_SCENARIOS;

/// Where the client runner writes the `auth/*` results: beside, not inside,
/// the directory the agreement replay reads.
const AUTH_RESULTS_DIR: &str = "target/conformance/client-auth";

/// Builds the host as the gate runs it. `oauth` is not a default feature:
/// `--oauth-conformance` exists only in a host built for this gate.
fn build_host(root: &Path) -> bool {
    eprintln!("xtask: conformance — building mcp-reference-host (client SUT)");
    let features = ["--features", "cli,oauth"];
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-reference-host"])
        .args(features)
        .current_dir(root)
        .status();
    matches!(build, Ok(status) if status.success())
}

pub(crate) fn run(root: &Path, suite: &str) -> ExitCode {
    if !build_host(root) {
        eprintln!("xtask: conformance — host build failed");
        return ExitCode::FAILURE;
    }
    let host = root.join(format!(
        "target/debug/mcp-reference-host{}",
        std::env::consts::EXE_SUFFIX
    ));
    let tap_dir = root.join(CLIENT_TAP_DIR);
    let results_dir = root.join(CLIENT_RESULTS_DIR);
    let auth_results_dir = root.join(AUTH_RESULTS_DIR);
    // Fresh artifacts every run, like the server leg.
    for dir in [&tap_dir, &results_dir, &auth_results_dir] {
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
    // `--command` is split on spaces by the runner, so the flags survive and
    // the scenario URL lands as the final argument.
    let recorded = format!("{} --trace-dir {}", host.display(), tap_dir.display());
    if run_scenarios(root, &recorded, &CLIENT_SCENARIOS, &results_dir, suite) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }
    if auth_scenarios(root, &host, &auth_results_dir, suite) != ExitCode::SUCCESS {
        return ExitCode::FAILURE;
    }

    match crate::agreement::run_with(
        root,
        &tap_dir,
        &results_dir,
        suite,
        CLIENT_DIVERGENCE_BASELINE,
        false,
        &crate::agreement::schema::CLIENT_LEG,
    ) {
        Ok(()) => {
            eprintln!(
                "xtask: conformance — client leg green: stdio smoke, {} protocol \
                 scenarios, the auth scenarios, agreement over the host-captured traces",
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

/// The `auth/*` phase: the list check, then the scenarios.
fn auth_scenarios(root: &Path, host: &Path, results_dir: &Path, suite: &str) -> ExitCode {
    if suite != mcp_conformance_cli::suite::VERSION {
        eprintln!(
            "xtask: conformance — auth client scenarios skipped: the set is the pinned \
             suite's ({}), and this run is of {suite}",
            mcp_conformance_cli::suite::VERSION
        );
        return ExitCode::SUCCESS;
    }
    let listed = Command::new("npx")
        .arg("-y")
        .arg(format!("{}@{suite}", super::SUITE_PACKAGE))
        .arg("list")
        .current_dir(root)
        .output();
    let listed = match listed {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        _ => {
            eprintln!("xtask: conformance — cannot list the suite's scenarios");
            return ExitCode::FAILURE;
        }
    };
    if let Err(message) = auth_set_is_the_listed_one(&listed) {
        eprintln!("xtask: conformance — {message}");
        return ExitCode::FAILURE;
    }
    let command = format!("{} --oauth-conformance", host.display());
    run_scenarios(root, &command, &AUTH_SCENARIOS, results_dir, suite)
}

/// Checks that the suite's listed `auth/*` scenarios are exactly the ones
/// `suite.rs` runs plus the ones it says it does not.
fn auth_set_is_the_listed_one(list_output: &str) -> Result<(), String> {
    use mcp_conformance_cli::suite::{AUTH_CLIENT_SCENARIOS_NOT_RUN, listed_auth_scenarios};

    let mut listed = listed_auth_scenarios(list_output);
    let mut known: Vec<String> = AUTH_SCENARIOS
        .iter()
        .chain(&AUTH_CLIENT_SCENARIOS_NOT_RUN)
        .map(|name| (*name).to_owned())
        .collect();
    listed.sort_unstable();
    known.sort_unstable();
    if listed == known {
        return Ok(());
    }
    let unlisted: Vec<&String> = known.iter().filter(|n| !listed.contains(n)).collect();
    let unknown: Vec<&String> = listed.iter().filter(|n| !known.contains(n)).collect();
    Err(format!(
        "the pinned suite's auth client scenarios are not the ones suite.rs names: \
         listed but in neither AUTH_CLIENT_SCENARIOS nor AUTH_CLIENT_SCENARIOS_NOT_RUN: \
         {unknown:?}; named there but not listed: {unlisted:?}"
    ))
}

/// `scenarios`, sequentially, with `command` as the client under test.
fn run_scenarios(
    root: &Path,
    command: &str,
    scenarios: &[&str],
    results_dir: &Path,
    suite: &str,
) -> ExitCode {
    for scenario in scenarios {
        eprintln!("xtask: conformance — client scenario {scenario} (suite {suite})");
        let status = Command::new("npx")
            .arg("-y")
            .arg(format!("{}@{suite}", super::SUITE_PACKAGE))
            .arg("client")
            .arg("--scenario")
            .arg(scenario)
            .arg("--command")
            .arg(command)
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn list_of(names: &[&str]) -> String {
        let mut output = "Client scenarios (test against a client):\n".to_owned();
        for name in names {
            output.push_str("  - ");
            output.push_str(name);
            output.push_str(" [2025-11-25]\n");
        }
        output
    }

    fn known() -> Vec<&'static str> {
        AUTH_SCENARIOS
            .iter()
            .chain(&mcp_conformance_cli::suite::AUTH_CLIENT_SCENARIOS_NOT_RUN)
            .copied()
            .collect()
    }

    #[test]
    fn the_listed_set_matches_in_any_order() {
        let mut names = known();
        names.reverse();
        names.push("initialize");
        assert_eq!(auth_set_is_the_listed_one(&list_of(&names)), Ok(()));
    }

    #[test]
    fn a_scenario_the_suite_added_or_dropped_is_named() {
        let mut added = known();
        added.push("auth/brand-new");
        let message = auth_set_is_the_listed_one(&list_of(&added)).unwrap_err();
        assert!(
            message.contains("[\"auth/brand-new\"]; named there"),
            "{message}"
        );
        assert!(message.ends_with("not listed: []"), "{message}");

        let dropped = &known()[1..];
        let message = auth_set_is_the_listed_one(&list_of(dropped)).unwrap_err();
        assert!(
            message.ends_with("not listed: [\"auth/metadata-default\"]"),
            "{message}"
        );
    }
}
