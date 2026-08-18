// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The prose half of `draft-coverage`: every count the current documents state,
//! checked against the committed reports.
//!
//! A generated table cannot stop a sentence three files away from disagreeing
//! with it, and that is how the counts drifted in the first place — the
//! numbers lived in narrative, where nothing could reach them. This reaches
//! them: the phrasing is fixed, the numbers are what vary, so any occurrence
//! of a phrase is parsed and required to name something real.
//!
//! This module owns the two questions that are not about parsing — *which*
//! documents are read, and what a document is reduced to before it is read.
//! The shapes themselves live one per module:
//!
//! - [`claim`] — `109 of the 124 judgeable clauses`, a pair against the corpus.
//! - [`verdict`] — `58 pass, 1 fail, …`, a tuple against one capture's row.
//! - [`readiness`] — `41 passing / 0 failing`, a score against the committed
//!   `draft-readiness` baseline.
//! - [`prose`] — Markdown with its code blanked, so a specimen of tool output
//!   is never read as an assertion about this corpus.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::path::Path;

use super::{Capture, Summary};

mod claim;
mod prose;
mod readiness;
mod verdict;

use claim::{claims, judge};
use readiness::Baseline;
use verdict::{judge_verdict, verdicts};

/// The Markdown a reader treats as current, and the only files whose claims
/// are checked.
///
/// The boundary is **living versus dated**, not shipped versus internal. A
/// dated document is allowed to be stale, because it records what was true when
/// it was written: `docs/reports/` is a measurement on a day, `docs/plan/`'s
/// `decisions/` are ADRs, and a released `CHANGELOG` section is a statement
/// about a shipped release — which is why only `CHANGELOG.md`'s `Unreleased`
/// section is scanned. Everything else here is maintained as current and is
/// therefore checkable.
///
/// The planning documents were outside this list until 2026-08-18, and it cost
/// exactly what it looks like it would: the sweep that corrected the
/// pre-[ADR-0012] vacuous-pass arithmetic everywhere the gate reached stopped
/// at this boundary, and the inflated pair survived in three plan documents
/// until a `CHANGELOG` entry quoted one of them and the gate rejected the
/// quote. A number the gate cannot see is a number that drifts.
///
/// [ADR-0012]: ../../../docs/plan/decisions/0012-not-observed-outcome.md
pub(super) const CLAIM_FILES: &[&str] = &[
    "README.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "corpus/README.md",
    "crates/mcp-conformance-core/README.md",
    "crates/mcp-trace-validator/README.md",
    "crates/mcp-everything-server/README.md",
    "crates/mcp-reference-host/README.md",
    "docs/plan/README.md",
    "docs/plan/00-charter.md",
    "docs/plan/01-ecosystem-context.md",
    "docs/plan/02-architecture.md",
    "docs/plan/03-conformance-strategy.md",
    "docs/plan/04-engineering-standards.md",
    "docs/plan/05-security-model.md",
    "docs/plan/06-roadmap.md",
    "docs/plan/07-ecosystem-engagement.md",
    "docs/plan/08-risk-register.md",
    "book/src/introduction.md",
    "book/src/architecture.md",
    "book/src/trace-format.md",
    "book/src/revisions.md",
    "book/src/corpus.md",
    "book/src/conformance-results.md",
    "book/src/SUMMARY.md",
];

/// How many documents the gate reads, for the success line: a count an
/// operator can compare against what they expected it to cover.
pub(super) const DOCUMENTS: usize = CLAIM_FILES.len();

