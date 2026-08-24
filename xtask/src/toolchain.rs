// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The pinned toolchain: that every workflow agrees on it, and that it has not
//! silently fallen behind the stable it tracks.
//!
//! `rust-toolchain.toml` names the compiler this workspace gates on. It has to
//! be pinned, for the reason the official-suite versions are: every gate runs
//! at `-D warnings` under clippy's pedantic and nursery groups, so the
//! toolchain is an *input* to the gate, and an input that moves underneath a
//! gate is not a gate. Before 2026-08-24 it floated — CI installed `stable`,
//! contributors ran whatever they had, and a `cargo xtask ci` that could not see
//! CI's lints reported green through thirteen consecutive red runs.
//!
//! Two tasks follow from a pin, and they are deliberately different shapes:
//!
//! - `toolchain-pin` is offline and runs in `cargo xtask ci`. `rustup` honours
//!   the file for every plain `cargo` in the tree, so most jobs need say
//!   nothing — but a leg that wants a *different* toolchain must override it in
//!   the environment, and the MSRV matrices name their version in `matrix`,
//!   which cannot read `env`. So the number is written out there and checked
//!   here, exactly as `local_gates`' `MSRV` is checked against the manifest. It also fails on a workflow that installs bare `stable` inside
//!   the workspace, which is the shape the pin exists to remove.
//!
//! - `toolchain-currency` is network and runs weekly beside the suite pins
//!   ([ADR-0010](../../docs/plan/decisions/0010-deferral-ledger-and-scheduled-reverification.md)).
//!   A Rust release is a maintenance event, not a defect in whatever pull
//!   request is open, so it pages the schedule rather than blocking unrelated
//!   work — and the weekly job files a tracking issue, so the news arrives.
//!   Bumping stays a decision: raise `channel`, run the gates, and fix or record
//!   what the new lints say in the same commit. Holding the pin deliberately is
//!   a legitimate outcome, recorded where the pin lives.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::path::{Path, PathBuf};

/// Rust's channel manifest for current stable — the one line this gate reads
/// out of it is `[pkg.rust]`'s `version`.
const STABLE_MANIFEST: &str = "https://static.rust-lang.org/dist/channel-rust-stable.toml";

/// Workflows that run cargo *inside* this workspace, and so are governed by the
/// pin. Listed rather than globbed: a new workflow should have to be considered,
/// and the check below fails when one of these is missing rather than passing
/// over a file it could not read.
const GOVERNED: [&str; 5] = [
    ".github/workflows/ci.yml",
    ".github/workflows/scheduled.yml",
    ".github/workflows/mutants.yml",
    ".github/workflows/pages.yml",
    ".github/workflows/release.yml",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// The `channel` of the committed `rust-toolchain.toml`.
///
/// # Errors
///
/// Returns the reason the file could not be read or did not name a channel — a
/// pin that cannot be found is a pin that is not holding.
pub(crate) fn pinned_channel(root: &Path) -> Result<String, String> {
    let path = root.join("rust-toolchain.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    channel_of(&text).ok_or_else(|| {
        format!(
            "{} has no `channel = \"…\"` line; the pin is the channel",
            path.display()
        )
    })
}

/// The channel a `rust-toolchain.toml` names, by its one significant line.
fn channel_of(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .find_map(|line| {
            let rest = line.strip_prefix("channel")?.trim_start();
            let value = rest.strip_prefix('=')?.trim();
            Some(value.trim_matches('"').to_owned())
        })
}

/// Verifies every governed workflow against the pin.
pub(crate) fn pin_gate() -> bool {
    let root = root();
    let pinned = match pinned_channel(&root) {
        Ok(pinned) => pinned,
        Err(error) => {
            eprintln!("xtask: toolchain-pin — {error}");
            return false;
        }
    };
    let mut problems = Vec::new();
    let mut named = 0_u32;
    for workflow in GOVERNED {
        let path = root.join(workflow);
        let Ok(text) = std::fs::read_to_string(&path) else {
            problems.push(format!(
                "cannot read {workflow}; a governed workflow that cannot be read \
                 cannot be checked"
            ));
            continue;
        };
        named += audit(workflow, &text, &pinned, &mut problems);
    }
    if problems.is_empty() {
        eprintln!(
            "xtask: toolchain-pin — rust-toolchain.toml pins {pinned}; {named} workflow \
             matrix entrie(s) name it and no governed workflow installs bare `stable`"
        );
        true
    } else {
        eprintln!("xtask: toolchain-pin — the pin and the workflows disagree:");
        for problem in &problems {
            eprintln!("  {problem}");
        }
        eprintln!(
            "  rust-toolchain.toml is the authority; edit the workflow, or bump both together."
        );
        false
    }
}

