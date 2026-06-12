// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask conformance` — the official suite against the everything server.
//!
//! Orchestration, not a test: this task may use the network (npm registry for
//! the pinned runner) and real sockets, which `cargo test` never does — that
//! boundary is the reason it lives here and in its own CI job rather than in
//! any test target. Steps:
//!
//! 1. build `mcp-everything-server` (all features — includes the tap);
//! 2. serve it on an OS-assigned loopback port (`--transport http`), with
//!    every session recorded to `target/conformance/tap/`;
//! 3. run `npx @modelcontextprotocol/conformance@<PIN> server` against it
//!    with the registry's spec revision and the committed expected-failures
//!    baseline;
//! 4. fail unless the runner exits green;
//! 5. reconcile the runner's verdicts with our validator's over the tapped
//!    sessions and check the coverage manifest (`agreement.rs`) — the
//!    agreement check from docs/plan/03-conformance-strategy.md §Calibration.
//!
//! Results land in `target/conformance/` (one JSON per scenario, the tapped
//! traces, and `agreement.json`). All paths are anchored at the workspace
//! root, so the task behaves the same from any working directory.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::io::BufRead as _;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

/// The pinned official suite version. Bumps are deliberate changes: review
/// the upstream diff, refresh `conformance/expected-failures.yaml`, and
/// update register row 2.4 in the same commit. `MCP_SUITE_VERSION` overrides
/// it for the scheduled alpha-tracking job only — the PR gate always runs
/// the pin.
pub(crate) const SUITE_VERSION: &str = "0.1.16";

/// The suite version this run uses: the pin, unless `MCP_SUITE_VERSION`
/// overrides it (the scheduled alpha-line early-warning job).
fn suite_version() -> String {
    std::env::var("MCP_SUITE_VERSION").unwrap_or_else(|_| SUITE_VERSION.to_owned())
}

/// Spec revision under test — the registry's revision.
pub(crate) const SPEC_VERSION: &str = "2025-11-25";

/// Committed baseline of expected scenario failures (suite-native YAML:
/// `server:`/`client:` keys). Empty today; whole-scenario entries only —
/// requirement-level triage lives in the divergence baselines.
pub(crate) const EXPECTED_FAILURES: &str = "conformance/expected-failures.yaml";

/// Where the server tap records each suite session, under the workspace root.
const TAP_DIR: &str = "target/conformance/tap";

/// Where the runner writes per-scenario results and the agreement artifact,
/// under the workspace root.
const RESULTS_DIR: &str = "target/conformance";

/// How long to wait for the server's readiness line before declaring the
/// spawn failed. Generous: the binary is already built, so startup is
/// socket-bind plus runtime init.
const READINESS_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn run() -> ExitCode {
    let root = crate::workspace_root();
    let suite = suite_version();
    eprintln!("xtask: conformance — building mcp-everything-server");
    let build = Command::new("cargo")
        .args(["build", "-p", "mcp-everything-server", "--all-features"])
        .current_dir(&root)
        .status();
    if !matches!(build, Ok(status) if status.success()) {
        eprintln!("xtask: conformance — server build failed");
        return ExitCode::FAILURE;
    }
    let results_dir = root.join(RESULTS_DIR);
    let tap_dir = root.join(TAP_DIR);
    // Fresh artifacts every run: stale scenario results or tapped sessions
    // from a previous invocation must not leak into this reconciliation.
    if let Err(error) = std::fs::remove_dir_all(&results_dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "xtask: conformance — cannot clear {}: {error}",
            results_dir.display()
        );
        return ExitCode::FAILURE;
    }
    let Some((mut server, address)) = start_server(&root, &tap_dir) else {
        return ExitCode::FAILURE;
    };
    let outcome = run_suite(&root, &results_dir, &address, &suite);
    if outcome != ExitCode::SUCCESS {
        let _ = server.kill();
        let _ = server.wait();
        return outcome;
    }
    // Let the tap's writer drain before the server dies: poll the tap
    // directory until its total size is stable, then terminate.
    await_tap_quiescence(&tap_dir);
    let _ = server.kill();
    let _ = server.wait();
    if let Err(message) = crate::agreement::run(&root, &tap_dir, &results_dir, &suite) {
        eprintln!("xtask: agreement — {message}");
        return ExitCode::FAILURE;
    }
    client::run(&root, &suite)
}

/// The client leg: the host binary as the suite's SUT, plus the
/// child-process transport's real-binary proof and the client-side
/// agreement replay.
mod client;

/// Waits until the tap directory's contents stop growing (three consecutive
/// identical size samples 150 ms apart), capped at ten seconds. The tap
/// flushes each event before taking the next, so a stable size means the
/// writer is idle; requiring two stable windows in a row tolerates one
/// scheduler stall between flushes. Hitting the cap is loud, not silent —
/// the agreement check that follows would otherwise read a file the writer
/// is still appending to.
fn await_tap_quiescence(tap_dir: &Path) {
    let total = || -> u64 {
        std::fs::read_dir(tap_dir).map_or(0, |entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| entry.metadata().ok())
                .map(|metadata| metadata.len())
                .sum()
        })
    };
    let mut previous = total();
    let mut stable_samples = 0;
    for _ in 0..66 {
        std::thread::sleep(Duration::from_millis(150));
        let current = total();
        if current == previous {
            stable_samples += 1;
            if stable_samples >= 2 {
                return;
            }
        } else {
            stable_samples = 0;
        }
        previous = current;
    }
    eprintln!(
        "xtask: conformance — tap directory never went quiescent within 10s; \
         the tapped traces may be incomplete and the agreement check may \
         report artifacts of truncation"
    );
}

