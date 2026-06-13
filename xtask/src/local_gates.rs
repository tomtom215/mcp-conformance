// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The local quality gates `cargo xtask ci` composes beyond the cargo
//! steps: the file-size cap, cargo-deny, the MSRV clippy leg, and the
//! diff-scoped mutation gate (`cargo xtask mutants`).
//!
//! Skip discipline: a gate whose tool is absent skips LOUDLY, naming the
//! install command and the CI job that enforces it regardless — a silent
//! skip is how local-vs-CI gate skew taught round two its lesson (a
//! versionless path dependency sailed through a green local run and failed
//! only in CI).

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::PathBuf;
use std::process::Command;

/// The MSRV this workspace pins (ADR-0008); the clippy leg runs on it.
const MSRV: &str = "1.88.0";

/// Runs `cargo deny check` when cargo-deny is installed; skips LOUDLY when it
/// is not. The CI `deny` job is the enforcement of record, but a versionless
/// path dependency once sailed through a green `cargo xtask ci` and failed
/// only in CI — the local gate set must run the same check when it can, and
/// must never skip it silently when it cannot.
pub(crate) fn deny_gate() -> bool {
    let root = crate::workspace_root();
    let available = Command::new("cargo")
        .args(["deny", "--version"])
        .current_dir(&root)
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        eprintln!(
            "xtask: cargo-deny — SKIPPED (not installed; `cargo install cargo-deny --locked`). \
             CI runs this gate regardless: a dependency-policy violation will fail there."
        );
        return true;
    }
    // Global options precede the subcommand in cargo-deny's CLI; this mirrors
    // the CI action's invocation (`--all-features check`) exactly.
    eprintln!("xtask: cargo-deny — cargo deny --all-features check");
    match Command::new("cargo")
        .args(["deny", "--all-features", "check"])
        .current_dir(&root)
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("xtask: cargo-deny failed with {status}");
            false
        }
        Err(error) => {
            eprintln!("xtask: cannot run cargo deny: {error}");
            false
        }
    }
}

/// Runs `cargo semver-checks check-release` against the published crates.io
/// baseline when cargo-semver-checks is installed; skips LOUDLY when it is not.
/// A conformance tool's public contract is partly its Rust API: this gate
/// catches an API-breaking change shipped under a version bump that does not
/// admit one (a breaking change on a patch release), so the changelog's
/// deliberate, declared breaks are never confused with accidental API breaks it
/// failed to declare. Network: it fetches the baseline from crates.io, so — like
/// `spec-drift` — it is a release-readiness gate run before tagging, not part of
/// the offline `ci` set. `xtask` is `publish = false` (no baseline) and excluded.
pub(crate) fn semver_gate() -> bool {
    let root = crate::workspace_root();
    let available = Command::new("cargo")
        .args(["semver-checks", "--version"])
        .current_dir(&root)
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        eprintln!(
            "xtask: cargo-semver-checks — SKIPPED (not installed; \
             `cargo install cargo-semver-checks --locked`). Run `cargo xtask \
             semver` before tagging a release: an undeclared API break must fail \
             before publish, not after."
        );
        return true;
    }
    eprintln!(
        "xtask: cargo-semver-checks — cargo semver-checks check-release \
         --workspace --exclude xtask"
    );
    match Command::new("cargo")
        .args([
            "semver-checks",
            "check-release",
            "--workspace",
            "--exclude",
            "xtask",
        ])
        .current_dir(&root)
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!(
                "xtask: cargo-semver-checks failed with {status} — an API change is \
                 inconsistent with the version bump; declare the break and bump \
                 accordingly (RELEASING.md: pre-1.0 minors may break, and say so)"
            );
            false
        }
        Err(error) => {
            eprintln!("xtask: cannot run cargo semver-checks: {error}");
            false
        }
    }
}