/// One workflow's toolchain lines, returning how many named the pin.
///
/// Two rules. A `rust:`/`toolchain:` matrix that names a concrete `X.Y.Z` must
/// name *this* one — a stale literal there silently gates on a compiler nobody
/// chose. And nothing may install bare `stable`, which would reintroduce the
/// floating input the pin removes.
fn audit(workflow: &str, text: &str, pinned: &str, problems: &mut Vec<String>) -> u32 {
    let mut named = 0;
    for (number, line) in text.lines().enumerate() {
        let line_no = number + 1;
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }
        if trimmed.contains("rustup toolchain install stable")
            || trimmed.contains("rustup default stable")
        {
            problems.push(format!(
                "{workflow}:{line_no} installs bare `stable`, which floats; the pinned \
                 toolchain comes from rust-toolchain.toml (`rustup show active-toolchain`)"
            ));
            continue;
        }
        for version in matrix_versions(trimmed) {
            named += 1;
            if version != pinned {
                problems.push(format!(
                    "{workflow}:{line_no} gates on {version}, rust-toolchain.toml pins {pinned}"
                ));
            }
        }
    }
    named
}

/// The `X.Y.Z` entries of a `rust:` / `toolchain:` matrix line.
///
/// Three-component only: `"1.88"` is the MSRV, written two-component on purpose
/// so it reads as a floor rather than a pin, and it is checked against the
/// manifest by [`crate::local_gates`] instead.
fn matrix_versions(line: &str) -> Vec<String> {
    let Some(rest) = line
        .strip_prefix("rust:")
        .or_else(|| line.strip_prefix("toolchain:"))
    else {
        return Vec::new();
    };
    rest.trim()
        .trim_matches(['[', ']'])
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_owned())
        .filter(|entry| entry.split('.').count() == 3 && entry.starts_with(char::is_numeric))
        .collect()
}

/// Asks Rust what the current stable is, and fails when the pin is not it.
pub(crate) fn currency_gate() -> bool {
    let root = root();
    let pinned = match pinned_channel(&root) {
        Ok(pinned) => pinned,
        Err(error) => {
            eprintln!("xtask: toolchain-currency — {error}");
            return false;
        }
    };
    eprintln!("xtask: toolchain-currency — asking rust-lang.org what stable currently is");
    let body = match fetch(STABLE_MANIFEST) {
        Ok(body) => body,
        Err(error) => {
            // A fetch failure is a failure: an unchecked pin is not a checked
            // one, the rule `spec-drift` and `suite-currency` both apply.
            eprintln!("xtask: toolchain-currency — cannot read {STABLE_MANIFEST}: {error}");
            return false;
        }
    };
    let Some(served) = stable_version(&body) else {
        eprintln!(
            "xtask: toolchain-currency — {STABLE_MANIFEST} carries no `[pkg.rust]` version; \
             the manifest's shape has changed and this gate needs re-reading"
        );
        return false;
    };
    complaint(&pinned, &served).map_or_else(
        || {
            eprintln!(
                "xtask: toolchain-currency — stable is {served}, and rust-toolchain.toml pins it"
            );
            true
        },
        |complaint| {
            eprintln!("xtask: toolchain-currency — {complaint}");
            false
        },
    )
}

/// What the served stable says about the pin that is not "unchanged".
///
/// Separate from the fetch so the comparison is testable without a network.
/// Inequality rather than "newer than": a pin *ahead* of stable is news too (a
/// beta version pinned by mistake, a yanked release), and saying so needs no
/// semver comparison and therefore no dependency to do the comparing.
fn complaint(pinned: &str, served: &str) -> Option<String> {
    (pinned != served).then(|| {
        format!(
            "stable is now {served}; rust-toolchain.toml pins {pinned}. Re-decide: raise \
             `channel`, run `cargo xtask ci`, and fix or record what the new lints say in the \
             commit that raises it. Holding the pin is a legitimate outcome — record it where \
             the pin lives, not by leaving this red"
        )
    })
}

