// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The verdict shape: `58 pass, 1 fail, 0 warn, 65 not observed, 148 excluded`,
//! as prose quotes one capture's row.
//!
//! Split from its sibling [`super::claim`] because the two shapes share nothing
//! but a number reader: a verdict is a tuple checked against the captures that
//! produced it, a claim is a pair checked against the corpus total.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use super::{Capture, trailing_number};

/// One capture's outcome counts as prose quotes them: `58 pass, 1 fail, 0
/// warn, 65 not observed, 148 excluded`.
///
/// `pass` and `fail` are what make it a verdict; the rest are optional because
/// prose quotes as much of the row as the sentence needs.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Verdict {
    pass: u32,
    fail: u32,
    warn: Option<u32>,
    not_observed: Option<u32>,
    excluded: Option<u32>,
    line: usize,
}

/// A verdict against the reports; `true` when some capture matches it.
///
/// Any capture, not a named one: the row's own table says which capture it
/// describes, and parsing Markdown structure to find out would buy precision
/// this does not need. A tuple no capture produced is wrong however it is
/// labelled — which is the failure that actually happened, twice, when the
/// not-observed fix changed every capture's numbers and the prose kept the old
/// ones.
pub(super) fn judge_verdict(name: &str, verdict: &Verdict, captures: &[Capture]) -> bool {
    let matched = captures.iter().any(|capture| {
        capture.pass == verdict.pass
            && capture.fail == verdict.fail
            && verdict.warn.is_none_or(|warn| warn == capture.warn)
            && verdict
                .not_observed
                .is_none_or(|count| count as usize == capture.not_observed.len())
            && verdict
                .excluded
                .is_none_or(|count| count == capture.excluded)
    });
    if !matched {
        eprintln!(
            "xtask: draft-coverage — {name}:{} quotes a verdict of {} pass, {} fail{}{}{} that no \
             committed report produced",
            verdict.line,
            verdict.pass,
            verdict.fail,
            verdict
                .warn
                .map_or_else(String::new, |n| format!(", {n} warn")),
            verdict
                .not_observed
                .map_or_else(String::new, |n| format!(", {n} not observed")),
            verdict
                .excluded
                .map_or_else(String::new, |n| format!(", {n} excluded")),
        );
    }
    matched
}

/// The outcome labels a verdict is written with, in the order prose writes
/// them. `pass` opens one and `fail` confirms it; the rest are optional.
const LABELS: [&str; 5] = ["pass", "fail", "warn", "not observed", "excluded"];

/// Every verdict quoted in `text`, with 1-based line numbers.
///
/// Scanned per table cell rather than per line, because a two-column
/// comparison table puts two captures' verdicts on one line — and *every*
/// verdict in a cell, because one cell holds several: register row 1.5i states
/// both servers' scores in a single sentence, and reading only the first would
/// have left the second unchecked while reporting the row as covered.
pub(super) fn verdicts(text: &str) -> Vec<Verdict> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        // Bold markers land in different places across these rows; stripping
        // them first means the parser reads numbers and labels, not emphasis.
        let plain = line.replace('*', "");
        for cell in plain.split('|') {
            found.extend(cell_verdicts(cell, index + 1));
        }
    }
    found
}

/// Every verdict in one cell, read left to right off its counted labels.
fn cell_verdicts(cell: &str, line: usize) -> Vec<Verdict> {
    let counts = counts(cell);
    let mut found = Vec::new();
    let mut index = 0;
    while index < counts.len() {
        let (label, pass) = counts[index];
        index += 1;
        if label != "pass" {
            continue;
        }
        // `pass` alone is not a verdict — prose says "89 clauses pass, and the
        // rest are not observed" — so the next counted label must be `fail`.
        let Some(&("fail", fail)) = counts.get(index) else {
            continue;
        };
        index += 1;
        let mut verdict = Verdict {
            pass,
            fail,
            line,
            ..Verdict::default()
        };
        while let Some(&(label, count)) = counts.get(index) {
            match label {
                "warn" => verdict.warn = Some(count),
                "not observed" => verdict.not_observed = Some(count),
                "excluded" => verdict.excluded = Some(count),
                // A second `pass` opens the next verdict rather than extending
                // this one.
                _ => break,
            }
            index += 1;
        }
        found.push(verdict);
    }
    found
}

/// Every `<number> <label>` pair in `cell`, in the order they appear.
///
/// A label must end a word, so `pass` does not match inside `passes`, and it
/// must carry a number immediately before it, so "65 clauses not observed"
/// counts nothing — the number there belongs to the clauses, not the label.
fn counts(cell: &str) -> Vec<(&'static str, u32)> {
    let mut found = Vec::new();
    let mut consumed = 0;
    for (at, _) in cell.char_indices() {
        if at < consumed {
            continue;
        }
        let Some(label) = LABELS.iter().find(|label| ends_word(cell, at, label)) else {
            continue;
        };
        consumed = at + label.len();
        if let Some((_, number)) = trailing_number(cell[..at].trim_end())
            && let Ok(count) = u32::try_from(number)
        {
            found.push((*label, count));
        }
    }
    found
}

