// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The claim shape: `109 of the 124 judgeable clauses`, as prose states what the
//! corpus evidences.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

use super::{Summary, trailing_number};

/// The phrase a coverage claim is written in. Prose that says this must mean
/// it: `<judged> of the <judgeable> judgeable clauses`.
const CLAIM: &str = "judgeable clauses";

/// One claim against the reports; `true` when it agrees, having said why not
/// when it does not.
pub(super) fn judge(
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

/// One `<judged> of the <judgeable> judgeable clauses` claim.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Claim {
    pub(super) judged: usize,
    pub(super) judgeable: usize,
    pub(super) line: usize,
}

/// Every coverage claim in `text`, with 1-based line numbers.
///
/// Hand-parsed backwards from the phrase rather than matched forwards, because
/// the numbers are what varies and the words are what is fixed: prose wraps
/// between them, bolds them, and puts `At ` or `evidences ` in front. Anything
/// that does not parse is not a claim and is left alone — this gate exists to
/// catch wrong numbers, not to police phrasing.
pub(super) fn claims(text: &str) -> Vec<Claim> {
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
    fn a_number_is_read_off_the_end_or_not_at_all() {
        assert_eq!(trailing_number("of the 124"), Some(("of the ", 124)));
        assert_eq!(trailing_number("124"), Some(("", 124)));
        assert_eq!(trailing_number("of the "), None);
        assert_eq!(trailing_number(""), None);
    }
}
