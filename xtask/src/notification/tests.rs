// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The notification steps run under stubbed `gh` and `cargo`.
//!
//! Each test builds a sandbox: a temporary directory holding the stubs, put
//! first on `PATH`, so the script's own `gh` and `cargo xtask deferrals`
//! invocations are recorded rather than performed. `jq` is the real one — it
//! ships on GitHub runners and is doing real work in the script, so stubbing it
//! would test a different program than CI runs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{CLOSE_STEP, OPEN_STEP, WORKFLOW, step_script, workspace_root};

/// One executed run of a step: what it wrote, and how the stubs were called.
struct Run {
    status: i32,
    body: String,
    gh_calls: Vec<String>,
    summary: String,
}

impl Run {
    /// The single `gh` invocation whose first argument pair matches, if any.
    fn gh_call(&self, starts_with: &str) -> Option<&str> {
        self.gh_calls
            .iter()
            .find(|call| call.starts_with(starts_with))
            .map(String::as_str)
    }
}

/// A sandbox directory that removes itself. `std::env::temp_dir` plus the test
/// name keeps runs independent without a dependency on a temp-dir crate.
struct Sandbox(PathBuf);

impl Sandbox {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("mcp-conformance-notification-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("bin")).expect("sandbox");
        Self(dir)
    }