/// Whether `label` starts at `at` in `cell` and ends a word there.
fn ends_word(cell: &str, at: usize, label: &str) -> bool {
    cell[at..].starts_with(label)
        && !cell[at + label.len()..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_is_read_out_of_the_row_prose_writes_it_in() {
        // The bold markers sit in a different place in each of these, which is
        // why they are stripped before parsing rather than matched around.
        let text = "\
| Our verdict | 58 pass, **1 fail**, 0 warn, 65 not observed, 148 excluded | \
**59 pass, 0 fail, 0 warn**, 65 not observed, 148 excluded |
scores **123 pass, 1 fail** against the older registry.
";
        let found = verdicts(text);
        assert_eq!(
            found
                .iter()
                .map(|v| (v.pass, v.fail, v.warn, v.not_observed, v.excluded))
                .collect::<Vec<_>>(),
            vec![
                (58, 1, Some(0), Some(65), Some(148)),
                (59, 0, Some(0), Some(65), Some(148)),
                (123, 1, None, None, None),
            ],
            "a two-column row carries two verdicts, and a partial quote is still one"
        );
        assert_eq!(found[2].line, 2);
    }

    #[test]
    fn every_verdict_in_a_cell_is_read_not_just_the_first() {
        // Register row 1.5i states both servers' scores in one sentence. Reading
        // only the first left the second unchecked while the row counted as
        // covered, which is worse than not reading the row at all.
        let text = "the registry separated them — 58 pass, 1 fail against the first \
and 59 pass, 0 fail against the second\n";
        assert_eq!(
            verdicts(text)
                .iter()
                .map(|v| (v.pass, v.fail))
                .collect::<Vec<_>>(),
            vec![(58, 1), (59, 0)]
        );
    }

    #[test]
    fn a_trailing_count_attaches_to_the_verdict_it_follows() {
        // Two verdicts, the first fully quoted: the optional counts must stop at
        // the next `pass` rather than drifting onto the wrong row.
        let text = "| 58 pass, 1 fail, 0 warn, 65 not observed, 148 excluded | 59 pass, 0 fail |\n";
        let found = verdicts(text);
        assert_eq!(
            found
                .iter()
                .map(|v| (v.pass, v.fail, v.warn, v.not_observed, v.excluded))
                .collect::<Vec<_>>(),
            vec![
                (58, 1, Some(0), Some(65), Some(148)),
                (59, 0, None, None, None)
            ]
        );
        // And within one cell, where no `|` separates them.
        let run = "58 pass, 1 fail, 0 warn then 59 pass, 0 fail, 2 warn\n";
        assert_eq!(
            verdicts(run)
                .iter()
                .map(|v| (v.pass, v.fail, v.warn))
                .collect::<Vec<_>>(),
            vec![(58, 1, Some(0)), (59, 0, Some(2))]
        );
    }

    #[test]
    fn a_number_that_belongs_to_the_noun_is_not_a_count() {
        // "65 clauses not observed" counts clauses, not the label; the register
        // and the strategy document both write it that way.
        let text = "58 pass, 1 fail, with 65 clauses not observed on each\n";
        let found = verdicts(text);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].not_observed, None, "{found:?}");
    }

    #[test]
    fn prose_that_names_no_count_is_not_a_verdict() {
        for text in [
            // No number before either label.
            "clauses that pass, and those that fail, are both judged\n",
            // A count on one label only — half a verdict is not one.
            "with 0 fail on the conforming captures\n",
            "89 clauses pass, and the rest are not observed\n",
            // The label must end a word.
            "3 passes, 1 failure\n",
        ] {
            assert!(verdicts(text).is_empty(), "{text:?} parsed as a verdict");
        }
    }

    #[test]
    fn a_verdict_matches_only_a_capture_that_produced_it() {
        let capture = crate::draft_coverage::tally(
            "c".to_owned(),
            &crate::draft_coverage::GoldenReport {
                totals: crate::draft_coverage::GoldenTotals { excluded: 0 },
                requirements: vec![],
            },
        );
        // The empty capture is 0/0/0 with nothing excluded, so it matches a
        // zero verdict and nothing else.
        let zero = Verdict::default();
        assert!(judge_verdict("f.md", &zero, &[capture]));
        let one_pass = Verdict {
            pass: 1,
            ..Verdict::default()
        };
        assert!(!judge_verdict("f.md", &one_pass, &[]));
    }
}
