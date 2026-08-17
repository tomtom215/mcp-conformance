// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! `cargo xtask draft-capture` — record a `2026-07-28` stdio session and judge
//! it.
//!
//! The `2026-07-28` HTTP captures come from `draft-readiness`, where the
//! official runner supplies the client. stdio has no such runner — the suite
//! drives servers over `--url` only — so the client here is this workspace's
//! own `mcp-reference-host`, speaking rmcp's stateless lifecycle
//! (`server/discover`, then a `_meta` envelope per request, and MRTR rounds
//! driven by rmcp).
//!
//! **That is a weaker provenance than the HTTP captures**, and the corpus
//! ledger says so where the trace is recorded rather than only here: both ends
//! of this session are ours. What it still supplies that an authored fixture
//! cannot is that neither end was written to satisfy the checks, and that
//! every byte was produced by the same rmcp machinery a third-party client
//! would use — the lifecycle, the envelope, and the MRTR retry loop are rmcp's
//! code, not ours.
//!
//! The flags below are the capture's definition, not an operator's taste.
//! Sweeping every tool means meeting `test_error_handling`, whose whole job is
//! to return an error result, so the run needs a budget the suite's scenarios
//! deliberately do not have; and `--subscribe` is there because
//! `subscriptions/listen` is a long-lived request rather than a tool, so no
//! sweep of the tool list would ever reach it.
//!
//! Like `conformance` and `draft-readiness` this is orchestration — it spawns
//! processes and speaks a real transport, which `cargo test` never does.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use mcp_conformance_core::requirement::RegistrySet;
use mcp_trace_validator::report::Outcome;

/// The revision captured.
const REVISION: &str = "2026-07-28";

/// Where the run's artifacts land, kept away from `target/conformance/` and
/// `target/draft-readiness/` so no run can be mistaken for another's.
const RESULTS_DIR: &str = "target/draft-capture";

/// The committed copy, relative to the workspace root.
const COMMITTED: &str = "corpus/draft/captured/reference-host-2026-07-28-stdio.jsonl";

/// How many error *results* the sweep tolerates.
///
/// `test_error_handling` returns one by design, and a capture that stopped
/// there would omit every tool after it alphabetically — including
/// `test_sampling`, the one that exercises an MRTR sampling round. Four is
/// slack for that one plus room to notice if the number grows.
const ERROR_BUDGET: &str = "4";

/// Turn cap, above the tool count so the sweep is not silently truncated.
const TURN_LIMIT: &str = "32";

pub(crate) fn run(bless: bool) -> ExitCode {
    let root = crate::workspace_root();
    let results = root.join(RESULTS_DIR);
    if let Err(message) = prepare(&root, &results) {
        eprintln!("xtask: draft-capture — {message}");
        return ExitCode::FAILURE;
    }
    let trace = match record(&root, &results) {
        Ok(trace) => trace,
        Err(message) => {
            eprintln!("xtask: draft-capture — {message}");
            return ExitCode::FAILURE;
        }
    };
    match judge(&trace) {
        Err(message) => {
            eprintln!("xtask: draft-capture — {message}");
            ExitCode::FAILURE
        }
        Ok(()) if bless => match std::fs::copy(&trace, root.join(COMMITTED)) {
            Ok(_) => {
                eprintln!(
                    "xtask: draft-capture — committed copy refreshed ({COMMITTED}); \
                     re-bless the goldens with `cargo xtask bless`"
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("xtask: draft-capture — cannot update {COMMITTED}: {error}");
                ExitCode::FAILURE
            }
        },
        Ok(()) => {
            eprintln!(
                "xtask: draft-capture — recording at {} judges clean; BLESS=1 to replace \
                 the committed copy",
                trace.display()
            );
            ExitCode::SUCCESS
        }
    }
}

/// Builds the two binaries and clears the previous run's artifacts.
fn prepare(root: &Path, results: &Path) -> Result<(), String> {
    if let Err(error) = std::fs::remove_dir_all(results)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(format!("cannot clear {}: {error}", results.display()));
    }
    eprintln!("xtask: draft-capture — building the server and the host");
    let built = Command::new("cargo")
        .args([
            "build",
            "-p",
            "mcp-everything-server",
            "-p",
            "mcp-reference-host",
            "--all-features",
        ])
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run cargo build: {error}"))?;
    if !built.success() {
        return Err(format!("cargo build failed with {built}"));
    }
    Ok(())
}

