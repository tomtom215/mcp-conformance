// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The semver gate: `cargo semver-checks check-release` against each published
//! crate's crates.io baseline.
//!
//! A conformance tool's public contract is partly its Rust API: this gate
//! catches an API-breaking change shipped under a version bump that does not
//! admit one (a breaking change on a patch release), so the changelog's
//! deliberate, declared breaks are never confused with accidental API breaks it
//! failed to declare. Network: it fetches the baselines from crates.io, so —
//! like `spec-drift` — it is a release-readiness gate run before tagging (and by
//! `release.yml`), not part of the offline `ci` set.
//!
//! A crate with no published version has no baseline to break, and
//! cargo-semver-checks aborts the whole run on it — which is how the first
//! release to include `mcp-trace-capture` would have failed in `release.yml`.
//! So each publishable crate is looked up in the crates.io index first, and one
//! that is not there yet is excluded by name, out loud. The lookup fails closed:
//! any answer but "present" or "absent" fails the gate.
//!
//! **Which release the tree is judged as.** With no flag, cargo-semver-checks
//! derives the release type from the version bump, which is right for a
//! release commit and is what `release.yml` runs. Between releases the version
//! still equals the published one (`0.6.0 -> 0.6.0`, which the tool reads as a
//! patch), so a derived check fails every PR after the first deliberate break,
//! though the next pre-1.0 minor licenses it. `--unreleased`, which CI runs on
//! every PR, judges the tree as the release the changelog's `[Unreleased]`
//! section describes instead ([`release_type_for`]).
//!
//! The tool's release types are version *positions*, applied as given: at
//! `0.6.0`, `--release-type minor` still fails a removed item ("semver requires
//! new major version"; measured with 0.51.0 on 2026-10-13). The next pre-1.0
//! minor, `0.7.0`, is a breaking release to Cargo, so to the tool it is
//! `major`. Hence:
//! - a breaking-changes table in `[Unreleased]` declares a breaking release:
//!   the gate runs at `major`, which admits any break, so a `minor`-level pass
//!   runs first and lists every break, never failing, for the reviewer to hold
//!   against the table (nothing mechanical can match lints to table rows);
//! - entries without a table promise no break: the gate runs at `minor`
//!   (additions allowed), and an API break fails the PR until it is declared.
//!
//! `--release-type T` passes a type through unchanged.

// See `local_gates.rs`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private modules.
#![allow(clippy::redundant_pub_crate)]

use std::process::Command;

/// How the gate picks the release type cargo-semver-checks judges against.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// Derived from the version bump (cargo-semver-checks' own default).
    Derived,
    /// Read from `CHANGELOG.md`'s `[Unreleased]` section.
    Unreleased,
    /// Given on the command line.
    Explicit(String),
}

/// The flags `cargo xtask semver` accepts, or why they are refused.
fn parse_mode(args: &[String]) -> Result<Mode, String> {
    match args {
        [] => Ok(Mode::Derived),
        [flag] if flag == "--unreleased" => Ok(Mode::Unreleased),
        [flag, kind] if flag == "--release-type" => match kind.as_str() {
            "major" | "minor" | "patch" => Ok(Mode::Explicit(kind.clone())),
            other => Err(format!(
                "--release-type takes major, minor or patch, not {other:?}"
            )),
        },
        _ => Err(format!(
            "unexpected arguments {args:?}; expected none, --unreleased, or \
             --release-type major|minor|patch"
        )),
    }
}

/// The cargo-semver-checks release type `[Unreleased]` declares:
/// `Some("major")` when it carries a breaking-changes table (a header row with
/// a `Migration` column, the shape STABILITY.md and RELEASING.md require of
/// every breaking release), `Some("minor")` when it has entries but no such
/// table, and `None` when it is empty. Empty means a release commit, whose version bump is the declaration,
/// so the type is derived from it as `release.yml` derives it. Fenced code is
/// an example, never a declaration.
fn release_type_for(changelog: &str) -> Option<&'static str> {
    let mut in_section = false;
    let mut in_fence = false;
    let mut has_entries = false;
    for line in changelog.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            has_entries |= in_section;
            continue;
        }
        if !in_fence && line.starts_with("## ") {
            if in_section {
                break;
            }
            in_section = line.starts_with("## [Unreleased]");
            continue;
        }
        if !in_section || in_fence || trimmed.is_empty() {
            continue;
        }
        has_entries = true;
        if trimmed.starts_with('|')
            && trimmed
                .trim_matches('|')
                .split('|')
                .any(|cell| cell.trim() == "Migration")
        {
            return Some("major");
        }
    }
    has_entries.then_some("minor")
}

