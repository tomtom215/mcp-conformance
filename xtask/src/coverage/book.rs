// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The book's per-revision registry table, verified against the registries.
//!
//! `book/src/revisions.md` opens with a three-row comparison — entries, clauses
//! judged by a check, clauses carrying an exclusion — for both revisions. Unlike
//! the README's table it is prose a human wrote, and on 2026-08-20 it was found
//! two revisions stale: it still read `52 | 124` and `88 | 148` after a change
//! moved two clauses out of exclusion and into judgment. Nothing had said so,
//! because the README block is generated and this one was not, and the
//! `draft-coverage` gate reads verdict tuples and "N of the M judgeable clauses"
//! claims — neither shape matches a table cell.
//!
//! ADR-0001's rule is that a count in prose is a count that rots, so this
//! verifies rather than trusts. It is a check, not a generator: the table's
//! wording, ordering and extra rows are the author's, and only the numbers are
//! the registry's.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use mcp_conformance_core::requirement::{RegistrySet, Verification};
use mcp_conformance_core::revision::ProtocolRevision;

/// The row labels this gate knows how to derive, in the order the table states
/// them. Each names the table's leading cell verbatim.
const ROWS: [(&str, Metric); 3] = [
    ("Registry entries", Metric::Entries),
    ("Judged by a named check", Metric::Checked),
    ("Carrying a documented exclusion", Metric::Excluded),
];

#[derive(Clone, Copy)]
enum Metric {
    Entries,
    Checked,
    Excluded,
}

/// The chapter, located relative to this crate so the task works from any
/// working directory inside the workspace.
fn chapter_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../book/src/revisions.md")
}

/// Verifies every numeric cell of the chapter's table.
///
/// # Errors
///
/// Returns a human-readable report naming each disagreeing cell, or the reason
/// the table could not be read at all — a chapter whose table has been renamed
/// or removed fails rather than silently verifying nothing.
pub(super) fn verify() -> Result<String, String> {
    let path = chapter_path();
    let set = RegistrySet::builtin().map_err(|error| format!("registry set: {error}"))?;
    let revisions = set.revisions().to_vec();
    if revisions.len() < 2 {
        return Err(
            "the registry set describes fewer than two revisions, so the chapter's \
             two-column table cannot be checked; run this task through the `cargo xtask` \
             alias, which enables the draft feature"
                .to_owned(),
        );
    }
    let text = read(&path)?;

    let mut problems = String::new();
    let mut checked = 0_u32;
    for (label, metric) in ROWS {
        let stated = row_cells(&text, label)
            .ok_or_else(|| format!("{}: no `| {label} |` row", path.display()))?;
        if stated.len() != revisions.len() {
            return Err(format!(
                "{}: row `{label}` has {} value cell(s) for {} revision(s)",
                path.display(),
                stated.len(),
                revisions.len()
            ));
        }
        for (revision, cell) in revisions.iter().zip(stated) {
            let actual = measure(&set, *revision, metric);
            checked += 1;
            if cell != actual {
                let _ = writeln!(
                    problems,
                    "  {label} / {revision}: the chapter says {cell}, the registry has {actual}"
                );
            }
        }
    }
    if problems.is_empty() {
        Ok(format!(
            "book revisions table — {checked} cell(s) across {} revision(s) match the registries",
            revisions.len()
        ))
    } else {
        Err(format!(
            "{} states counts the registries do not:\n{problems}\
             Edit the table; the registries are the authority.",
            path.display()
        ))
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// The numeric cells of the `| label | n | m |` row, when every value cell is a
/// bare integer. A row whose cells are prose (`Shipped by default`) yields
/// `None` here and is simply not one of [`ROWS`].
fn row_cells(text: &str, label: &str) -> Option<Vec<u32>> {
    let prefix = format!("| {label} |");
    let line = text.lines().find(|line| line.starts_with(&prefix))?;
    line.trim_matches('|')
        .split('|')
        .skip(1)
        .map(|cell| cell.trim().parse::<u32>().ok())
        .collect()
}

fn measure(set: &RegistrySet, revision: ProtocolRevision, metric: Metric) -> u32 {
    let Some(registry) = set.registry(revision) else {
        return 0;
    };
    let requirements = registry.requirements();
    let count = match metric {
        Metric::Entries => requirements.len(),
        Metric::Checked => requirements
            .iter()
            .filter(|requirement| matches!(requirement.verification, Verification::Checks { .. }))
            .count(),
        Metric::Excluded => requirements
            .iter()
            .filter(|requirement| matches!(requirement.verification, Verification::Excluded { .. }))
            .count(),
    };
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The chapter's table has a column per shipped revision, so this needs the
    // draft feature to have both. The `cargo xtask` alias turns it on, which is
    // how the gate itself always runs; a plain `cargo test -p xtask` does not.
    #[test]
    #[cfg(feature = "draft-2026-07-28")]
    fn the_committed_chapter_agrees_with_the_registries() {
        // The gate, run against the real tree: this is the assertion that would
        // have caught the table going stale.
        match verify() {
            Ok(message) => assert!(message.contains("match the registries"), "{message}"),
            Err(problems) => panic!("{problems}"),
        }
    }

    #[test]
    fn a_row_of_bare_integers_parses_and_prose_does_not() {
        let table = "| | `a` | `b` |\n|---|---:|---:|\n| Registry entries | 140 | 272 |\n\
                     | Shipped by default | yes | no |\n";
        assert_eq!(row_cells(table, "Registry entries"), Some(vec![140, 272]));
        assert_eq!(row_cells(table, "Shipped by default"), None);
        assert_eq!(row_cells(table, "Nothing here"), None);
    }

    #[test]
    fn a_renamed_row_fails_rather_than_verifying_nothing() {
        // The failure mode a substring search would have: a table that no
        // longer states a metric must be an error, not a silent pass.
        assert_eq!(row_cells("| Other | 1 | 2 |", "Registry entries"), None);
    }
}