/// Spawns the freshly built server on an OS-assigned port and returns the
/// child plus the address from its readiness line.
///
/// The readiness protocol is one line on stderr (`listening on <addr>`),
/// guaranteed first by the server binary. Reading it is bounded by
/// [`READINESS_TIMEOUT`] so a wedged spawn fails the task instead of hanging
/// it, and the rest of the child's stderr is drained (and forwarded) so a
/// chatty server can never fill the pipe and deadlock against it.
fn start_server(root: &Path, tap_dir: &Path) -> Option<(std::process::Child, String)> {
    let binary = root.join(format!(
        "target/debug/mcp-everything-server{}",
        std::env::consts::EXE_SUFFIX
    ));
    eprintln!(
        "xtask: conformance — starting {} on 127.0.0.1:0 (tap: {})",
        binary.display(),
        tap_dir.display()
    );
    let Ok(mut server) = Command::new(&binary)
        .arg("--transport")
        .arg("http")
        .arg("--bind")
        .arg("127.0.0.1:0")
        .arg("--tap-dir")
        .arg(tap_dir)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    else {
        eprintln!("xtask: conformance — cannot spawn the server binary");
        return None;
    };
    let address = server.stderr.take().and_then(await_readiness_line);
    let Some(address) = address else {
        eprintln!(
            "xtask: conformance — no readiness line from the server within \
             {}s (expected `listening on <addr>` first on stderr)",
            READINESS_TIMEOUT.as_secs()
        );
        let _ = server.kill();
        let _ = server.wait();
        return None;
    };
    Some((server, address))
}

/// Reads the readiness line from the child's stderr, bounded by
/// [`READINESS_TIMEOUT`], then keeps the pipe drained in the background.
fn await_readiness_line(stderr: std::process::ChildStderr) -> Option<String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stderr);
        let mut line = String::new();
        let first = reader.read_line(&mut line).ok().map(|_| line);
        // The parent only waits on the first line; if it timed out and killed
        // the child, this send goes nowhere and the thread exits on EOF.
        let _ = sender.send(first);
        // Keep draining so the child can never block on a full stderr pipe;
        // forward for diagnosability.
        let mut rest = String::new();
        loop {
            rest.clear();
            match reader.read_line(&mut rest) {
                Ok(0) | Err(_) => break,
                Ok(_) => eprint!("server: {rest}"),
            }
        }
    });
    let first = receiver.recv_timeout(READINESS_TIMEOUT).ok().flatten()?;
    let address = first.trim().strip_prefix("listening on ");
    if address.is_none() && !first.trim().is_empty() {
        // A first line that is not the readiness line is the server saying
        // why it could not start; losing it would leave only "no readiness
        // line" as the diagnosis.
        eprint!("server: {first}");
    }
    address.map(ToOwned::to_owned)
}

/// Runs the pinned npx runner against the served address.
fn run_suite(root: &Path, results_dir: &Path, address: &str, suite: &str) -> ExitCode {
    eprintln!(
        "xtask: conformance — running @modelcontextprotocol/conformance@{suite} \
         (spec {SPEC_VERSION}) against http://{address}/mcp"
    );
    let status = Command::new("npx")
        .arg("-y")
        .arg(format!("@modelcontextprotocol/conformance@{suite}"))
        .arg("server")
        .arg("--url")
        .arg(format!("http://{address}/mcp"))
        .arg("--spec-version")
        .arg(SPEC_VERSION)
        .arg("--expected-failures")
        .arg(root.join(EXPECTED_FAILURES))
        .arg("--output-dir")
        .arg(results_dir)
        .current_dir(root)
        .status();
    match status {
        Ok(status) if status.success() => {
            eprintln!(
                "xtask: conformance — green against suite {suite}; \
                 results in {}",
                results_dir.display()
            );
            ExitCode::SUCCESS
        }
        // A non-zero exit with results on disk is the runner's verdict; the
        // same exit with no results means the runner itself never ran
        // (registry fetch failure, bad npx cache, OOM-killed node, …) — two
        // different problems with two different fixes, so say which happened.
        Ok(status) if wrote_any_results(results_dir) => {
            eprintln!(
                "xtask: conformance — runner reported failures ({status}); \
                 inspect {} and either fix the server or record an explained \
                 divergence in {EXPECTED_FAILURES}",
                results_dir.display()
            );
            ExitCode::FAILURE
        }
        Ok(status) => {
            eprintln!(
                "xtask: conformance — runner exited {status} without writing \
                 any results under {} — the suite did not run (npm registry \
                 unreachable? broken npx cache?); this is not a conformance \
                 verdict",
                results_dir.display()
            );
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("xtask: conformance — cannot run npx (Node 18+ required): {error}");
            ExitCode::FAILURE
        }
    }
}

/// Whether the runner produced at least one per-scenario `checks.json`.
fn wrote_any_results(results_dir: &Path) -> bool {
    std::fs::read_dir(results_dir).is_ok_and(|entries| {
        entries
            .filter_map(Result::ok)
            .any(|entry| entry.path().join("checks.json").is_file())
    })
}
