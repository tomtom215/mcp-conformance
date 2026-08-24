// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The notification steps, executed branch by branch.
//!
//! The sandbox they run in is [`super::harness`]; what they assert is that the
//! shell `scheduled.yml` ships does the right thing with a `gh` that works, and
//! fails loudly with one that does not.
//!
//! **The executing tests are `cfg(unix)`.** The step under test is a `run:`
//! block on an `ubuntu-latest` job, so a POSIX shell is not an implementation
//! detail of the test — it is the environment the script actually has. Running
//! it on a Windows runner exercises Git Bash over native paths instead, and the
//! harness cannot even reach it there: `PATH` is joined with `:`, which is not
//! the Windows separator, so the stub directory never goes on `PATH` and the
//! script reaches the runner's real `gh` and `cargo` instead. Four of the eight
//! failed that way; the other four were running against those real binaries,
//! which is not what any of them mean to assert.
//!
//! What stays platform-independent is everything that reads the workflow rather
//! than running it: the extractor, the committed-script check, and the dispatch
//! guard. Those are the ones a Windows contributor can still break.

use std::fs;
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
use super::harness::{Sandbox, TITLE, outcomes, run_step};
use super::{CLOSE_STEP, OPEN_STEP, WORKFLOW, step_script, workspace_root};

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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

/// Every gate this job runs, and the section its failure must add.
///
/// The list is the contract: a gate added to `scheduled.yml` without a row here
/// is a gate whose failure the issue would not explain, which is the whole
/// reason the issue names one at all — an un-re-decided deferral, an upstream
/// suite release, a Rust release and a transient fetch failure are identical
/// from outside the run.
#[cfg(unix)]
const GATES: [(&str, &str); 4] = [
    ("ledger", "### Expired ledger rows"),
    ("drift", "### Spec-quote drift"),
    ("pins", "### Suite pins"),
    ("toolchain", "### Toolchain pin"),
];

#[cfg(unix)]
#[test]
fn each_gate_contributes_its_own_section_and_no_others() {
    for (key, section) in GATES {
        let run = run_step(
            key,
            OPEN_STEP,
            &outcomes(&[(key, "failure")]),
            Some("a-row 2026-01-01\n"),
            "[]",
        );
        assert!(
            run.body.contains(section),
            "{section} missing:\n{}",
            run.body
        );
        for (_, other) in GATES.iter().filter(|(other, _)| *other != key) {
            assert!(
                !run.body.contains(other),
                "a red `{key}` produced {other}:\n{}",
                run.body
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn every_gate_red_at_once_names_all_of_them() {
    let all = run_step(
        "all",
        OPEN_STEP,
        &outcomes(&GATES.map(|(key, _)| (key, "failure"))),
        Some("a-row 2026-01-01\n"),
        "[]",
    );
    for (_, section) in GATES {
        assert!(
            all.body.contains(section),
            "{section} missing:\n{}",
            all.body
        );
    }
}

#[cfg(unix)]
#[test]
fn the_drift_section_does_not_republish_what_it_fetched() {
    let drift = run_step(
        "drift-text",
        OPEN_STEP,
        &outcomes(&[("drift", "failure")]),
        Some(""),
        "[]",
    );
    assert!(
        drift.body.contains("deliberately not republished"),
        "{}",
        drift.body
    );
}

#[cfg(unix)]
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

#[cfg(unix)]
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

#[cfg(unix)]
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
        .env("TOOLCHAIN_OUTCOME", "success")
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

/// The guard every job but `claims-expire` must carry, so a `claims`-scoped
/// dispatch runs the notification and nothing else.
const SCOPE_GUARD: &str = "if: github.event_name != 'workflow_dispatch' || inputs.only == 'all'";

#[test]
fn a_claims_scoped_dispatch_runs_the_notification_job_and_no_other() {
    // The input exists to make the live `gh` behaviour cheap to establish. A
    // job added later without the guard would quietly put the mutation sweep
    // (or the cross-architecture matrix, or the benchmarks) back into that
    // dispatch, so the invariant is asserted rather than remembered.
    let workflow =
        fs::read_to_string(workspace_root().join(WORKFLOW)).expect("the workflow exists");

    let mut unguarded = Vec::new();
    let mut guarded = 0_usize;
    let lines: Vec<&str> = workflow.lines().collect();
    let jobs_at = lines
        .iter()
        .position(|line| *line == "jobs:")
        .expect("the workflow declares jobs");
    for (index, line) in lines.iter().enumerate().skip(jobs_at + 1) {
        // A job header is exactly two spaces of indent, an identifier, and a
        // trailing colon — not a two-space comment that happens to end in one,
        // which this workflow has several of.
        let Some(name) = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with([' ', '#']) && rest.ends_with(':'))
            .map(|rest| rest.trim_end_matches(':'))
            .filter(|name| {
                !name.is_empty()
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            })
        else {
            continue;
        };
        // Its own keys are the lines until the next job header.
        let body: String = lines[index + 1..]
            .iter()
            .take_while(|line| line.starts_with("    ") || line.trim().is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("\n");
        if body.contains(SCOPE_GUARD) {
            guarded += 1;
        } else {
            unguarded.push(name);
        }
    }

    assert!(
        guarded >= 8,
        "the guard was found on {guarded} job(s); the parser is probably wrong"
    );
    assert_eq!(
        unguarded,
        ["claims-expire"],
        "every job but claims-expire must carry `{SCOPE_GUARD}`, so a `claims` dispatch \
         stays cheap; these do not"
    );
}

#[cfg(unix)]
#[test]
fn two_sandboxes_never_share_a_directory() {
    // `cargo mutants` runs many `cargo test` processes at once. A sandbox name
    // keyed only on the test made every one of them share a directory, and each
    // `remove_dir_all` deleted a sibling's stubs mid-run — a mutation gate whose
    // *baseline* failed inside this harness. Uniqueness within a process is what
    // can be asserted here; the process id in the name carries the rest, and
    // both are named in `harness::Sandbox`.
    let first = Sandbox::new("same-name");
    let second = Sandbox::new("same-name");
    assert_ne!(first.0, second.0);
    assert!(first.0.is_dir() && second.0.is_dir());
    let name = first.0.file_name().unwrap_or_default().to_string_lossy();
    assert!(
        name.contains(&std::process::id().to_string()),
        "the process id distinguishes concurrent test processes: {name}"
    );
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
