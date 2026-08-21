// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The normative census: how many MUSTs each in-scope page carries.
//!
//! The registry's first entry rule is the strongest claim it makes —
//! *"Every MUST / MUST NOT on an in-scope page enters, with checks when a
//! recorded trace can judge it, with a documented exclusion naming where it is
//! enforced when it cannot. No exceptions: that is the SEP-2484 floor"*
//! (03-conformance-strategy §What enters the registry). Until 2026-08-20
//! nothing enforced it.
//!
//! The sibling gate proves the quotes the registry *has* are still accurate. It
//! cannot see a clause the registry never had: a page can gain a MUST and every
//! committed quote still verifies, the page set still agrees, and the run stays
//! green. The fingerprint the drift gate prints per page would have shown the
//! page changed, but it is printed and never compared, so nothing acts on it.
//!
//! Two omissions were found by hand when this gate was written, both on
//! `2025-11-25`, both clauses restating an obligation the registry held under
//! another page's id and neither entered on its own page: `basic#schema-dialect`
//! ("Implementations MUST support at least 2020-12…", restating BASE-013 and
//! BASE-016) and `basic/lifecycle#version-negotiation` (the `<Note>` restating
//! TRAN-017's `MCP-Protocol-Version` header rule). They are BASE-082 and
//! LIFE-018.
//!
//! **What this gate is, and what it deliberately is not.** It counts normative
//! keyword instances in each page's prose and compares against a committed
//! number. It does *not* segment clauses and match them to entries: that
//! segmentation is genuinely hard — `tools/extract-clauses.py`'s own docstring
//! records two of its rules as "learned by the check failing first" — and a
//! completeness gate that cries wolf is worse than none. A count is coarse and
//! it is *exact*: a reworded clause keeps it, so the SUBS-005/006 reshuffle of
//! 2026-08-20 would not have fired this; a clause added or removed moves it,
//! which is precisely the event nobody was watching for. The failure is a
//! re-decide, in the deferral ledger's sense: read the page, enter or retire the
//! clause, and update the number in the same commit.

use std::collections::BTreeMap;

use super::quote::normalize;

/// The keywords rule 1 names, longest first so `MUST NOT` is never counted as a
/// `MUST` followed by a word.
///
/// Deliberately not the whole RFC 2119 vocabulary. `SHOULD` and `MAY` enter the
/// registry under rule 2, which is a judgment about wire-observability rather
/// than an absolute, so counting them would fire on guidance edits that rule 2
/// filters out anyway. Rule 1 is the one with "no exceptions" in it, and this is
/// the gate for rule 1.
const KEYWORDS: [&str; 4] = ["MUST NOT", "SHALL NOT", "MUST", "SHALL"];

/// Counts the normative keyword instances in a page's prose.
///
/// Frontmatter and fenced code are removed first: a `MUST` inside a JSON sample
/// or a schema excerpt is not a clause, and RFC 8174 makes only the uppercase
/// keyword normative, so lowercase prose uses of the word are ignored by
/// construction.
#[must_use]
pub(super) fn count(page_text: &str) -> u32 {
    let prose = normalize(&strip_frontmatter_and_code(page_text));
    let bytes = prose.as_bytes();
    let mut count = 0_u32;
    let mut at = 0;
    while at < bytes.len() {
        let matched = KEYWORDS
            .iter()
            .find(|keyword| starts_keyword_at(bytes, at, keyword));
        if let Some(keyword) = matched {
            count += 1;
            at += keyword.len();
        } else {
            at += 1;
        }
    }
    count
}

/// Whether `keyword` sits at `at` as a whole word.
///
/// A bare `starts_with` would count the `MUST` inside `MUSTARD`, and — more
/// realistically for this corpus — inside an anchor or identifier that happens
/// to contain it.
fn starts_keyword_at(bytes: &[u8], at: usize, keyword: &str) -> bool {
    let end = at + keyword.len();
    if end > bytes.len() || &bytes[at..end] != keyword.as_bytes() {
        return false;
    }
    let before_ok = at == 0 || !is_word_byte(bytes[at - 1]);
    let after_ok = end == bytes.len() || !is_word_byte(bytes[end]);
    before_ok && after_ok
}

