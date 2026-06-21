// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The `changelog-links` gate: every `## [X.Y.Z]` version heading in
//! `CHANGELOG.md` must carry a matching `[X.Y.Z]: <url>` reference definition,
//! and the `[Unreleased]:` definition must compare against the most recent
//! released version.
//!
//! Root cause this gate exists for (the same class `version_sync` guards for
//! the README): the release checklist moves `[Unreleased]` to a `## [X.Y.Z]`
//! heading and adds a fresh `[Unreleased]` section, but said nothing about the
//! link-reference definitions at the foot of the file. So v0.3.0 shipped with
//! `## [0.3.0]` but **no** `[0.3.0]: …` definition — a shortcut reference link
//! with no target renders as the literal text `[0.3.0]`, not a release link —
//! and `[Unreleased]:` still comparing against `v0.2.0`. The `docs-links` gate
//! misses both: it checks that the definitions it *finds* resolve, not that a
//! shortcut reference *has* one, and the `[Unreleased]:` target is an absolute
//! URL it skips by design. RELEASING.md now names the link-reference update,
//! and this gate fails the next release that forgets it.
//!
//! Fenced code blocks are stripped before scanning, so an example changelog
//! inside a fence is never mistaken for the real headings or definitions.
//! Fail-closed: a file with no `## [..]` headings fails, because a gate that
//! cannot find what it guards is worse than no gate.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

/// Runs the gate over `CHANGELOG.md`; `true` when every version heading has a
/// reference definition and the `[Unreleased]` compare base is current.
pub(crate) fn run() -> bool {
    let root = crate::workspace_root();
    let path = root.join("CHANGELOG.md");
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!("xtask: changelog-links — cannot read CHANGELOG.md");
        return false;
    };
    let found = problems(&text);
    if found.is_empty() {
        let releases = bracket_headings(&text)
            .iter()
            .filter(|label| is_version(label))
            .count();
        eprintln!(
            "xtask: changelog-links — {releases} version headings each carry a reference \
             definition and `[Unreleased]` compares against the latest release"
        );
        true
    } else {
        for problem in &found {
            eprintln!("xtask: changelog-links — {problem}");
        }
        false
    }
}

/// Every contract violation in `text`, human-readable. Empty when the changelog
/// is well-formed. Pure (no I/O), so the failure modes are driven directly by
/// synthetic input in the tests.
fn problems(text: &str) -> Vec<String> {
    let headings = bracket_headings(text);
    let defs = reference_defs(text);
    let mut problems = Vec::new();

    if headings.is_empty() {
        problems.push("no `## [..]` headings found — is this CHANGELOG.md?".to_owned());
        return problems;
    }

    // Every bracketed heading needs a definition, or it renders as literal text
    // instead of a link (the v0.3.0 defect).
    for label in &headings {
        if !defs.iter().any(|(defined, _)| defined == label) {
            problems.push(format!(
                "heading `[{label}]` has no `[{label}]: <url>` definition \
                 (renders as literal text, not a link)"
            ));
        }
    }
    // Every definition needs a heading — an orphan definition is a typo or a
    // stale leftover from a removed section.
    for (label, _) in &defs {
        if !headings.iter().any(|heading| heading == label) {
            problems.push(format!(
                "definition `[{label}]:` has no matching `## [{label}]` heading"
            ));
        }
    }
    // The `[Unreleased]` compare link must target the most recent released
    // version — the first version-like heading, since the file is newest-first.
    if let Some(latest) = headings.iter().find(|label| is_version(label))
        && let Some((_, url)) = defs.iter().find(|(label, _)| label == "Unreleased")
    {
        match compare_base(url) {
            Some(base) => {
                let want = format!("v{latest}");
                if base != want {
                    problems.push(format!(
                        "`[Unreleased]:` compares against `{base}`, but the latest \
                         release is `{want}` — repoint it to `…/compare/{want}...HEAD`"
                    ));
                }
            }
            None => problems.push(format!(
                "`[Unreleased]:` is not a `compare/vX.Y.Z...HEAD` link: {url}"
            )),
        }
    }
    problems
}

/// The label of every `## [label] …` level-2 heading, in document order
/// (`Unreleased` and the versions), fenced code skipped.
fn bracket_headings(text: &str) -> Vec<String> {
    let mut headings = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = line.strip_prefix("## [")
            && let Some((label, _)) = rest.split_once(']')
        {
            headings.push(label.to_owned());
        }
    }
    headings
}

