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

mod mutants;

pub(crate) use mutants::mutants_gate;

/// The MSRV this workspace pins (ADR-0008); the clippy leg runs on it.
const MSRV: &str = "1.88.0";

/// Whether `cargo-deny` is installed. Split out so the `ci` summary can name
/// what did not run without duplicating the probe.
fn deny_available() -> bool {
    Command::new("cargo")
        .args(["deny", "--version"])
        .current_dir(crate::workspace_root())
        .output()
        .is_ok_and(|output| output.status.success())
}

/// Whether clippy exists **for the MSRV toolchain** — see `msrv_clippy_gate`
/// for why the toolchain alone is not a sufficient probe.
fn msrv_clippy_available() -> bool {
    Command::new("cargo")
        .arg(format!("+{MSRV}"))
        .args(["clippy", "--version"])
        .current_dir(crate::workspace_root())
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The gates this machine cannot run, by name.
///
/// A local run that skips a gate is not a smaller version of CI — it is a
/// different, weaker claim, and the difference has to be stated where the
/// reader is looking. `cargo xtask ci` prints "all steps passed" partway
/// through (it means the cargo steps), so a skip notice scrolling past above
/// that line is easy to read as coverage. This drives the closing summary that
/// cannot be.
pub(crate) fn skipped_gates() -> Vec<&'static str> {
    let mut skipped = Vec::new();
    if !msrv_clippy_available() {
        skipped.push("MSRV clippy");
    }
    if !deny_available() {
        skipped.push("cargo-deny");
    }
    skipped
}

/// The closing line for `cargo xtask ci`, given the gates that did not run.
pub(crate) fn ci_summary(skipped: &[&str]) -> String {
    if skipped.is_empty() {
        return "xtask: ci — every local gate ran and passed".to_owned();
    }
    format!(
        "xtask: ci — PASSED, but {} gate(s) did NOT run here: {}. CI enforces them, so a \
         green local run is a weaker claim than a green CI run — install the missing tools \
         before treating this as verification.",
        skipped.len(),
        skipped.join(", ")
    )
}

