// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The sandbox each notification test runs in.
//!
//! A temporary directory holding stub `gh` and `cargo` executables, put first
//! on `PATH`, so the script's own invocations are recorded rather than
//! performed. `jq` is the real one — it ships on GitHub runners and is doing
//! real work in the script, so stubbing it would test a different program than
//! CI runs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use super::{WORKFLOW, step_script, workspace_root};

/// One executed run of a step: what it wrote, and how the stubs were called.
pub(super) struct Run {
    pub(super) status: i32,
    pub(super) body: String,
    pub(super) gh_calls: Vec<String>,
    pub(super) summary: String,
}

impl Run {
    /// The single `gh` invocation whose first argument pair matches, if any.
    pub(super) fn gh_call(&self, starts_with: &str) -> Option<&str> {
        self.gh_calls
            .iter()
            .find(|call| call.starts_with(starts_with))
            .map(String::as_str)
    }
}

/// A sandbox directory that removes itself, unique to this process and call.
///
/// The name carries the process id and a counter, not just the test's name.
/// `cargo mutants` runs many `cargo test` **processes** at once, and with a
/// name keyed only on the test every one of them shared a directory: each
/// `remove_dir_all` below deleted a sibling's stubs mid-run. The symptom was a
/// mutation gate whose *baseline* failed — four notification tests panicking
/// inside the harness rather than a mutant surviving — which is a confusing
/// place to start reading. Uniqueness per process makes the sandboxes
/// independent for the same reason the writer task assigns `seq`: shared names
/// are shared state.
pub(super) struct Sandbox(pub(super) PathBuf);

/// Distinguishes two sandboxes built by the same test in one process.
static SANDBOXES: AtomicU32 = AtomicU32::new(0);

impl Sandbox {
    pub(super) fn new(name: &str) -> Self {
        let ordinal = SANDBOXES.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mcp-conformance-notification-{name}-{}-{ordinal}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).expect("sandbox");
        Self(dir)
    }

    pub(super) fn write_stub(&self, name: &str, script: &str) {
        let path = self.0.join("bin").join(name);
        fs::write(&path, format!("#!/usr/bin/env bash\n{script}")).expect("stub written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("stub executable");
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) const TITLE: &str = "Weekly claims-expire run is red (ADR-0010)";

/// Runs a step's committed script with the given step outcomes.
///
/// `expired` is what the stubbed `cargo xtask deferrals --expired` prints;
/// `open_issues` is what the stubbed `gh issue list` prints (a JSON array, the
/// shape the real one produces under `--json number,title`).
pub(super) fn run_step(
    name: &str,
    step: &str,
    outcomes: &BTreeMap<&str, &str>,
    expired: Option<&str>,
    open_issues: &str,
) -> Run {
    let sandbox = Sandbox::new(name);
    let calls = sandbox.0.join("gh-calls");

    sandbox.write_stub(
        "gh",
        &format!(
            "printf '%s\\n' \"$*\" >> {calls}\n\
             if [[ \"$1\" == issue && \"$2\" == list ]]; then printf '%s' '{open_issues}'; fi\n\
             exit 0\n",
            calls = calls.display()
        ),
    );
    // `None` makes the stub fail: the script's `|| true` covers it, because an
    // unreadable ledger is a different diagnosis than an expired row and is
    // reported as one.
    sandbox.write_stub(
        "cargo",
        &expired.map_or_else(
            || "exit 1\n".to_owned(),
            |rows| format!("printf '%s' '{rows}'\nexit 0\n"),
        ),
    );

    let workflow =
        fs::read_to_string(workspace_root().join(WORKFLOW)).expect("the workflow exists");
    let script = step_script(&workflow, step)
        .unwrap_or_else(|| panic!("{WORKFLOW} has no step named {step:?}; the tests below exercise that step's script, so a rename must fail here rather than pass over nothing"));
    let script_path = sandbox.0.join("step.sh");
    fs::write(&script_path, &script).expect("script written");

    let path = format!(
        "{}:{}",
        sandbox.0.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new("bash");
    command
        .arg(&script_path)
        .current_dir(&sandbox.0)
        .env("PATH", path)
        .env("ISSUE_TITLE", TITLE)
        .env("RUN_URL", "https://example.invalid/run/1")
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REPOSITORY", "tomtom215/mcp-conformance")
        .env("GH_TOKEN", "stub")
        .env("GH_REPO", "tomtom215/mcp-conformance")
        .env("GITHUB_STEP_SUMMARY", sandbox.0.join("summary.md"));
    // Each gate's step outcome, defaulting to the colour a step that did not
    // fail reports. A gate added to the job without a row here would make every
    // test below run with it green, so the list is the contract.
    for (key, variable) in [
        ("ledger", "LEDGER_OUTCOME"),
        ("drift", "DRIFT_OUTCOME"),
        ("pins", "PINS_OUTCOME"),
        ("toolchain", "TOOLCHAIN_OUTCOME"),
    ] {
        command.env(variable, outcomes.get(key).copied().unwrap_or("success"));
    }
    let output = command.output().expect("bash runs the step");

    Run {
        status: output.status.code().unwrap_or(-1),
        body: fs::read_to_string(sandbox.0.join("issue-body.md")).unwrap_or_default(),
        gh_calls: read_calls(&calls),
        summary: fs::read_to_string(sandbox.0.join("summary.md")).unwrap_or_default(),
    }
}

fn read_calls(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

pub(super) fn outcomes(
    pairs: &[(&'static str, &'static str)],
) -> BTreeMap<&'static str, &'static str> {
    pairs.iter().copied().collect()
}
