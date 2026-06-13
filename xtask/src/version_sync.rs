// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `version-sync` task: the README's stated crates.io version must equal
//! `[workspace.package].version`.
//!
//! The 2026-06-13 audit's predecessor found the README claiming `0.1.0` on
//! crates.io long after `0.2.0` shipped — a stale fact nothing re-checked,
//! because the release checklist updates `SECURITY.md` but never the README's
//! version line. This gate ties the two together so a release cannot reintroduce
//! that falsehood: bump `[workspace.package].version` and the README's
//! `**Status:` line must move with it, or CI fails. Fail-closed — an anchor it
//! cannot find in either file fails, because a version gate that cannot locate
//! the version it guards is worse than no gate.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fs;

/// Verifies the README's `**Status:` version equals the workspace version;
/// `true` when they agree. Reports the mismatch (or the missing anchor) loudly.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let Ok(manifest) = fs::read_to_string(root.join("Cargo.toml")) else {
        eprintln!("xtask: version sync — cannot read Cargo.toml");
        return false;
    };
    let Ok(readme) = fs::read_to_string(root.join("README.md")) else {
        eprintln!("xtask: version sync — cannot read README.md");
        return false;
    };
    let Some(want) = workspace_version(&manifest) else {
        eprintln!("xtask: version sync — [workspace.package] carries no `version` key");
        return false;
    };
    let got = match readme_status_version(&readme) {
        Ok(version) => version,
        Err(problem) => {
            eprintln!("xtask: version sync — {problem}");
            return false;
        }
    };
    if want == got {
        eprintln!(
            "xtask: version sync — README states `{got}`, matching [workspace.package].version"
        );
        true
    } else {
        eprintln!(
            "xtask: version sync — README's `**Status:` line says `{got}` but \
             [workspace.package].version is `{want}`; bump the README's version line \
             (this is the README update the release checklist otherwise forgets)"
        );
        false
    }
}

/// The `version` value from the manifest's `[workspace.package]` table — *not*
/// `rust-version`, and not a `version` key from any other table.
fn workspace_version(manifest: &str) -> Option<String> {
    let start = manifest.find("[workspace.package]")?;
    for line in manifest[start..].lines().skip(1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            break; // Reached the next table without a version.
        }
        if let Some((key, value)) = trimmed.split_once('=')
            && key.trim() == "version"
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// The version in the README's single `**Status:` line — the first backticked
/// token, which must look like a version. Errors (fail-closed) when the line is
/// missing, duplicated, or carries no version token.
fn readme_status_version(readme: &str) -> Result<String, String> {
    let mut found: Option<String> = None;
    for line in readme.lines() {
        let Some(rest) = line.strip_prefix("**Status:") else {
            continue;
        };
        let token = first_backticked(rest)
            .ok_or_else(|| format!("README `**Status:` line carries no `version`: {line}"))?;
        if !is_version_like(token) {
            return Err(format!(
                "README `**Status:` token `{token}` is not a version"
            ));
        }
        if found.replace(token.to_owned()).is_some() {
            return Err("README has more than one `**Status:` line".to_owned());
        }
    }
    found.ok_or_else(|| "README has no `**Status:` line — the version-sync anchor moved".to_owned())
}

/// The text between the first pair of backticks in `text`.
fn first_backticked(text: &str) -> Option<&str> {
    let open = text.find('`')? + 1;
    let close = text[open..].find('`')? + open;
    Some(&text[open..close])
}

/// A loose `X.Y.Z` (all-numeric components) test — enough to reject prose
/// without re-implementing a full semver parser.
fn is_version_like(token: &str) -> bool {
    let mut components = 0u8;
    for component in token.split('.') {
        components += 1;
        if component.is_empty() || !component.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    components == 3
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn workspace_version_reads_the_package_table_not_rust_version() {
        // rust-version precedes version, and a later table also has a `version`
        // key — the gate must pick the [workspace.package] one and stop there.
        let manifest = "[workspace.package]\nrust-version = \"1.88\"\nversion = \"0.3.1\"\n\n[workspace.dependencies]\nserde = { version = \"1\" }\n";
        assert_eq!(workspace_version(manifest).as_deref(), Some("0.3.1"));
    }

    #[test]
    fn readme_status_version_extracts_the_first_backtick_token() {
        let readme = "# title\n**Status: `1.20.3` on [crates.io](u)** (`cargo install`)\nbody\n";
        assert_eq!(readme_status_version(readme).unwrap(), "1.20.3");
    }

    #[test]
    fn readme_status_version_fails_closed_without_the_anchor() {
        assert!(readme_status_version("# no status line here\n").is_err());
    }

    #[test]
    fn readme_status_version_rejects_a_non_version_token() {
        assert!(readme_status_version("**Status: `beta` on crates.io**\n").is_err());
    }

    #[test]
    fn is_version_like_distinguishes_versions_from_prose() {
        assert!(is_version_like("0.2.0"));
        assert!(is_version_like("10.20.30"));
        assert!(!is_version_like("0.2"));
        assert!(!is_version_like("1.2.3.4"));
        assert!(!is_version_like("v1.2.3"));
        assert!(!is_version_like("1.2.x"));
        assert!(!is_version_like(""));
    }

    #[test]
    fn the_committed_tree_is_in_sync() {
        // Guards the gate against the real files: `cargo test` fails the same
        // way CI would if the README and the workspace version ever diverge.
        assert!(run());
    }
}