/// Every `[label]: url` reference definition (label and target), fenced code
/// skipped.
fn reference_defs(text: &str) -> Vec<(String, String)> {
    let mut defs = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[')
            && let Some((label, url)) = rest.split_once("]: ")
        {
            defs.push((label.to_owned(), url.trim().to_owned()));
        }
    }
    defs
}

/// The base ref of a GitHub `…/compare/<base>...<head>` URL (`v0.3.0` from
/// `…/compare/v0.3.0...HEAD`); `None` when the URL is not a compare link.
fn compare_base(url: &str) -> Option<String> {
    let after = url.split_once("compare/")?.1;
    let base = after.split_once("...")?.0;
    (!base.is_empty()).then(|| base.to_owned())
}

/// A loose `X.Y.Z` (all-numeric components) test — enough to tell a version
/// heading from `Unreleased` without a full semver parser.
fn is_version(label: &str) -> bool {
    let mut components = 0u8;
    for component in label.split('.') {
        components += 1;
        if component.is_empty() || !component.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
    }
    components == 3
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const GOOD: &str = "\
# Changelog

## [Unreleased]
### Added
- thing

## [0.3.0] - 2026-06-14
- stuff

## [0.2.0] - 2026-06-11
- old

[Unreleased]: https://example.com/compare/v0.3.0...HEAD
[0.3.0]: https://example.com/releases/tag/v0.3.0
[0.2.0]: https://example.com/releases/tag/v0.2.0
";

    #[test]
    fn a_well_formed_changelog_has_no_problems() {
        assert!(problems(GOOD).is_empty(), "{:?}", problems(GOOD));
    }

    #[test]
    fn flags_a_version_heading_with_no_definition() {
        // The v0.3.0 defect itself: the `[0.3.0]:` definition is missing.
        let doc = GOOD.replace("[0.3.0]: https://example.com/releases/tag/v0.3.0\n", "");
        let found = problems(&doc);
        assert!(
            found
                .iter()
                .any(|p| p.contains("[0.3.0]") && p.contains("literal text")),
            "{found:?}"
        );
    }

    #[test]
    fn flags_a_stale_unreleased_compare_base() {
        // The other half of the v0.3.0 defect: Unreleased still on v0.2.0.
        let doc = GOOD.replace("compare/v0.3.0...HEAD", "compare/v0.2.0...HEAD");
        let found = problems(&doc);
        assert!(
            found
                .iter()
                .any(|p| p.contains("Unreleased") && p.contains("v0.2.0")),
            "{found:?}"
        );
    }

    #[test]
    fn flags_an_orphan_definition() {
        let doc = format!("{GOOD}[0.9.9]: https://example.com/releases/tag/v0.9.9\n");
        let found = problems(&doc);
        assert!(
            found
                .iter()
                .any(|p| p.contains("[0.9.9]") && p.contains("no matching")),
            "{found:?}"
        );
    }

    #[test]
    fn flags_an_unreleased_target_that_is_not_a_compare_link() {
        let doc = GOOD.replace(
            "[Unreleased]: https://example.com/compare/v0.3.0...HEAD",
            "[Unreleased]: https://example.com/releases/tag/v0.3.0",
        );
        let found = problems(&doc);
        assert!(
            found
                .iter()
                .any(|p| p.contains("Unreleased") && p.contains("not a `compare")),
            "{found:?}"
        );
    }

    #[test]
    fn fenced_blocks_are_ignored() {
        // A `## [9.9.9]` heading and a `[fake]:` definition inside a fence must
        // be invisible to both scanners — an example changelog is not the file.
        let doc = "## [Unreleased]\n```\n## [9.9.9]\n[fake]: x\n```\n## [1.0.0]\n\n[Unreleased]: https://e/compare/v1.0.0...HEAD\n[1.0.0]: https://e/releases/tag/v1.0.0\n";
        let found = problems(doc);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn compare_base_extracts_the_tag() {
        assert_eq!(
            compare_base("https://x/compare/v0.3.0...HEAD").as_deref(),
            Some("v0.3.0")
        );
        assert_eq!(compare_base("https://x/releases/tag/v0.3.0"), None);
    }

    #[test]
    fn is_version_distinguishes_versions_from_unreleased() {
        assert!(is_version("0.3.0"));
        assert!(is_version("10.20.30"));
        assert!(!is_version("Unreleased"));
        assert!(!is_version("0.3"));
        assert!(!is_version("v0.3.0"));
        assert!(!is_version("1.2.x"));
    }

    #[test]
    fn the_committed_changelog_passes() {
        // Guards the real file: `cargo test` fails the same way CI would if a
        // release leaves a version heading undefined or the compare base stale.
        assert!(run());
    }
}
