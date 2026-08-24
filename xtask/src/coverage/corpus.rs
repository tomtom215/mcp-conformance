// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The corpus README's golden-accounting table, verified against the corpus.
//!
//! `corpus/README.md` explains why the exclusion rows live in a per-revision
//! ledger rather than in every golden, and it argues the case with numbers: how
//! many goldens there are, how many rows the ledger holds, how many distinct
//! not-observed sets the goldens carry. Those were prose, and prose does not
//! recount itself.
//!
//! On 2026-08-21 all six were wrong. The paragraph said 53 shipped goldens and
//! 79 draft ones (57 and 80), 88 and 148 ledger rows (87 and 147), 28 and 67
//! distinct not-observed sets (29 and 69) — drift accumulated across several
//! changes, none of which had any reason to look here. The sibling gate
//! ([`super::book`]) had just caught the same class in the book's table, and
//! the README's own coverage block is generated for exactly this reason; this
//! paragraph was the third document stating counts the data owns, and the only
//! one with nothing watching it.
//!
//! Like [`super::book`] this verifies rather than generates: the prose, the
//! ordering and the argument are the author's, and only the numbers are the
//! corpus's.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The revisions the table has a column for, paired with the golden directory
/// holding that revision's reports. Structural rather than derived from the
/// registry set: these are directories on disk, present in both feature modes,
/// so the gate reads the same in both.
const COLUMNS: [(&str, &str); 2] = [("2025-11-25", "golden"), ("2026-07-28", "golden/draft")];

/// The row labels this gate knows how to measure, naming each table row's
/// leading cell verbatim.
const ROWS: [(&str, Metric); 3] = [
    ("Goldens", Metric::Goldens),
    ("Excluded rows the ledger holds", Metric::LedgerRows),
    (
        "Distinct not-observed sets across them",
        Metric::NotObservedSets,
    ),
];

#[derive(Clone, Copy)]
enum Metric {
    Goldens,
    LedgerRows,
    NotObservedSets,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../corpus")
}

/// Verifies every numeric cell of the README's accounting table.
///
/// # Errors
///
/// Returns a report naming each disagreeing cell, or the reason the table could
/// not be read — a renamed or deleted row fails rather than verifying nothing.
pub(super) fn verify() -> Result<String, String> {
    let root = corpus_root();
    let path = root.join("README.md");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;

    let mut problems = String::new();
    let mut checked = 0_u32;
    for (label, metric) in ROWS {
        let stated = row_cells(&text, label)
            .ok_or_else(|| format!("{}: no `| {label} |` row", path.display()))?;
        if stated.len() != COLUMNS.len() {
            return Err(format!(
                "{}: row `{label}` has {} value cell(s) for {} column(s)",
                path.display(),
                stated.len(),
                COLUMNS.len()
            ));
        }
        for ((revision, directory), cell) in COLUMNS.iter().zip(stated) {
            let actual = measure(&root, revision, directory, metric)?;
            checked += 1;
            if cell != actual {
                let _ = writeln!(
                    problems,
                    "  {label} / {revision}: the README says {cell}, the corpus has {actual}"
                );
            }
        }
    }
    if problems.is_empty() {
        Ok(format!(
            "corpus accounting table — {checked} cell(s) across {} revision(s) match the corpus",
            COLUMNS.len()
        ))
    } else {
        Err(format!(
            "{} states counts the corpus does not:\n{problems}\
             Edit the table; the corpus is the authority.",
            path.display()
        ))
    }
}

/// The numeric cells of the `| label | n | m |` row, when every value cell is a
/// bare integer. A row whose cells are prose yields `None` and is simply not one
/// of [`ROWS`].
fn row_cells(text: &str, label: &str) -> Option<Vec<u32>> {
    let prefix = format!("| {label} |");
    let line = text.lines().find(|line| line.starts_with(&prefix))?;
    line.trim_matches('|')
        .split('|')
        .skip(1)
        .map(|cell| cell.trim().parse::<u32>().ok())
        .collect()
}

/// The goldens of one revision: the `.json` files directly in its directory,
/// never the subdirectories, so the draft tree and the exclusion ledger cannot
/// be counted as the shipped revision's reports.
fn goldens(root: &Path, directory: &str) -> Result<Vec<PathBuf>, String> {
    let dir = root.join(directory);
    let entries = std::fs::read_dir(&dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?;
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    if found.is_empty() {
        return Err(format!(
            "{}: no goldens found; a gate that measured nothing would pass on anything",
            dir.display()
        ));
    }
    found.sort();
    Ok(found)
}

/// The `requirements` array of one report or ledger document.
fn requirements(path: &Path) -> Result<Vec<serde_json::Value>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let document: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    document
        .get("requirements")
        .and_then(|rows| rows.as_array())
        .cloned()
        .ok_or_else(|| format!("{}: no `requirements` array", path.display()))
}

fn measure(root: &Path, revision: &str, directory: &str, metric: Metric) -> Result<u32, String> {
    let count = match metric {
        Metric::Goldens => goldens(root, directory)?.len(),
        Metric::LedgerRows => {
            requirements(&root.join(format!("golden/exclusions/{revision}.json")))?.len()
        }
        Metric::NotObservedSets => {
            let mut sets: BTreeSet<Vec<String>> = BTreeSet::new();
            for path in goldens(root, directory)? {
                sets.insert(
                    requirements(&path)?
                        .iter()
                        .filter(|row| {
                            row.get("outcome").and_then(serde_json::Value::as_str)
                                == Some("not-observed")
                        })
                        .filter_map(|row| {
                            row.get("id")
                                .and_then(serde_json::Value::as_str)
                                .map(ToOwned::to_owned)
                        })
                        .collect(),
                );
            }
            sets.len()
        }
    };
    Ok(u32::try_from(count).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_readme_agrees_with_the_corpus() {
        // The gate, run against the real tree: the assertion that would have
        // caught all six numbers going stale.
        match verify() {
            Ok(message) => assert!(message.contains("match the corpus"), "{message}"),
            Err(problems) => panic!("{problems}"),
        }
    }

    #[test]
    fn a_row_of_bare_integers_parses_and_prose_does_not() {
        let table = "| | `a` | `b` |\n|---|---:|---:|\n| Goldens | 57 | 80 |\n\
                     | Where they live | authored | captured |\n";
        assert_eq!(row_cells(table, "Goldens"), Some(vec![57, 80]));
        assert_eq!(row_cells(table, "Where they live"), None);
        // A renamed row is an error, not a silent pass.
        assert_eq!(row_cells(table, "Nothing here"), None);
    }
}
