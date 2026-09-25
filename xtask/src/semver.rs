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

// See `local_gates.rs`: rustc's `unreachable_pub` and clippy's
// `redundant_pub_crate` disagree about items in a binary's private modules.
#![allow(clippy::redundant_pub_crate)]

use std::process::Command;

/// Runs the gate; skips LOUDLY when cargo-semver-checks is not installed.
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
    let Some(excluded) = excluded_packages() else {
        return false;
    };
    let mut args = vec![
        "semver-checks".to_owned(),
        "check-release".to_owned(),
        "--workspace".to_owned(),
    ];
    for package in &excluded {
        args.push("--exclude".to_owned());
        args.push(package.clone());
    }
    eprintln!("xtask: cargo-semver-checks — cargo {}", args.join(" "));
    match Command::new("cargo")
        .args(&args)
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
    use super::index_path;

    #[test]
    fn index_paths_follow_the_registry_layout() {
        assert_eq!(index_path("a"), "1/a");
        assert_eq!(index_path("ab"), "2/ab");
        assert_eq!(index_path("abc"), "3/a/abc");
        assert_eq!(index_path("mcp-trace-capture"), "mc/p-/mcp-trace-capture");
        assert_eq!(index_path("Serde"), "se/rd/serde");
    }
}