    fn write_stub(&self, name: &str, script: &str) {
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

const TITLE: &str = "Weekly claims-expire run is red (ADR-0010)";

/// Runs a step's committed script with the given step outcomes.
///
/// `expired` is what the stubbed `cargo xtask deferrals --expired` prints;
/// `open_issues` is what the stubbed `gh issue list` prints (a JSON array, the
/// shape the real one produces under `--json number,title`).
fn run_step(
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
    let output = Command::new("bash")
        .arg(&script_path)
        .current_dir(&sandbox.0)
        .env("PATH", path)
        .env("ISSUE_TITLE", TITLE)
        .env("RUN_URL", "https://example.invalid/run/1")
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REPOSITORY", "tomtom215/mcp-conformance")
        .env("GH_TOKEN", "stub")
        .env("GH_REPO", "tomtom215/mcp-conformance")
        .env("GITHUB_STEP_SUMMARY", sandbox.0.join("summary.md"))
        .env(
            "LEDGER_OUTCOME",
            outcomes.get("ledger").copied().unwrap_or("success"),
        )
        .env(
            "DRIFT_OUTCOME",
            outcomes.get("drift").copied().unwrap_or("success"),
        )
        .env(
            "PINS_OUTCOME",
            outcomes.get("pins").copied().unwrap_or("success"),
        )
        .output()
        .expect("bash runs the step");

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

fn outcomes(pairs: &[(&'static str, &'static str)]) -> BTreeMap<&'static str, &'static str> {
    pairs.iter().copied().collect()
}

#[test]
fn an_expired_row_opens_an_issue_naming_it() {
    let run = run_step(
        "expired",
        OPEN_STEP,
        &outcomes(&[("ledger", "failure")]),
        Some("rust-sdk-902-offer-clock 2026-10-16\nsuite-0-2-0-stable-pin-bump 2026-10-15\n"),
        "[]",
    );
    assert_eq!(
        run.status,
        0,
        "the step must not fail: {run:?}",
        run = run.body
    );

    // Every expired row reaches the body, id and date both.
    assert!(
        run.body
            .contains("`rust-sdk-902-offer-clock` — review by 2026-10-16"),
        "{}",
        run.body
    );
    assert!(
        run.body
            .contains("`suite-0-2-0-stable-pin-bump` — review by 2026-10-15"),
        "{}",
        run.body
    );
    // The outcome table distinguishes the three gates.
    assert!(
        run.body
            .contains("| Deferral ledger within its review dates | `failure` |"),
        "{}",
        run.body
    );
    assert!(
        run.body
            .contains("| Registry quotes match the published spec text | `success` |"),
        "{}",
        run.body
    );
    assert!(
        run.body
            .contains("| Both suite pins still equal the dist-tags they track | `success` |"),
        "{}",
        run.body
    );
    assert!(
        run.body.contains("https://example.invalid/run/1"),
        "{}",
        run.body
    );
    // Backticks survive: the script uses printf rather than a heredoc for this.
    assert!(run.body.contains("`claims-expire`"), "{}", run.body);

    // With no matching open issue, one is filed.
    let create = run.gh_call("issue create").expect("an issue is created");
    assert!(create.contains(TITLE), "{create}");
    assert!(create.contains("--body-file issue-body.md"), "{create}");
    assert!(run.gh_call("issue comment").is_none(), "{:?}", run.gh_calls);

    // The run summary states the outcome, so a notification that did not
    // happen is visible on a page someone already has open.
    assert!(
        run.summary.contains("claims-expire notification"),
        "{}",
        run.summary
    );
    assert!(run.summary.contains("**opened**"), "{}", run.summary);
}

#[test]
fn a_run_that_is_still_red_comments_rather_than_filing_again() {
    let run = run_step(
        "still-red",
        OPEN_STEP,
        &outcomes(&[("ledger", "failure")]),
        Some("rust-sdk-902-offer-clock 2026-10-16\n"),
        &format!(r#"[{{"number":41,"title":"{TITLE}"}}]"#),
    );
    assert_eq!(run.status, 0);
    let comment = run
        .gh_call("issue comment")
        .expect("the open issue is commented on");
    assert!(comment.contains("41"), "{comment}");
    assert!(run.gh_call("issue create").is_none(), "{:?}", run.gh_calls);
    assert!(run.summary.contains("commented on #41"), "{}", run.summary);
}

#[test]
fn an_unrelated_open_issue_does_not_absorb_the_notification() {
    // The de-duplication key is the exact title. A repository with other open
    // issues must still get its own.
    let run = run_step(
        "unrelated",
        OPEN_STEP,
        &outcomes(&[("ledger", "failure")]),
        Some("rust-sdk-902-offer-clock 2026-10-16\n"),
        r#"[{"number":7,"title":"Something else entirely"},{"number":9,"title":"Weekly claims-expire run is red"}]"#,
    );
    assert_eq!(run.status, 0);
    assert!(run.gh_call("issue create").is_some(), "{:?}", run.gh_calls);
    assert!(run.gh_call("issue comment").is_none(), "{:?}", run.gh_calls);
}

#[test]
fn a_red_ledger_with_no_expired_row_says_the_ledger_itself_is_the_problem() {
    // The other way the ledger gate goes red: the file does not parse, or
    // breaks a shape rule. The body must not imply a row expired.
    let run = run_step(
        "unreadable",
        OPEN_STEP,
        &outcomes(&[("ledger", "failure")]),
        None,
        "[]",
    );
    assert_eq!(
        run.status, 0,
        "the `|| true` keeps a failing query from failing the step"
    );
    assert!(run.body.contains("did not parse"), "{}", run.body);
    assert!(!run.body.contains("review by"), "{}", run.body);
}

#[test]
fn each_gate_contributes_its_own_section_and_no_others() {
    let drift = run_step(
        "drift",
        OPEN_STEP,
        &outcomes(&[("drift", "failure")]),
        Some(""),
        "[]",
    );
    assert!(
        drift.body.contains("### Spec-quote drift"),
        "{}",
        drift.body
    );
    assert!(
        !drift.body.contains("### Expired ledger rows"),
        "{}",
        drift.body
    );
    assert!(!drift.body.contains("### Suite pins"), "{}", drift.body);
    // The fetched specification text is deliberately not republished.
    assert!(
        drift.body.contains("deliberately not republished"),
        "{}",
        drift.body
    );

    let pins = run_step(
        "pins",
        OPEN_STEP,
        &outcomes(&[("pins", "failure")]),
        Some(""),
        "[]",
    );
    assert!(pins.body.contains("### Suite pins"), "{}", pins.body);
    assert!(!pins.body.contains("### Spec-quote drift"), "{}", pins.body);

    let all = run_step(
        "all",
        OPEN_STEP,
        &outcomes(&[
            ("ledger", "failure"),
            ("drift", "failure"),
            ("pins", "failure"),
        ]),
        Some("a-row 2026-01-01\n"),
        "[]",
    );
    for section in [
        "### Expired ledger rows",
        "### Spec-quote drift",
        "### Suite pins",
    ] {
        assert!(
            all.body.contains(section),
            "{section} missing from {}",
            all.body
        );
    }
}

#[test]
fn a_green_run_closes_the_open_issue_and_a_quiet_one_does_nothing() {
    let closed = run_step(
        "close",
        CLOSE_STEP,
        &outcomes(&[]),
        Some(""),
        &format!(r#"[{{"number":41,"title":"{TITLE}"}}]"#),
    );
    assert_eq!(closed.status, 0);
    let close = closed.gh_call("issue close").expect("the issue is closed");
    assert!(close.contains("41"), "{close}");
    assert!(close.contains("https://example.invalid/run/1"), "{close}");

    assert!(closed.summary.contains("#41 closed"), "{}", closed.summary);

    let nothing = run_step("close-noop", CLOSE_STEP, &outcomes(&[]), Some(""), "[]");
    assert_eq!(nothing.status, 0);
    assert!(
        nothing.gh_call("issue close").is_none(),
        "{:?}",
        nothing.gh_calls
    );
    assert!(nothing.summary.is_empty(), "{}", nothing.summary);
}

#[test]
fn a_broken_gh_fails_the_step_rather_than_skipping_the_notification() {
    // The failure this whole module exists for: these steps run inside a job
    // that is already red, so a `gh` that does not work must at least make its
    // own step fail. `set -euo pipefail` is what guarantees it, and nothing
    // else asserted that it was there.
    let sandbox = Sandbox::new("broken-gh");
    sandbox.write_stub("gh", "echo 'unknown flag' >&2\nexit 1\n");
    sandbox.write_stub("cargo", "exit 0\n");
    let workflow =
        fs::read_to_string(workspace_root().join(WORKFLOW)).expect("the workflow exists");
    let script = step_script(&workflow, CLOSE_STEP).expect("the close step exists");
    let script_path = sandbox.0.join("step.sh");
    fs::write(&script_path, script).expect("script written");

    let path = format!(
        "{}:{}",
        sandbox.0.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("bash")
        .arg(&script_path)
        .current_dir(&sandbox.0)
        .env("PATH", path)
        .env("ISSUE_TITLE", TITLE)
        .env("RUN_URL", "https://example.invalid/run/1")
        .env("GH_TOKEN", "stub")
        .output()
        .expect("bash runs the step");
    assert!(
        !output.status.success(),
        "a failing gh must fail the step, not be swallowed by the pipeline"
    );
}

#[test]
fn an_unset_step_summary_does_not_trip_set_u() {
    // The summary variable exists only inside Actions. `set -u` turns a bare
    // reference to it into a step failure everywhere else — including any
    // future local rehearsal of this script — so it is defaulted.
    let sandbox = Sandbox::new("no-summary");
    sandbox.write_stub("gh", "printf '%s' '[]'\nexit 0\n");
    sandbox.write_stub("cargo", "exit 0\n");
    let workflow =
        fs::read_to_string(workspace_root().join(WORKFLOW)).expect("the workflow exists");
    let script = step_script(&workflow, OPEN_STEP).expect("the open step exists");
    let script_path = sandbox.0.join("step.sh");
    fs::write(&script_path, script).expect("script written");

    let path = format!(
        "{}:{}",
        sandbox.0.join("bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("bash")
        .arg(&script_path)
        .current_dir(&sandbox.0)
        .env("PATH", path)
        .env("ISSUE_TITLE", TITLE)
        .env("RUN_URL", "https://example.invalid/run/1")
        .env("GITHUB_SERVER_URL", "https://github.com")
        .env("GITHUB_REPOSITORY", "tomtom215/mcp-conformance")
        .env("GH_TOKEN", "stub")
        .env("LEDGER_OUTCOME", "success")
        .env("DRIFT_OUTCOME", "failure")
        .env("PINS_OUTCOME", "success")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .expect("bash runs the step");
    assert!(
        output.status.success(),
        "unset GITHUB_STEP_SUMMARY must not fail the step: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_script_under_test_is_the_committed_one() {
    // The extractor is the load-bearing part of this module: if it silently
    // returned an empty script every test above would pass over nothing.
    let workflow =
        fs::read_to_string(workspace_root().join(WORKFLOW)).expect("the workflow exists");
    for step in [OPEN_STEP, CLOSE_STEP] {
        let script = step_script(&workflow, step).unwrap_or_else(|| panic!("{step} exists"));
        assert!(script.starts_with("set -euo pipefail"), "{step}: {script}");
        assert!(script.contains("gh issue list"), "{step}: {script}");
        assert!(script.lines().count() > 3, "{step}: {script}");
    }
    assert!(step_script(&workflow, "No such step").is_none());
}

#[test]
fn the_extractor_stops_at_the_end_of_the_block() {
    let workflow = "\
jobs:
  x:
    steps:
      - name: First
        run: |
          echo one
          echo two
      - name: Second
        run: |
          echo three
";
    assert_eq!(
        step_script(workflow, "First").as_deref(),
        Some("echo one\necho two\n")
    );
    assert_eq!(
        step_script(workflow, "Second").as_deref(),
        Some("echo three\n")
    );
}
