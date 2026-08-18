// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The prose half of `draft-coverage`: every "N of the M judgeable clauses"
//! claim in the shipped Markdown, checked against the committed reports.
//!
//! A generated table cannot stop a sentence three files away from disagreeing
//! with it, and that is how the counts drifted in the first place — the
//! numbers lived in narrative, where nothing could reach them. This reaches
//! them: the phrasing is fixed, the numbers are what vary, so any occurrence
//! of the phrase is parsed and required to name a real pair.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use std::fs;
use std::path::Path;

use super::{Capture, Summary};

/// The phrase a coverage claim is written in. Prose that says this must mean
/// it: `<judged> of the <judgeable> judgeable clauses`.
const CLAIM: &str = "judgeable clauses";

/// The Markdown a reader treats as current, and the only files whose claims
/// are checked.
///
/// A *dated* document is allowed to be stale: `docs/reports/` records what was
/// true on the day it was written, and a released `CHANGELOG` section records
/// what was true at that release — which is why only `CHANGELOG.md`'s
/// `Unreleased` section is scanned.
const CLAIM_FILES: &[&str] = &[
    "README.md",
    "CHANGELOG.md",
    "corpus/README.md",
    "crates/mcp-conformance-core/README.md",
    "crates/mcp-trace-validator/README.md",
    "crates/mcp-everything-server/README.md",
    "crates/mcp-reference-host/README.md",
];

/// Verifies every coverage claim in [`CLAIM_FILES`]; `true` when all agree.
pub(super) fn check(root: &Path, captures: &[Capture], summary: &Summary) -> bool {
    let allowed = summary.allowed();
    let mut ok = true;
    for name in CLAIM_FILES {
        let Ok(text) = fs::read_to_string(root.join(name)) else {
            eprintln!("xtask: draft-coverage — cannot read {name}");
            ok = false;
            continue;
        };
        let scanned = if *name == "CHANGELOG.md" {
            unreleased(&text)
        } else {
            &text
        };
        for claim in claims(scanned) {
            ok &= judge(name, &claim, summary, &allowed);
        }
        for verdict in verdicts(scanned) {
            ok &= judge_verdict(name, &verdict, captures);
        }
    }
    ok
}

/// One claim against the reports; `true` when it agrees, having said why not
/// when it does not.
fn judge(
    name: &str,
    claim: &Claim,
    summary: &Summary,
    allowed: &std::collections::BTreeSet<usize>,
) -> bool {
    if claim.judgeable != summary.judgeable {
        eprintln!(
            "xtask: draft-coverage — {name}:{} claims a judgeable total of {}; the reports say {}",
            claim.line, claim.judgeable, summary.judgeable
        );
        return false;
    }
    if !allowed.contains(&claim.judged) {
        eprintln!(
            "xtask: draft-coverage — {name}:{} claims {} of {} judgeable clauses; \
             no capture judged that many and the union is {}",
            claim.line, claim.judged, claim.judgeable, summary.observed
        );
        return false;
    }
    true
}