/// The `X.Y.Z` of the channel manifest's `[pkg.rust]` section.
///
/// The manifest lists every component and target, and many carry their own
/// `version` line — `[pkg.cargo]`'s is a different number entirely — so this
/// reads the section rather than the first match.
fn stable_version(manifest: &str) -> Option<String> {
    let mut in_rust = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_rust = line == "[pkg.rust]";
            continue;
        }
        if in_rust && let Some(rest) = line.strip_prefix("version") {
            let quoted = rest.trim_start().strip_prefix('=')?.trim();
            let value = quoted.trim_matches('"');
            // `1.98.0 (88d9e12ae 2026-08-18)` — the release, then its build.
            return value.split_whitespace().next().map(ToOwned::to_owned);
        }
    }
    None
}

/// Fetches one URL via curl — a checked tool dependency CI runners already
/// have, in the same shape `spec-drift` and `suite-currency` use.
fn fetch(url: &str) -> Result<String, String> {
    let output = std::process::Command::new("curl")
        .args(["-sSf", "--max-time", "30", url])
        .output()
        .map_err(|error| format!("cannot run curl: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "curl exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| "response is not UTF-8".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_workflows_agree_with_the_committed_pin() {
        // The gate, run against the real tree: the assertion that would have
        // caught a workflow gating on a compiler the pin does not name.
        assert!(pin_gate(), "see the report above");
    }

    #[test]
    fn the_channel_is_read_past_the_commentary() {
        let file = "# channel = \"9.9.9\" in a comment is not the pin\n\
                    [toolchain]\nchannel = \"1.98.0\"\nprofile = \"minimal\"\n";
        assert_eq!(channel_of(file).as_deref(), Some("1.98.0"));
        assert_eq!(channel_of("[toolchain]\nprofile = \"minimal\"\n"), None);
    }

    #[test]
    fn only_three_component_matrix_entries_are_the_pin() {
        // `1.88` is the MSRV floor and is checked elsewhere; `1.98.0` is the pin.
        assert_eq!(
            matrix_versions(r#"rust: ["1.98.0", "1.88"]"#),
            vec!["1.98.0".to_owned()]
        );
        // A user-facing matrix naming `stable` names no version to check.
        assert!(matrix_versions(r#"toolchain: [stable, "1.88"]"#).is_empty());
        assert!(matrix_versions("os: [ubuntu-latest]").is_empty());
    }

    #[test]
    fn a_stale_literal_and_a_floating_install_are_both_reported() {
        let mut problems = Vec::new();
        let named = audit(
            "w.yml",
            "        rust: [\"1.97.0\", \"1.88\"]\n          rustup default stable\n",
            "1.98.0",
            &mut problems,
        );
        assert_eq!(named, 1);
        assert_eq!(problems.len(), 2, "{problems:?}");
        // By content rather than by position: the two are reported in line
        // order, which is a property of the fixture rather than of the rule.
        assert!(
            problems
                .iter()
                .any(|p| p.contains("installs bare `stable`")),
            "{problems:?}"
        );
        assert!(
            problems.iter().any(|p| p.contains("gates on 1.97.0")),
            "{problems:?}"
        );
    }

    #[test]
    fn the_manifest_version_comes_from_the_rust_section() {
        let manifest = "[pkg.cargo]\nversion = \"0.99.0 (797e8a9bc 2026-08-05)\"\n\
                        [pkg.rust]\nversion = \"1.98.0 (88d9e12ae 2026-08-18)\"\n";
        assert_eq!(stable_version(manifest).as_deref(), Some("1.98.0"));
        assert_eq!(stable_version("[pkg.cargo]\nversion = \"0.99.0\"\n"), None);
    }

    #[test]
    fn a_pin_equal_to_stable_says_nothing_and_any_difference_says_what_to_do() {
        assert_eq!(complaint("1.98.0", "1.98.0"), None);
        let behind = complaint("1.97.0", "1.98.0").unwrap();
        assert!(behind.contains("stable is now 1.98.0"), "{behind}");
        assert!(
            behind.contains("Holding the pin is a legitimate outcome"),
            "{behind}"
        );
        // Ahead of stable is news too.
        assert!(complaint("1.99.0", "1.98.0").is_some());
    }
}