/// Drives one session and returns the trace it wrote.
fn record(root: &Path, results: &Path) -> Result<PathBuf, String> {
    let binary = |name: &str| {
        root.join(format!(
            "target/debug/{name}{}",
            std::env::consts::EXE_SUFFIX
        ))
        .display()
        .to_string()
    };
    let server = format!(
        "{} --transport stdio --protocol-version {REVISION}",
        binary("mcp-everything-server")
    );
    eprintln!("xtask: draft-capture — {REVISION} host against {server}");
    let status = Command::new(binary("mcp-reference-host"))
        .args(["--server-cmd", &server])
        .args(["--protocol-version", REVISION])
        .args(["--error-budget", ERROR_BUDGET])
        .args(["--turn-limit", TURN_LIMIT])
        // `subscriptions/listen` is the one `2026-07-28` feature no tool call
        // reaches: it is a long-lived request, not a tool, so a sweep of the
        // tool list would record everything about this server except the
        // mechanism the revision introduced to replace `resources/subscribe`.
        .arg("--subscribe")
        .arg("--trace-dir")
        .arg(results)
        .current_dir(root)
        .status()
        .map_err(|error| format!("cannot run the reference host: {error}"))?;
    if !status.success() {
        return Err(format!("the host exited {status}; no capture taken"));
    }
    // One run, one file; naming it here would duplicate the host's own
    // convention (scenario + pid), which is the thing that keeps concurrent
    // runs from colliding.
    let mut traces: Vec<PathBuf> = std::fs::read_dir(results)
        .map_err(|error| format!("cannot read {}: {error}", results.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    traces.sort();
    traces
        .pop()
        .ok_or_else(|| format!("the host wrote no trace into {}", results.display()))
}

/// Fails unless every judged clause of the revision passes.
///
/// A *capture* normally asserts no verdict — a real implementation is whatever
/// it is, and `corpus/draft/captured/` pins reports rather than outcomes. This
/// one is different because both ends are this workspace's: the recording is
/// evidence about our own server, so a finding in it is a defect to fix rather
/// than news about somebody else's code.
fn judge(trace: &Path) -> Result<(), String> {
    let document = std::fs::read_to_string(trace)
        .map_err(|error| format!("cannot read {}: {error}", trace.display()))?;
    let events = mcp_trace_validator::reader::parse_trace(
        &document,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .map_err(|error| format!("{} is malformed: {error}", trace.display()))?;
    let set = RegistrySet::builtin().map_err(|error| format!("registry set: {error}"))?;
    let revision = REVISION
        .parse()
        .map_err(|_| format!("{REVISION} is not a protocol revision"))?;
    let registry = set.registry(revision).ok_or_else(|| {
        format!("this build does not describe {REVISION}; enable `draft-2026-07-28`")
    })?;
    let report = mcp_trace_validator::engine::validate(&registry, &events);
    let failed: Vec<&str> = report
        .requirements
        .iter()
        .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
        .map(|row| row.id.as_str())
        .collect();
    let counts = report.totals;
    eprintln!(
        "xtask: draft-capture — {} pass, {} fail, {} warn, {} excluded",
        counts.pass, counts.fail, counts.warn, counts.excluded
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the capture is not clean: {}. Both ends of this session are ours, so a \
             finding here is a defect in this workspace, not news about another \
             implementation.",
            failed.join(", ")
        ))
    }
}