/// The MSRV clippy leg CI runs on every PR (all-features mode, the strictest
/// of CI's three): present locally so a stable-only green can no longer hide
/// an MSRV-incompatible construct until CI. Skips loudly when the toolchain
/// is not installed.
pub(crate) fn msrv_clippy_gate() -> bool {
    let root = crate::workspace_root();
    let available = Command::new("cargo")
        .arg(format!("+{MSRV}"))
        .arg("--version")
        .current_dir(&root)
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        eprintln!(
            "xtask: MSRV clippy — SKIPPED (toolchain {MSRV} not installed; \
             `rustup toolchain install {MSRV} --component clippy`). CI runs \
             this gate regardless: an MSRV break will fail there."
        );
        return true;
    }
    eprintln!(
        "xtask: MSRV clippy — cargo +{MSRV} clippy --workspace --all-targets \
         --all-features -- -D warnings"
    );
    match Command::new("cargo")
        .arg(format!("+{MSRV}"))
        .args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(&root)
        .status()
    {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("xtask: MSRV clippy failed with {status}");
            false
        }
        Err(error) => {
            eprintln!("xtask: cannot run cargo +{MSRV}: {error}");
            false
        }
    }
}

/// `cargo xtask mutants` — the exact diff-scoped mutation gate CI runs on
/// PRs, computed against `origin/main`, so "run the gates" includes the one
/// that catches untested code. Not part of `ci` (minutes, not seconds);
/// offered as its own task.
pub(crate) fn mutants_gate() -> bool {
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
    eprintln!(
        "xtask: mutants — cargo mutants --workspace --no-shuffle --in-diff {} -- --all-features",
        diff_path.display()
    );
    match Command::new("cargo")
        .args(["mutants", "--workspace", "--no-shuffle", "--in-diff"])
        .arg(&diff_path)
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

/// What producing the diff yielded; each variant was already reported.
enum DiffOutcome {
    Wrote(PathBuf),
    Empty,
    Failed,
}

/// Writes `git diff origin/main` to `target/xtask-mutants.diff`.
fn write_diff_against_main(root: &std::path::Path) -> DiffOutcome {
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

/// The ≤ 500-line cap from 04-engineering-standards §Source standards,
/// enforced over non-test source (crate and xtask `src/` trees) and the
/// embedded registry documents (whose loader promises per-file
/// reviewability). Integration tests and benches live outside `src/` and
/// are exempt by construction.
pub(crate) fn file_size_gate() -> bool {
    const CAP: usize = 500;
    let root = crate::workspace_root();
    let mut roots: Vec<PathBuf> = vec![root.join("xtask/src")];
    if let Ok(crates) = std::fs::read_dir(root.join("crates")) {
        for krate in crates.filter_map(Result::ok) {
            roots.push(krate.path().join("src"));
            roots.push(krate.path().join("registry"));
        }
    }
    let mut offenders = Vec::new();
    let mut scanned = 0usize;
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
                scanned += 1;
                let lines = text.lines().count();
                if lines > CAP {
                    offenders.push((path, lines));
                }
            }
        }
    }
    // A gate that scanned nothing proves nothing: the workspace has dozens
    // of source files, so an empty walk means the roots are wrong, and a
    // green verdict from it would be vacuous.
    if scanned < 10 {
        eprintln!("xtask: file sizes — only {scanned} files found; the scan roots are wrong");
        return false;
    }
    if offenders.is_empty() {
        eprintln!(
            "xtask: file sizes — every source and registry file ({scanned}) is within {CAP} lines"
        );
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn file_size_gate_scans_the_real_tree_and_passes() {
        // The gate that guards the cap is itself guarded: it must find a
        // plausible number of files (the vacuous-walk check) and the
        // committed tree must be within the cap.
        assert!(file_size_gate());
    }

    #[test]
    fn msrv_constant_matches_the_workspace_manifest() {
        // The leg runs the toolchain the manifest pins; a drift between the
        // two would test the wrong floor. rust-version omits the patch, so
        // compare the minor prefix.
        let manifest = std::fs::read_to_string(crate::workspace_root().join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("rust-version = \"1.88\""),
            "workspace rust-version moved; update local_gates::MSRV with it"
        );
        assert!(MSRV.starts_with("1.88"));
    }
}