/// Verifies every coverage claim in [`CLAIM_FILES`]; `true` when all agree.
pub(super) fn check(root: &Path, captures: &[Capture], summary: &Summary) -> bool {
    let allowed = summary.allowed();
    let baseline = match Baseline::load(root) {
        Ok(baseline) => baseline,
        Err(error) => {
            eprintln!("xtask: draft-coverage — {error}");
            return false;
        }
    };
    let mut ok = true;
    for name in CLAIM_FILES {
        let Ok(text) = fs::read_to_string(root.join(name)) else {
            eprintln!("xtask: draft-coverage — cannot read {name}");
            ok = false;
            continue;
        };
        let current = if *name == "CHANGELOG.md" {
            unreleased(&text)
        } else {
            &text
        };
        // Both shapes read the document with its code blanked out, so a
        // specimen of tool output is never mistaken for an assertion about this
        // corpus — see `prose`, which owns that reduction.
        let scanned = prose::without_code(current);
        for claim in claims(&scanned) {
            ok &= judge(name, &claim, summary, &allowed);
        }
        for verdict in verdicts(&scanned) {
            ok &= judge_verdict(name, &verdict, captures);
        }
        ok &= readiness::check(name, &scanned, &baseline);
    }
    ok
}

/// The changelog above its first released heading.
///
/// Everything below it is a statement about a shipped release and is allowed
/// to disagree with today's corpus — it was true when it was written.
fn unreleased(changelog: &str) -> &str {
    changelog
        .find("\n## [0")
        .map_or(changelog, |end| &changelog[..end])
}

/// Splits the ASCII digits off the end of `text`.
pub(super) fn trailing_number(text: &str) -> Option<(&str, usize)> {
    let start = text
        .rfind(|character: char| !character.is_ascii_digit())
        .map_or(0, |index| index + 1);
    if start == text.len() {
        return None;
    }
    text.get(start..)?
        .parse()
        .ok()
        .map(|number| (&text[..start], number))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_changelog_is_read_only_above_its_first_release() {
        // A released section records what was true at that release. Checking it
        // would force a rewrite of shipped history every time the corpus grows.
        let changelog = "\
# Changelog

## [Unreleased]

- 109 of the 124 judgeable clauses.

## [0.4.0] - 2026-07-27

- 56 of the 124 judgeable clauses.
";
        assert_eq!(
            claims(unreleased(changelog))
                .iter()
                .map(|c| c.judged)
                .collect::<Vec<_>>(),
            vec![109]
        );
        // No release yet: the whole file is current.
        let fresh = "# Changelog\n\n## [Unreleased]\n";
        assert_eq!(unreleased(fresh), fresh);
    }

    /// The list is hand-kept, and a hand-kept list of what to check is the same
    /// hazard as a hand-kept count: a document added later falls outside it
    /// silently, which is exactly how the plan documents came to be unchecked.
    /// A directory this gate claims to cover is covered entirely, or this fails
    /// naming the file that was left out.
    #[test]
    fn every_living_document_in_a_covered_directory_is_listed() {
        let root = crate::workspace_root();
        // Top-level `docs/plan` only: `decisions/` are ADRs, dated by nature.
        let mut missed = Vec::new();
        for (directory, pattern) in [
            ("docs/plan", "*.md"),
            ("book/src", "*.md"),
            ("crates", "*/README.md"),
        ] {
            let Ok(entries) = glob(&root, directory, pattern) else {
                panic!("cannot read {directory}");
            };
            for path in entries {
                if !CLAIM_FILES.contains(&path.as_str()) {
                    missed.push(path);
                }
            }
        }
        assert!(
            missed.is_empty(),
            "living documents outside CLAIM_FILES: {missed:?}"
        );
    }

    /// Workspace-relative paths under `directory` matching `pattern`, which is
    /// either `*.md` (that directory only) or `*/README.md` (one level down).
    fn glob(root: &Path, directory: &str, pattern: &str) -> std::io::Result<Vec<String>> {
        let mut found = Vec::new();
        for entry in fs::read_dir(root.join(directory))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if pattern == "*.md" {
                if entry.path().extension().is_some_and(|ext| ext == "md") {
                    found.push(format!("{directory}/{name}"));
                }
            } else if entry.path().join("README.md").is_file() {
                found.push(format!("{directory}/{name}/README.md"));
            }
        }
        found.sort();
        Ok(found)
    }
}