const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The page with its YAML frontmatter and fenced code blocks removed.
///
/// Mirrors `tools/extract-clauses.py`'s function of the same name, for the same
/// reason: those regions are not prose, and a keyword inside them is not a
/// clause. The parent module's quote matching deliberately does *not* strip
/// them — a quote is checked against everything the page says — so this lives
/// here rather than there.
fn strip_frontmatter_and_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    if lines.peek().is_some_and(|line| line.trim_end() == "---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim_end() == "---" {
                break;
            }
        }
    }
    let mut in_code = false;
    for line in lines {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Compares every page's live count against the committed one.
///
/// # Errors
///
/// Returns one line per disagreeing page, naming both numbers and what to do.
pub(super) fn reconcile(
    revision: &str,
    committed: &BTreeMap<String, u32>,
    live: &BTreeMap<String, u32>,
) -> Result<u32, Vec<String>> {
    let mut problems = Vec::new();
    for (page, actual) in live {
        match committed.get(page) {
            None => problems.push(format!(
                "{revision}/{page}: {actual} MUST-family clause(s) on the page and no \
                 `must_census` entry for it. Add one to sources.json once the page's \
                 clauses are entered."
            )),
            Some(expected) if expected != actual => problems.push(format!(
                "{revision}/{page}: the page now carries {actual} MUST-family clause(s), \
                 `must_census` says {expected}. Re-read the page: enter the new clause \
                 (or retire the removed one) and update the number in the same commit."
            )),
            Some(_) => {}
        }
    }
    for page in committed.keys() {
        if !live.contains_key(page) {
            problems.push(format!(
                "{revision}/{page}: has a `must_census` entry but is not an in-scope page."
            ));
        }
    }
    if problems.is_empty() {
        Ok(live.values().sum())
    } else {
        Err(problems)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn counts_whole_uppercase_keywords_only() {
        assert_eq!(count("The client **MUST** send it."), 1);
        // MUST NOT is one clause, not a MUST plus a word.
        assert_eq!(count("The client **MUST NOT** send it."), 1);
        assert_eq!(count("A **MUST** here and a **MUST NOT** there."), 2);
        assert_eq!(count("**SHALL** and **SHALL NOT**"), 2);
        // RFC 8174: only the uppercase instance is normative.
        assert_eq!(count("the client must send it, and should too"), 0);
        // Not a substring of a longer word.
        assert_eq!(count("MUSTARD and SHALLOW and RE_MUST_X"), 0);
    }

    #[test]
    fn frontmatter_and_code_are_not_prose() {
        let page = "---\ntitle: MUST not count\n---\n\nThe client **MUST** send it.\n\n\
                    ```json\n{\"note\": \"MUST NOT count\"}\n```\n\nAnd a **MUST NOT** after.\n";
        assert_eq!(count(page), 2);
    }

    #[test]
    fn a_line_wrapped_keyword_still_counts_once() {
        // The published pages wrap at 80 columns, so a keyword and the words
        // around it are routinely split across lines; normalization joins them.
        assert_eq!(count("the client\n**MUST**\ninclude the header"), 1);
        assert_eq!(count("the client **MUST**\n**NOT** do that"), 1);
    }

    #[test]
    fn reconcile_names_both_numbers_and_the_missing_pages() {
        let committed = BTreeMap::from([("a".to_owned(), 3), ("gone".to_owned(), 1)]);
        let live = BTreeMap::from([("a".to_owned(), 4), ("new".to_owned(), 2)]);
        let problems = reconcile("r", &committed, &live).unwrap_err();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(
            problems
                .iter()
                .any(|p| p.contains("now carries 4") && p.contains("says 3"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("r/new") && p.contains("no `must_census` entry"))
        );
        assert!(
            problems
                .iter()
                .any(|p| p.contains("r/gone") && p.contains("not an in-scope page"))
        );

        let agreed = BTreeMap::from([("a".to_owned(), 3)]);
        assert_eq!(reconcile("r", &agreed, &agreed).unwrap(), 3);
    }
}
