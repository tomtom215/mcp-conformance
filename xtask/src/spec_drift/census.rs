// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The normative census: how many RFC 2119 keywords each in-scope page carries,
//! and whether every one the registry claims sits inside an entry's quote.
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
//! green.
//!
//! **Two halves.** [`Tally`] counts each page's keyword instances per family
//! and [`reconcile`] compares the counts against the committed numbers, so a
//! clause added or removed upstream fails the run. [`uncovered`] then holds each
//! instance to the registry: a MUST-family or SHOULD-family keyword that no
//! entry citing the page quotes is a clause nobody decided.
//!
//! The count alone was the gate until 2026-10-13, and a count cannot tell an
//! entered clause from an unentered one. When this half was added,
//! `2026-07-28`'s "These headers are REQUIRED for compliance" had no entry —
//! `REQUIRED` was not even a counted keyword — and neither did twelve
//! `2025-11-25` SHOULD-family keywords (UI guidance, SSE polling, timeouts) or
//! three `2026-07-28` ones. The census was exact about how many clauses there
//! were and silent about which of them the registry held.
//!
//! **Which families, and why.** MUST-family (`MUST`, `MUST NOT`, `REQUIRED`,
//! `SHALL`, `SHALL NOT`) is rule 1: every instance enters. SHOULD-family
//! (`SHOULD`, `SHOULD NOT`, `RECOMMENDED`, `NOT RECOMMENDED`) is rule 2, and
//! the registry enters every one of those too — judged where the clause
//! constrains observable wire behavior, excluded with the reason where it binds
//! UI, internal policy or model interaction — so a reader can see each was
//! decided. MAY-family (`MAY`, `OPTIONAL`) is counted, so an edit to one still
//! asks for a re-read, but not held to an entry: a MAY grants latitude, so there
//! is nothing in it for a trace to violate (`Level::May` is "never a
//! violation"), and the registry enters none.
//!
//! **What this is not.** It does not segment clauses and match them to
//! entries; it asks only whether each keyword's position falls inside some
//! entry's quote on the same page. A quote spanning two clauses covers both, so
//! one entry may cover two keywords — `2026-07-28`'s BASE-068 quotes a SHOULD
//! and a SHOULD NOT in one sentence. Quotes are located in the same prose the
//! keywords are counted in (frontmatter and fenced code removed), so a quote
//! that ran across a code block would show its keywords as uncovered: loudly,
//! never silently.

use std::collections::BTreeMap;

use super::quote::{normalize, spans};

/// An RFC 2119 keyword family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// `MUST`, `MUST NOT`, `REQUIRED`, `SHALL`, `SHALL NOT` — rule 1.
    Must,
    /// `SHOULD`, `SHOULD NOT`, `RECOMMENDED`, `NOT RECOMMENDED` — rule 2.
    Should,
    /// `MAY`, `OPTIONAL` — counted, never required to have an entry.
    May,
}

impl Family {
    /// The committed `sources.json` member that holds this family's census.
    pub(super) const fn field(self) -> &'static str {
        match self {
            Self::Must => "must_census",
            Self::Should => "should_census",
            Self::May => "may_census",
        }
    }

    /// How a report line names the family.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Must => "MUST-family",
            Self::Should => "SHOULD-family",
            Self::May => "MAY-family",
        }
    }

    /// Whether every instance of the family must sit inside an entry's quote.
    pub(super) const fn must_be_entered(self) -> bool {
        !matches!(self, Self::May)
    }
}

/// Every RFC 2119 keyword with its family, longest first so `MUST NOT` is never
/// counted as a `MUST` followed by a word, nor `NOT RECOMMENDED` as a
/// `RECOMMENDED`.
const KEYWORDS: [(&str, Family); 11] = [
    ("NOT RECOMMENDED", Family::Should),
    ("SHOULD NOT", Family::Should),
    ("RECOMMENDED", Family::Should),
    ("SHALL NOT", Family::Must),
    ("MUST NOT", Family::Must),
    ("REQUIRED", Family::Must),
    ("OPTIONAL", Family::May),
    ("SHOULD", Family::Should),
    ("SHALL", Family::Must),
    ("MUST", Family::Must),
    ("MAY", Family::May),
];

/// One keyword instance in a page's prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Instance {
    /// Byte offset into [`prose`]'s output.
    pub at: usize,
    /// The keyword as it appears.
    pub keyword: &'static str,
    /// Its family.
    pub family: Family,
}

/// A page's keyword count per family.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Tally {
    pub must: u32,
    pub should: u32,
    pub may: u32,
}

impl Tally {
    /// The count for one family.
    pub(super) const fn get(self, family: Family) -> u32 {
        match family {
            Family::Must => self.must,
            Family::Should => self.should,
            Family::May => self.may,
        }
    }

    /// Counts the instances found in a page.
    pub(super) fn of(instances: &[Instance]) -> Self {
        let mut tally = Self::default();
        for instance in instances {
            match instance.family {
                Family::Must => tally.must += 1,
                Family::Should => tally.should += 1,
                Family::May => tally.may += 1,
            }
        }
        tally
    }
}

