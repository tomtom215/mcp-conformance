// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The diff-scoped mutation gate: the exact check CI runs on pull requests,
//! reproduced locally against `origin/main`.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// `cargo xtask mutants [--jobs N]` — the exact diff-scoped mutation gate CI
/// runs on PRs, computed against `origin/main`, so "run the gates" includes
/// the one that catches untested code. Not part of `ci` (minutes, not
/// seconds); offered as its own task.
///
/// `--jobs` is the only argument accepted, and the reason it is the only one
/// is that it changes how *fast* a fixed set of mutants is judged and nothing
/// else: not which mutants exist, not which tests run, not how a verdict is
/// reached. Every other cargo-mutants flag would make this task something
/// other than the gate, which is the whole point of the task existing. The
/// default is cargo-mutants' own — one job — because parallel test runs
/// compete for the machine, and a mutant that is merely *slow* under
/// contention is reported as a timeout rather than as caught. `.cargo/
/// mutants.toml` buys headroom for that (3× multiplier, 30s floor); raising
/// `--jobs` spends it.
pub(crate) fn mutants_gate(extra: &[String]) -> bool {
    let jobs = match jobs(extra) {
        Ok(jobs) => jobs,
        Err(error) => {
            eprintln!("xtask: mutants — {error}");
            return false;
        }
    };
    let root = crate::workspace_root();
    let available = Command::new("cargo")
        .args(["mutants", "--version"])
        .current_dir(&root)
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        eprintln!(
            "xtask: mutants — cargo-mutants not installed \
             (`cargo install cargo-mutants --locked`); the PR gate runs it \
             regardless"
        );
        return false;
    }
    let diff_path = match write_diff_against_main(&root) {
        DiffOutcome::Wrote(path) => path,
        DiffOutcome::Empty => return true,
        DiffOutcome::Failed => return false,
    };
    let parallel = jobs
        .as_ref()
        .map_or_else(String::new, |n| format!(" --jobs {n}"));
    eprintln!(
        "xtask: mutants — cargo mutants --workspace --no-shuffle --in-diff {}{parallel} -- --all-features",
        diff_path.display()
    );
    let mut command = Command::new("cargo");
    command
        .args(["mutants", "--workspace", "--no-shuffle", "--in-diff"])
        .arg(&diff_path);
    if let Some(jobs) = &jobs {
        command.args(["--jobs", jobs]);
    }
    match command
        .args(["--", "--all-features"])
        .current_dir(&root)
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!(
                "xtask: mutants failed with {status} — every missed mutant is \
                 a behavior change no test observes; kill each before the PR"
            );
            false
        }
        Err(error) => {
            eprintln!("xtask: cannot run cargo mutants: {error}");
            false
        }
    }
}

/// The `--jobs N` value, when the task was given one.
///
/// Fails closed on anything else: a typo'd flag that were silently ignored
/// would run the gate at a parallelism the operator did not ask for and
/// report it as though they had.
fn jobs(extra: &[String]) -> Result<Option<String>, String> {
    match extra {
        [] => Ok(None),
        [flag, count] if flag == "--jobs" => match count.parse::<u32>() {
            Ok(parsed) if parsed >= 1 => Ok(Some(count.clone())),
            _ => Err(format!(
                "--jobs takes a positive whole number of parallel jobs, not {count:?}"
            )),
        },
        _ => Err(format!(
            "usage: cargo xtask mutants [--jobs N]; got {extra:?}"
        )),
    }
}

/// What producing the diff yielded; each variant was already reported.
enum DiffOutcome {
    Wrote(PathBuf),
    Empty,
    Failed,
}

/// Writes `git diff origin/main` to `target/xtask-mutants.diff`.
/// Says so when a mutable crate holds an untracked `.rs` file.
///
/// `git diff` cannot see one, so `--in-diff` never mutates it and the gate goes
/// green over code it did not test. CI is unaffected — it runs on a checked-out
/// branch where everything is committed — which is exactly what makes this
/// worth saying out loud: the local run is the one that can quietly differ from
/// the gate it claims to reproduce.
fn warn_about_untracked_sources(root: &Path) {
    let Ok(output) = Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard", "--", "crates"])
        .current_dir(root)
        .output()
    else {
        return;
    };
    let listing = String::from_utf8_lossy(&output.stdout);
    let untracked: Vec<&str> = listing
        .lines()
        .filter(|path| Path::new(path).extension().is_some_and(|ext| ext == "rs"))
        .collect();
    if !untracked.is_empty() {
        eprintln!(
            "xtask: mutants — {} untracked source file(s) are invisible to `git diff` and \
             will NOT be mutated; `git add` them for a run that matches CI: {untracked:?}",
            untracked.len()
        );
    }
}

fn write_diff_against_main(root: &Path) -> DiffOutcome {
    let diff = Command::new("git")
        .args(["diff", "origin/main"])
        .current_dir(root)
        .output();
    let Ok(diff) = diff else {
        eprintln!("xtask: mutants — cannot run git diff origin/main");
        return DiffOutcome::Failed;
    };
    if !diff.status.success() {
        eprintln!(
            "xtask: mutants — git diff origin/main failed (is the ref \
             fetched? `git fetch origin main`)"
        );
        return DiffOutcome::Failed;
    }
    if diff.stdout.is_empty() {
        eprintln!("xtask: mutants — no diff against origin/main; nothing to test");
        return DiffOutcome::Empty;
    }
    warn_about_untracked_sources(root);
    let diff_path = root.join("target/xtask-mutants.diff");
    if let Err(error) = std::fs::write(&diff_path, &diff.stdout) {
        eprintln!(
            "xtask: mutants — cannot write {}: {error}",
            diff_path.display()
        );
        return DiffOutcome::Failed;
    }
    DiffOutcome::Wrote(diff_path)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_mutation_gate_takes_a_jobs_count_and_nothing_else() {
        assert_eq!(jobs(&[]), Ok(None), "the default is cargo-mutants' own");
        assert_eq!(
            jobs(&["--jobs".to_owned(), "4".to_owned()]),
            Ok(Some("4".to_owned()))
        );
        // Fails closed rather than running at a parallelism nobody asked for.
        for wrong in [
            vec!["--jobs".to_owned()],
            vec!["--jobs".to_owned(), "0".to_owned()],
            vec!["--jobs".to_owned(), "many".to_owned()],
            vec!["--jobs".to_owned(), "-1".to_owned()],
            vec!["-j".to_owned(), "4".to_owned()],
            vec!["--list".to_owned()],
            vec!["--jobs".to_owned(), "4".to_owned(), "--list".to_owned()],
        ] {
            assert!(jobs(&wrong).is_err(), "{wrong:?} was accepted");
        }
    }
}
