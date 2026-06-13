// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Workspace task runner, invoked as `cargo xtask <task>`.
//!
//! Tasks:
//!
//! - `ci` — run the local quality gates in CI order (format, clippy across feature
//!   combinations, tests, docs across feature modes, the file-size cap, cargo-deny
//!   when installed, documentation links, coverage-table sync). The same commands
//!   CONTRIBUTING.md lists, as code, so "run the gates" cannot drift from what CI
//!   runs.
//! - `bless` — regenerate the golden corpus reports (`BLESS=1` golden test run); review
//!   the diff like any other code change.
//! - `coverage` — regenerate the README's requirement-coverage table from the embedded
//!   registry; `coverage --check` verifies it instead (ADR-0001: no hand-kept counts).
//! - `file-sizes` — verify the ≤ 500-line cap on source and registry files.
//! - `deny` — run `cargo deny --all-features check`, skipping loudly when
//!   cargo-deny is not installed.
//! - `docs-links` — verify every relative link in tracked Markdown resolves
//!   (`docs_links.rs`).
//! - `deferrals [--check]` — list the deferral ledger (docs/plan/deferrals.json);
//!   `--check` (weekly scheduled job) fails on rows past their review-by date
//!   (ADR-0010).
//! - `spec-drift` — verify every registry quote against the published spec
//!   text (network; weekly scheduled job — ADR-0010).
//! - `mutants` — the exact diff-scoped mutation gate CI runs on PRs, against
//!   `origin/main`.
//! - `semver` — `cargo semver-checks check-release` against the published
//!   crates.io baseline (network; release-readiness gate, run before tagging):
//!   an API-breaking change shipped under a version bump that does not admit one
//!   fails here, so declared behavioral breaks are never confused with
//!   undeclared API breaks.
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
mod deferrals;
mod docs_links;
mod local_gates;
mod spec_drift;

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
        Some("ci") => run_ci(),
        Some("bless") => exit_if(run_all(&bless_steps())),
        Some("coverage") => coverage::run(args.next().as_deref() == Some("--check")),
        Some("file-sizes") => exit_if(local_gates::file_size_gate()),
        Some("deny") => exit_if(local_gates::deny_gate()),
        Some("mutants") => exit_if(local_gates::mutants_gate()),
        Some("semver") => exit_if(local_gates::semver_gate()),
        Some("deferrals") => exit_if(deferrals::run(args.next().as_deref() == Some("--check"))),
        Some("spec-drift") => spec_drift::run(),
        Some("docs-links") => exit_if(docs_links::run()),
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

/// `ExitCode::SUCCESS` when `ok`, else `ExitCode::FAILURE` — the shape every
/// boolean gate task collapses to.
const fn exit_if(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The `ci` task: every local gate, in CI order.
fn run_ci() -> ExitCode {
    if !run_all(&ci_steps()) {
        return ExitCode::FAILURE;
    }
    if !local_gates::msrv_clippy_gate() {
        return ExitCode::FAILURE;
    }
    if !local_gates::file_size_gate() {
        return ExitCode::FAILURE;
    }
    if !local_gates::deny_gate() {
        return ExitCode::FAILURE;
    }
    if !docs_links::run() {
        return ExitCode::FAILURE;
    }
    eprintln!("xtask: coverage table in sync — cargo xtask coverage --check");
    coverage::run(true)
}

const USAGE: &str = "usage: cargo xtask <task>\n\ntasks:\n  ci                 run all local quality gates\n  bless              regenerate golden corpus reports\n  coverage [--check] regenerate (or verify) the README coverage table\n  file-sizes         verify the 500-line cap on source and registry files\n  deny               run cargo deny check (loud skip when cargo-deny is absent)\n  mutants            diff-scoped mutation gate vs origin/main (the PR gate, locally)\n  semver             cargo-semver-checks vs the crates.io baseline (release-readiness)\n  deferrals [--check] list the deferral ledger; --check fails on expired rows\n  spec-drift         verify registry quotes against the published spec (network)\n  docs-links         verify every relative link in tracked Markdown resolves\n  conformance        run the pinned official suite against the everything server,\n                     then the agreement and coverage-manifest checks (BLESS=1 to\n                     regenerate the manifest)";

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
