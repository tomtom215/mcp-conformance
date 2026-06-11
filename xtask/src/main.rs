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
            if !file_size_gate() {
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
        Some("file-sizes") => {
            if file_size_gate() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
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

const USAGE: &str = "usage: cargo xtask <task>\n\ntasks:\n  ci                 run all local quality gates\n  bless              regenerate golden corpus reports\n  coverage [--check] regenerate (or verify) the README coverage table\n  file-sizes         verify the 500-line cap on source and registry files\n  conformance        run the pinned official suite against the everything server,\n                     then the agreement and coverage-manifest checks (BLESS=1 to\n                     regenerate the manifest)";

/// The ≤ 500-line cap from 04-engineering-standards §Source standards,
/// enforced over non-test source (crate and xtask `src/` trees) and the
/// embedded registry documents (whose loader promises per-file
/// reviewability). Integration tests and benches live outside `src/` and
/// are exempt by construction.
fn file_size_gate() -> bool {
    const CAP: usize = 500;
    let root = workspace_root();
    let mut roots: Vec<PathBuf> = vec![root.join("xtask/src")];
    if let Ok(crates) = std::fs::read_dir(root.join("crates")) {
        for krate in crates.filter_map(Result::ok) {
            roots.push(krate.path().join("src"));
            roots.push(krate.path().join("registry"));
        }
    }
    let mut offenders = Vec::new();
    while let Some(dir) = roots.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                roots.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "rs" || ext == "json")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                let lines = text.lines().count();
                if lines > CAP {
                    offenders.push((path, lines));
                }
            }
        }
    }
    if offenders.is_empty() {
        eprintln!("xtask: file sizes — every source and registry file is within {CAP} lines");
        true
    } else {
        for (path, lines) in &offenders {
            eprintln!(
                "xtask: file sizes — {} is {lines} lines (cap {CAP}); split it at a \
                 reviewable seam",
                path.display()
            );
        }
        false
    }
}

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
    // Docs build twice: default features, then all features. Feature-gated
    // modules (the everything-server's `tap`) carry their own rustdoc, and a
    // broken intra-doc link there is invisible to a default-feature doc build
    // — the gap that let a private-item link survive until the 2026-06-11
    // audit. `-D warnings` makes either build fail on the first warning.
    steps.push(Step {
        name: "docs (default features, deny warnings)".to_owned(),
        args: vec!["doc", "--workspace", "--no-deps"],
        env: &[("RUSTDOCFLAGS", "-D warnings")],
    });
    steps.push(Step {
        name: "docs (all features, deny warnings)".to_owned(),
        args: vec!["doc", "--workspace", "--no-deps", "--all-features"],
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