/// A page's prose in the form both keywords and quotes are located in:
/// frontmatter and fenced code removed, then normalized the way quotes are,
/// with the quote convention's `"; "` list joins relaxed to single spaces —
/// exactly the text `quote::spans` searches.
///
/// Markdown and MDX table rows stay: a keyword in a table cell is a clause,
/// and RFC 8174 makes only the uppercase keyword normative, so lowercase prose
/// uses of the words are ignored by construction.
pub(super) fn prose(page_text: &str) -> String {
    normalize(&strip_frontmatter_and_code(page_text)).replace("; ", " ")
}

/// Every keyword instance in `prose`, in order.
pub(super) fn instances(prose: &str) -> Vec<Instance> {
    let bytes = prose.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let matched = KEYWORDS
            .iter()
            .find(|(keyword, _)| starts_keyword_at(bytes, at, keyword));
        if let Some(&(keyword, family)) = matched {
            found.push(Instance {
                at,
                keyword,
                family,
            });
            at += keyword.len();
        } else {
            at += 1;
        }
    }
    found
}

/// Counts the keyword instances in a page, per family.
#[must_use]
pub(super) fn count(page_text: &str) -> Tally {
    Tally::of(&instances(&prose(page_text)))
}

/// The MUST-family and SHOULD-family instances in `prose` that no quote in
/// `quotes` covers, each rendered with the words around it so the line says
/// which clause to enter.
pub(super) fn uncovered<'q>(prose: &str, quotes: impl IntoIterator<Item = &'q str>) -> Vec<String> {
    let covered: Vec<(usize, usize)> = quotes
        .into_iter()
        .filter_map(|quote| spans(prose, quote))
        .flatten()
        .collect();
    instances(prose)
        .into_iter()
        .filter(|instance| instance.family.must_be_entered())
        .filter(|instance| {
            let end = instance.at + instance.keyword.len();
            !covered
                .iter()
                .any(|&(start, stop)| start <= instance.at && end <= stop)
        })
        .map(|instance| {
            format!(
                "{} {:?} has no entry: …{}…",
                instance.family.label(),
                instance.keyword,
                context(prose, instance.at, instance.keyword.len())
            )
        })
        .collect()
}

/// About ninety bytes either side of a keyword, cut on character boundaries.
fn context(prose: &str, at: usize, len: usize) -> &str {
    let mut start = at.saturating_sub(90);
    while !prose.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + len + 90).min(prose.len());
    while !prose.is_char_boundary(end) {
        end += 1;
    }
    &prose[start..end]
}

/// Whether `keyword` sits at `at` as a whole word.
///
/// A bare `starts_with` would count the `MUST` inside `MUSTARD`, and — more
/// realistically for this corpus — inside an anchor or identifier that happens
/// to contain it, such as `MISSING_REQUIRED_CLIENT_CAPABILITY`.
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
/// clause. Unlike that tool it keeps table rows: they carry clauses too. The
/// parent module's quote verification deliberately does *not* strip any of
/// this — a quote is checked against everything the page says — so this lives
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

/// Compares every page's live count of one family against the committed one.
///
/// # Errors
///
/// Returns one line per disagreeing page, naming both numbers and what to do.
pub(super) fn reconcile(
    revision: &str,
    family: Family,
    committed: &BTreeMap<String, u32>,
    live: &BTreeMap<String, Tally>,
) -> Result<u32, Vec<String>> {
    let (field, label) = (family.field(), family.label());
    let mut problems = Vec::new();
    for (page, tally) in live {
        let actual = tally.get(family);
        match committed.get(page) {
            None => problems.push(format!(
                "{revision}/{page}: {actual} {label} keyword(s) on the page and no \
                 `{field}` entry for it. Add one to sources.json once the page's \
                 clauses are entered."
            )),
            Some(expected) if *expected != actual => problems.push(format!(
                "{revision}/{page}: the page now carries {actual} {label} keyword(s), \
                 `{field}` says {expected}. Re-read the page: enter the new clause \
                 (or retire the removed one) and update the number in the same commit."
            )),
            Some(_) => {}
        }
    }
    for page in committed.keys() {
        if !live.contains_key(page) {
            problems.push(format!(
                "{revision}/{page}: has a `{field}` entry but is not an in-scope page."
            ));
        }
    }
    if problems.is_empty() {
        Ok(live.values().map(|tally| tally.get(family)).sum())
    } else {
        Err(problems)
    }
}

/// Prints one revision's census verdict — every family's reconciliation, and
/// every keyword `unentered` names — and returns how many problems it found.
pub(super) fn report(
    revision: &str,
    sources: &super::Sources,
    live: &BTreeMap<String, Tally>,
    unentered: &[String],
) -> u32 {
    let mut problems: Vec<String> = unentered
        .iter()
        .map(|line| {
            format!(
                "{line}\n  Enter the clause — a check when a recorded trace can judge it, \
                 an exclusion saying why it cannot — quoting it verbatim."
            )
        })
        .collect();
    let mut totals = Vec::new();
    for (family, committed) in sources.censuses() {
        match reconcile(revision, family, committed, live) {
            Ok(total) => totals.push(format!("{total} {}", family.label())),
            Err(found) => problems.extend(found),
        }
    }
    for problem in &problems {
        eprintln!("xtask: spec-drift — {problem}");
    }
    if problems.is_empty() {
        eprintln!(
            "xtask: spec-drift — {revision}: {} keyword(s) across {} in-scope page(s), \
             matching the committed census; every MUST-family and SHOULD-family one \
             inside an entry's quote",
            totals.join(", "),
            live.len()
        );
    }
    u32::try_from(problems.len()).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