/// Runs the gate (`args` as `cargo xtask semver` received them); skips LOUDLY
/// when cargo-semver-checks is not installed.
pub(crate) fn semver_gate(args: &[String]) -> bool {
    let root = crate::workspace_root();
    let mode = match parse_mode(args) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("xtask: cargo-semver-checks — {message}");
            return false;
        }
    };
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
    let Some(excluded) = excluded_packages() else {
        return false;
    };
    let release_type = match mode {
        Mode::Derived => None,
        Mode::Explicit(kind) => Some(kind),
        Mode::Unreleased => match unreleased_release_type(&root, &excluded) {
            Ok(kind) => kind.map(str::to_owned),
            Err(message) => {
                eprintln!("xtask: cargo-semver-checks — {message}");
                return false;
            }
        },
    };
    match check_release(&root, &excluded, release_type.as_deref()) {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!(
                "xtask: cargo-semver-checks failed with {status} — an API change is \
                 inconsistent with the release being judged; declare the break (a row \
                 in CHANGELOG.md [Unreleased]'s breaking-changes table, and at release \
                 a version bump that admits it: RELEASING.md, pre-1.0 minors may \
                 break, and say so)"
            );
            false
        }
        Err(error) => {
            eprintln!("xtask: cannot run cargo semver-checks: {error}");
            false
        }
    }
}

/// `--unreleased`: the type `[Unreleased]` declares, said out loud. When it
/// declares a breaking release, first runs the informational `minor` pass.
fn unreleased_release_type(
    root: &std::path::Path,
    excluded: &[String],
) -> Result<Option<&'static str>, String> {
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md"))
        .map_err(|error| format!("cannot read CHANGELOG.md: {error}"))?;
    let kind = release_type_for(&changelog);
    eprintln!(
        "xtask: cargo-semver-checks — [Unreleased] {}",
        match kind {
            Some("major") => "declares breaking changes: judged as a breaking release",
            Some(_) => "declares no breaking change: any API break fails",
            None => "is empty: the release type is derived from the version",
        }
    );
    if kind == Some("major") {
        // A breaking release admits any break, so the gate cannot fail on
        // one. This pass is what makes it worth reading: every break against
        // the baseline, to compare with the table.
        eprintln!(
            "xtask: cargo-semver-checks — informational, never fails: every API \
             break against the baseline (compare with [Unreleased]'s table)"
        );
        let _ = check_release(root, excluded, Some("minor"));
    }
    Ok(kind)
}

/// One `cargo semver-checks check-release` over the workspace minus `excluded`.
fn check_release(
    root: &std::path::Path,
    excluded: &[String],
    release_type: Option<&str>,
) -> std::io::Result<std::process::ExitStatus> {
    let mut args = vec![
        "semver-checks".to_owned(),
        "check-release".to_owned(),
        "--workspace".to_owned(),
    ];
    for package in excluded {
        args.push("--exclude".to_owned());
        args.push(package.clone());
    }
    if let Some(kind) = release_type {
        args.push("--release-type".to_owned());
        args.push(kind.to_owned());
    }
    eprintln!("xtask: cargo-semver-checks — cargo {}", args.join(" "));
    Command::new("cargo").args(&args).current_dir(root).status()
}

/// `xtask` (never published) and every publishable crate with no version on
/// crates.io yet, each named on stderr; `None`, having said why, when that
/// cannot be determined.
fn excluded_packages() -> Option<Vec<String>> {
    let mut excluded = vec!["xtask".to_owned()];
    let packages = publishable_packages()
        .map_err(|message| eprintln!("xtask: cargo-semver-checks — {message}"))
        .ok()?;
    for package in packages {
        match published(&package) {
            Ok(true) => {}
            Ok(false) => {
                eprintln!(
                    "xtask: cargo-semver-checks — {package} has no published version, so \
                     no baseline to break; excluded (its first release)"
                );
                excluded.push(package);
            }
            Err(message) => {
                eprintln!("xtask: cargo-semver-checks — {message}");
                return None;
            }
        }
    }
    Some(excluded)
}

