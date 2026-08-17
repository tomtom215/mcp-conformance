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

use super::Summary;

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
pub(super) fn check(root: &Path, summary: &Summary) -> bool {
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
}