/// One capture's outcome counts as prose quotes them: `58 pass, 1 fail, 0
/// warn, 65 not observed, 148 excluded`.
///
/// `pass` and `fail` are what make it a verdict; the rest are optional because
/// prose quotes as much of the row as the sentence needs.
#[derive(Debug, Default, PartialEq, Eq)]
struct Verdict {
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
fn judge_verdict(name: &str, verdict: &Verdict, captures: &[Capture]) -> bool {
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

/// Every verdict quoted in `text`, with 1-based line numbers.
///
/// Scanned per table cell rather than per line, because a two-column
/// comparison table puts two captures' verdicts on one line. Fenced code
/// blocks are skipped: sample CLI output is an illustration of the tool's
/// format, not a claim about this corpus.
fn verdicts(text: &str) -> Vec<Verdict> {
    let mut found = Vec::new();
    let mut fenced = false;
    for (index, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // Bold markers land in different places across these rows; stripping
        // them first means the parser reads numbers and labels, not emphasis.
        let plain = line.replace('*', "");
        for cell in plain.split('|') {
            let (Some(pass), Some(fail)) = (labelled(cell, "pass"), labelled(cell, "fail")) else {
                continue;
            };
            found.push(Verdict {
                pass,
                fail,
                warn: labelled(cell, "warn"),
                not_observed: labelled(cell, "not observed"),
                excluded: labelled(cell, "excluded"),
                line: index + 1,
            });
        }
    }
    found
}

/// The number immediately before `label` in `cell`, when there is one.
///
/// The label must end a word, so `pass` does not match inside `passes` and a
/// sentence about clauses that "pass" without counting them is not a verdict.
fn labelled(cell: &str, label: &str) -> Option<u32> {
    let mut search = 0;
    while let Some(offset) = cell[search..].find(label) {
        let at = search + offset;
        search = at + label.len();
        if cell[search..]
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        if let Some((_, number)) = trailing_number(cell[..at].trim_end()) {
            return u32::try_from(number).ok();
        }
    }
    None
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

/// One `<judged> of the <judgeable> judgeable clauses` claim.
#[derive(Debug, PartialEq, Eq)]
struct Claim {
    judged: usize,
    judgeable: usize,
    line: usize,
}

/// Every coverage claim in `text`, with 1-based line numbers.
///
/// Hand-parsed backwards from the phrase rather than matched forwards, because
/// the numbers are what varies and the words are what is fixed: prose wraps
/// between them, bolds them, and puts `At ` or `evidences ` in front. Anything
/// that does not parse is not a claim and is left alone — this gate exists to
/// catch wrong numbers, not to police phrasing.
fn claims(text: &str) -> Vec<Claim> {
    let mut found = Vec::new();
    let mut search = 0;
    while let Some(offset) = text[search..].find(CLAIM) {
        let at = search + offset;
        search = at + CLAIM.len();
        let Some(claim) = parse_claim(&text[..at]) else {
            continue;
        };
        found.push(Claim {
            line: text[..at].matches('\n').count() + 1,
            ..claim
        });
    }
    found
}

/// Reads `<judged> of the <judgeable> ` off the end of `head`.
fn parse_claim(head: &str) -> Option<Claim> {
    let (head, judgeable) = trailing_number(head.trim_end())?;
    let head = head.trim_end().strip_suffix("of the")?;
    let (_, judged) = trailing_number(head.trim_end())?;
    Some(Claim {
        judged,
        judgeable,
        line: 0,
    })
}

/// Splits the ASCII digits off the end of `text`.
fn trailing_number(text: &str) -> Option<(&str, usize)> {
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
    fn claims_are_read_through_the_shapes_prose_actually_uses() {
        let text = "\
plain: 77 of the 124 judgeable clauses were judged.
bold: **109 of the 124 judgeable clauses** between them.
comma: At **89 of the 124 judgeable clauses, 0 fail** it leads.
wrapped: 59 of the 124
judgeable clauses are evidenced.
";
        let found = claims(text);
        assert_eq!(
            found.iter().map(|c| c.judged).collect::<Vec<_>>(),
            vec![77, 109, 89, 59]
        );
        assert!(found.iter().all(|c| c.judgeable == 124), "{found:?}");
        // Line numbers are 1-based and point at the phrase, so a wrapped claim
        // is reported on the line the reader would have to edit.
        assert_eq!(
            found.iter().map(|c| c.line).collect::<Vec<_>>(),
            vec![1, 2, 3, 5]
        );
    }

    #[test]
    fn text_that_is_not_a_claim_is_left_alone() {
        // Each of these mentions the phrase without stating a pair. Treating
        // any of them as a claim would make the gate fire on prose it has no
        // business judging, and the first is real: the registry's own count.
        for text in [
            "124 judgeable clauses exist at this revision.\n",
            "the judgeable clauses are listed above\n",
            "some of the judgeable clauses\n",
            "109 of the many judgeable clauses\n",
        ] {
            assert!(claims(text).is_empty(), "{text:?} parsed as a claim");
        }
    }

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

    #[test]
    fn a_number_is_read_off_the_end_or_not_at_all() {
        assert_eq!(trailing_number("of the 124"), Some(("of the ", 124)));
        assert_eq!(trailing_number("124"), Some(("", 124)));
        assert_eq!(trailing_number("of the "), None);
        assert_eq!(trailing_number(""), None);
    }

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
    fn sample_output_in_a_fenced_block_is_not_a_claim() {
        // The README prints the validator's own `totals:` line as an example of
        // the format. It is a different registry's numbers, and reading it as a
        // claim about this corpus would make the gate unsatisfiable.
        let text = "\
```text
totals: 11 pass, 1 fail, 1 warn, 88 excluded, 0 unsupported, 25 not observed
```
";
        assert!(verdicts(text).is_empty(), "{:?}", verdicts(text));
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
        let capture = super::super::tally(
            "c".to_owned(),
            &super::super::GoldenReport {
                totals: super::super::GoldenTotals { excluded: 0 },
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