/// The workspace packages `cargo publish` would upload: those whose manifest
/// does not set `publish = false`.
fn publishable_packages() -> Result<Vec<String>, String> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(crate::workspace_root())
        .output()
        .map_err(|error| format!("cannot run cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err("cargo metadata failed".to_owned());
    }
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("cargo metadata output is not JSON: {error}"))?;
    Ok(metadata["packages"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        // `publish` is null when unrestricted and `[]` for `publish = false`.
        .filter(|package| package["publish"].is_null())
        .filter_map(|package| package["name"].as_str().map(str::to_owned))
        .collect())
}

/// Whether `name` has a version on crates.io, from its sparse-index file.
fn published(name: &str) -> Result<bool, String> {
    let url = format!("https://index.crates.io/{}", index_path(name));
    let output = Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--max-time",
            "30",
            &url,
        ])
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    match String::from_utf8_lossy(&output.stdout).trim() {
        "200" => Ok(true),
        "404" => Ok(false),
        other => Err(format!(
            "cannot tell whether {name} is published: {url} answered {other:?}"
        )),
    }
}

/// A crate's path in the crates.io sparse index (the registry's documented
/// layout: by name length, then by leading characters, lowercased).
fn index_path(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    match name.len() {
        1 => format!("1/{name}"),
        2 => format!("2/{name}"),
        3 => format!("3/{}/{name}", &name[..1]),
        _ => format!("{}/{}/{name}", &name[..2], &name[2..4]),
    }
}

#[cfg(test)]
mod tests {
    use super::{Mode, index_path, parse_mode, release_type_for};

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn modes_parse_and_anything_else_is_refused() {
        assert_eq!(parse_mode(&[]), Ok(Mode::Derived));
        assert_eq!(
            parse_mode(&strings(&["--unreleased"])),
            Ok(Mode::Unreleased)
        );
        assert_eq!(
            parse_mode(&strings(&["--release-type", "minor"])),
            Ok(Mode::Explicit("minor".to_owned()))
        );
        assert!(parse_mode(&strings(&["--release-type", "huge"])).is_err());
        assert!(parse_mode(&strings(&["--release-type"])).is_err());
        assert!(parse_mode(&strings(&["--unreleased", "--release-type", "patch"])).is_err());
    }

    #[test]
    fn a_breaking_changes_table_declares_a_breaking_release() {
        let changelog = "# Changelog\n\n## [Unreleased]\n\nIntro.\n\n\
            | Surface | Change | Migration |\n|---|---|---|\n| API | gone | use x |\n\n\
            ## [0.6.0] - 2026-10-04\n";
        assert_eq!(release_type_for(changelog), Some("major"));
    }

    #[test]
    fn entries_without_a_table_declare_no_break() {
        // The released section's table belongs to that release, not this one.
        let changelog = "## [Unreleased]\n\n### Fixed\n\n- a thing\n\n\
            ## [0.6.0] - 2026-10-04\n\n| Crate | Break | Migration |\n";
        assert_eq!(release_type_for(changelog), Some("minor"));
    }

    #[test]
    fn an_empty_section_derives_from_the_version() {
        let changelog = "## [Unreleased]\n\n## [0.7.0] - 2026-11-01\n\n\
            | Surface | Change | Migration |\n";
        assert_eq!(release_type_for(changelog), None);
        assert_eq!(release_type_for("# Changelog\n"), None);
    }

    #[test]
    fn a_table_inside_a_fence_is_an_example_not_a_declaration() {
        let changelog = "## [Unreleased]\n\n```text\n| Surface | Change | Migration |\n\
            ## [0.6.0]\n```\n\n## [0.6.0]\n";
        assert_eq!(release_type_for(changelog), Some("minor"));
    }

    #[test]
    fn a_migration_word_outside_a_table_row_declares_nothing() {
        let changelog = "## [Unreleased]\n\n- Migration | notes, not a table\n";
        assert_eq!(release_type_for(changelog), Some("minor"));
    }

    #[test]
    fn index_paths_follow_the_registry_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("mcp-trace-capture"), "mc/p-/mcp-trace-capture");
        assert_eq!(index_path("Serde"), "se/rd/serde");
    }
}