/// Runs `cargo deny check` when cargo-deny is installed; skips LOUDLY when it
/// is not. The CI `deny` job is the enforcement of record, but a versionless
/// path dependency once sailed through a green `cargo xtask ci` and failed
/// only in CI — the local gate set must run the same check when it can, and
/// must never skip it silently when it cannot.
pub(crate) fn deny_gate() -> bool {
    let root = crate::workspace_root();
    if !deny_available() {
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
    // Probe clippy specifically, not just the toolchain: `cargo +{MSRV}
    // --version` succeeds even when rustup auto-installs a *minimal* toolchain on
    // the `+` reference, so the older probe reported "available" and then
    // hard-failed on a missing clippy component — exactly what the v0.3.0 release
    // rehearsal surfaced (release.yml had installed `1.88` while this gate asks
    // for `1.88.0`). Checking `clippy --version` makes the loud skip below honest.
    if !msrv_clippy_available() {
        eprintln!(
            "xtask: MSRV clippy — SKIPPED (clippy for toolchain {MSRV} not installed; \
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

/// Extensions whose files cannot carry a comment, so cannot carry a header.
///
/// JSON has no comment syntax at all, and this repository holds a great deal of
/// it: the registries, every golden report, every corpus trace. Requiring a
/// header there would mean inventing a member for it inside the data, which
/// would then have to be excluded from every schema and every diff.
const UNCOMMENTABLE: [&str; 2] = ["json", "jsonl"];

/// Tracked paths that deliberately carry no header, each for a reason that is
/// not "nobody got round to it".
///
/// `Cargo.lock` files are generated by cargo and rewritten wholesale; a header
/// would be deleted by the next resolve. `LICENSE` *is* the licence the
/// identifier refers to. The fuzz corpus holds opaque byte inputs whose whole
/// value is being byte-exact — a header would change the input.
fn header_exempt(path: &str) -> bool {
    path == "LICENSE"
        || path.ends_with("Cargo.lock")
        || path.starts_with("fuzz/corpus/")
        || std::path::Path::new(path)
            .extension()
            .is_some_and(|ext| UNCOMMENTABLE.iter().any(|skip| ext == *skip))
}

/// Every tracked file that can carry a comment carries an SPDX identifier.
///
/// This was a line on the pull-request checklist and nothing else — a claim a
/// human ticked. It happened to be true (at 2026-08-24, 281 of 281 eligible
/// tracked files carried one), which is the argument *for* mechanizing it
/// rather than against: the cost of the gate is now, while the tree is clean,
/// and the thing it prevents is the one file that slips in during a large
/// change and is never noticed because the box was ticked from memory.
///
/// Reads `git ls-files` rather than walking the filesystem, so it judges what
/// is committed: a scratch file in the working tree cannot fail the gate, and a
/// file cannot pass it by being untracked.
pub(crate) fn spdx_gate() -> bool {
    let root = crate::workspace_root();
    let Ok(listing) = Command::new("git")
        .args(["ls-files"])
        .current_dir(&root)
        .output()
    else {
        eprintln!("xtask: spdx — cannot run git ls-files");
        return false;
    };
    if !listing.status.success() {
        eprintln!("xtask: spdx — git ls-files failed; is this a git checkout?");
        return false;
    }

    let mut checked = 0_usize;
    let mut missing = Vec::new();
    for path in String::from_utf8_lossy(&listing.stdout).lines() {
        if path.is_empty() || header_exempt(path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            // Unreadable or not UTF-8: nothing to find a header in, and the
            // file-size gate reports the same class of file the same way.
            continue;
        };
        checked += 1;
        if !text
            .lines()
            .take(3)
            .any(|line| line.contains("SPDX-License-Identifier"))
        {
            missing.push(path.to_owned());
        }
    }

    // A gate that examined nothing proves nothing — the same vacuous-walk guard
    // `file_size_gate` carries, for the same reason.
    if checked < 10 {
        eprintln!("xtask: spdx — only {checked} file(s) examined; the listing is wrong");
        return false;
    }
    if missing.is_empty() {
        eprintln!(
            "xtask: spdx — all {checked} comment-carrying tracked file(s) have a licence header"
        );
        return true;
    }
    eprintln!(
        "xtask: spdx — {} tracked file(s) have no `SPDX-License-Identifier` in their first three \
         lines; add the two-line header the PR template shows:",
        missing.len()
    );
    for path in &missing {
        eprintln!("  {path}");
    }
    false
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
    fn a_complete_run_says_so_plainly() {
        let summary = super::ci_summary(&[]);
        assert!(summary.contains("every local gate ran"), "{summary}");
        assert!(!summary.contains("NOT run"), "{summary}");
    }

    #[test]
    fn a_partial_run_names_what_was_missing_and_refuses_to_call_it_verification() {
        let summary = super::ci_summary(&["MSRV clippy", "cargo-deny"]);
        // The count, both names, and the caveat all have to survive — this is
        // the line that stops "all steps passed" from being over-read.
        assert!(summary.contains("2 gate(s) did NOT run"), "{summary}");
        assert!(summary.contains("MSRV clippy, cargo-deny"), "{summary}");
        assert!(summary.contains("weaker claim"), "{summary}");
    }

    #[test]
    fn spdx_gate_scans_the_real_tree_and_passes() {
        // Same shape as the file-size gate's own test: the gate must find a
        // plausible number of files and the committed tree must satisfy it.
        assert!(spdx_gate());
    }

    #[test]
    fn the_exemptions_are_the_ones_argued_for_and_no_others() {
        // Each of these is exempt for a stated reason; a future edit that
        // widens the set has to change this test to do it.
        for exempt in [
            "LICENSE",
            "Cargo.lock",
            "fuzz/Cargo.lock",
            "fuzz/corpus/trace_parse/seed-full-session",
            "corpus/golden/http-session.json",
            "corpus/good/http-session.jsonl",
        ] {
            assert!(header_exempt(exempt), "{exempt} should be exempt");
        }
        // Everything that can hold a comment is judged, including the file
        // types a walker keyed only on `.rs` would miss.
        for judged in [
            "xtask/src/local_gates.rs",
            "README.md",
            "Cargo.toml",
            ".github/workflows/ci.yml",
            ".github/CODEOWNERS",
            "tools/extract-clauses.py",
            "rust-toolchain.toml",
        ] {
            assert!(!header_exempt(judged), "{judged} should be judged");
        }
        // `LICENSE-MIT` is not `LICENSE`: a suffix match would exempt it, and
        // a licence *copy* is an ordinary file that carries the header.
        assert!(!header_exempt("LICENSE-MIT"));
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
