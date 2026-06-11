// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Workspace task runner, invoked as `cargo xtask <task>`.
//!
//! Tasks:
//!
//! - `ci` — run the local quality gates in CI order (format, clippy across feature
//!   combinations, tests, docs). The same commands CONTRIBUTING.md lists, as code, so
//!   "run the gates" cannot drift from what CI runs.
//! - `bless` — regenerate the golden corpus reports (`BLESS=1` golden test run); review
//!   the diff like any other code change.
//! - `coverage` — regenerate the README's requirement-coverage table from the embedded
//!   registry; `coverage --check` verifies it instead (ADR-0001: no hand-kept counts).
//!
//! - `conformance` — spawn the everything-server over streamable HTTP (session tap on)
//!   and drive the pinned official runner against it, then reconcile the runner's
//!   verdicts with our validator's over the tapped sessions (`agreement.rs`) and check
//!   the coverage manifest. `conformance.rs` documents the network-use boundary:
//!   orchestration may dial out, `cargo test` never does.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

mod agreement;
mod conformance;
mod coverage;

/// The workspace root: the parent of this crate's manifest directory.
///
/// Tasks anchor every path here so they behave identically from any working
/// directory — `cargo xtask` inherits the invoker's cwd, which need not be
/// the root. The compile-time manifest path is correct for a dev-only task
/// that is always built and run in-tree.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let task = args.next();
    match task.as_deref() {
        Some("ci") => {
            if !run_all(&ci_steps()) {
                return ExitCode::FAILURE;
            }
            eprintln!("xtask: coverage table in sync — cargo xtask coverage --check");
            coverage::run(true)
        }
        Some("bless") => {
            if run_all(&bless_steps()) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Some("coverage") => coverage::run(args.next().as_deref() == Some("--check")),
        Some("conformance") => conformance::run(),
        Some(other) => {
            eprintln!("unknown task {other:?}\n{USAGE}");
            ExitCode::FAILURE
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: cargo xtask <task>\n\ntasks:\n  ci                 run all local quality gates\n  bless              regenerate golden corpus reports\n  coverage [--check] regenerate (or verify) the README coverage table\n  conformance        run the pinned official suite against the everything server,\n                     then the agreement and coverage-manifest checks (BLESS=1 to\n                     regenerate the manifest)";

/// One gate: a display name plus the cargo arguments to run.
struct Step {
    name: String,
    args: Vec<&'static str>,
    env: &'static [(&'static str, &'static str)],
}

/// The feature combinations every lint/test gate runs across.
const FEATURE_MODES: [(&str, &[&str]); 3] = [
    ("default features", &[]),
    ("no default features", &["--no-default-features"]),
    ("all features", &["--all-features"]),
];

fn ci_steps() -> Vec<Step> {
    let mut steps = vec![Step {
        name: "format".to_owned(),
        args: vec!["fmt", "--all", "--", "--check"],
        env: &[],
    }];
    for (mode, flags) in FEATURE_MODES {
        let mut args = vec!["clippy", "--workspace", "--all-targets"];
        args.extend_from_slice(flags);
        args.extend_from_slice(&["--", "-D", "warnings"]);
        steps.push(Step {
            name: format!("clippy ({mode})"),
            args,
            env: &[],
        });
    }
    for (mode, flags) in FEATURE_MODES {
        let mut args = vec!["test", "--workspace"];
        args.extend_from_slice(flags);
        steps.push(Step {
            name: format!("test ({mode})"),
            args,
            env: &[],
        });
    }
    steps.push(Step {
        name: "docs (deny warnings)".to_owned(),
        args: vec!["doc", "--workspace", "--no-deps"],
        env: &[("RUSTDOCFLAGS", "-D warnings")],
    });
    steps
}

fn bless_steps() -> Vec<Step> {
    vec![Step {
        name: "bless golden corpus".to_owned(),
        args: vec!["test", "-p", "mcp-trace-validator", "--test", "golden"],
        env: &[("BLESS", "1")],
    }]
}

/// Runs the steps in order; `true` when every one succeeded.
fn run_all(steps: &[Step]) -> bool {
    for step in steps {
        eprintln!("xtask: {} — cargo {}", step.name, step.args.join(" "));
        let mut command = Command::new("cargo");
        command.args(&step.args);
        for (key, value) in step.env {
            command.env(key, value);
        }
        match command.status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("xtask: {} failed with {status}", step.name);
                return false;
            }
            Err(error) => {
                eprintln!("xtask: cannot run cargo: {error}");
                return false;
            }
        }
    }
    eprintln!("xtask: all steps passed");
    true
}
