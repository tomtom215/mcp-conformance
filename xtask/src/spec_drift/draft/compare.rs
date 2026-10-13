// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The comparisons `spec-drift --draft` reports, as pure functions over text
//! and page sets, so every one of them is tested offline.
//!
//! **Present** means exactly what the release gate means: the quote verifies
//! under [`quote_present`], the normalization `SourceRef::quote` documents. A
//! quote that does not is either **changed** — some passage of the draft page
//! still carries most of its words, and that passage is shown — or
//! **removed**: nothing on the page comes close. The split between those two
//! is a heuristic and says so; "present" is not. A reader acting on the report
//! reads the page either way: "changed" only saves finding where to look.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::super::census::{Family, Tally};
use super::super::pages::canonical;
use super::super::quote::{normalize, quote_present, spans};

/// How much of a fragment's wording a passage must share to count as the same
/// clause reworded rather than a clause gone, in percent of its words.
///
/// Measured 2026-10-13 against the live draft: deleting each of the 309
/// `2026-07-28` quotes that sit contiguously on their page, with the page's
/// other quotes masked as [`classify`] masks them, left no passage sharing more
/// than 64% of the deleted quote's words; the one real rewording the draft
/// carried (TRAN-081) shares 92%. Without the masking, 39 of the 309 would
/// have read as "changed" on the strength of a sibling clause.
pub(super) const CHANGED_FLOOR: u32 = 70;

/// What became of one released quote on the draft page that carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Status {
    /// The quote verifies verbatim, as the release gate verifies it.
    Present,
    /// Not verbatim, but a passage shares most of its words: `closest` is the
    /// draft's wording of the fragment that matched least, `shared` how much
    /// of it survived, in percent.
    Changed { closest: String, shared: u32 },
    /// No passage on the draft page shares [`CHANGED_FLOOR`] of its words.
    Removed { shared: u32 },
    /// The draft no longer publishes the page the quote cites.
    PageRemoved,
}

/// Classifies one quote against a draft page's normalized text.
///
/// `others` are the quotes of the page's other entries: the passages they
/// still match verbatim are masked before the closest passage is sought,
/// because a page states parallel clauses in parallel words, and a deleted
/// clause's surviving sibling is not its rewording.
pub(super) fn classify<'q>(
    page_normalized: &str,
    quote: &str,
    others: impl IntoIterator<Item = &'q str>,
) -> Status {
    if quote_present(page_normalized, quote) {
        return Status::Present;
    }
    let page = page_normalized.replace("; ", " ");
    let masked = mask(&page, others);
    let page_tokens: Vec<&str> = masked.split_whitespace().collect();
    let mut worst: Option<(u32, String)> = None;
    for fragment in fragments(&page, &normalize(quote)) {
        if page.contains(fragment.as_str()) {
            continue;
        }
        let (shared, passage) = closest(&page_tokens, &fragment);
        if worst.as_ref().is_none_or(|(least, _)| shared < *least) {
            worst = Some((shared, passage));
        }
    }
    match worst {
        // Every fragment verbatim but the whole not: the items moved apart or
        // were reordered past what the fragment rule accepts.
        None => Status::Changed {
            closest: String::new(),
            shared: 100,
        },
        Some((shared, closest)) if shared >= CHANGED_FLOOR => Status::Changed { closest, shared },
        Some((shared, _)) => Status::Removed { shared },
    }
}

/// `page` with every passage an `others` quote matches replaced by a token no
/// quote contains, so no window can count words from it.
fn mask<'q>(page: &str, others: impl IntoIterator<Item = &'q str>) -> String {
    let mut ranges: Vec<(usize, usize)> = others
        .into_iter()
        .filter_map(|quote| spans(page, quote))
        .flatten()
        .collect();
    ranges.sort_unstable();
    let mut out = String::with_capacity(page.len());
    let mut at = 0;
    for (start, end) in ranges {
        if start >= at {
            out.push_str(&page[at..start]);
            out.push_str(" \u{1} ");
            at = end;
        } else if end > at {
            at = end;
        }
    }
    out.push_str(&page[at..]);
    out
}

/// The verbatim units of a normalized quote, the way `quote::spans` splits
/// them: an introducing clause ending `:` whose intro still occurs on the page
/// is a unit of its own, then each `"; "`-separated item is one.
fn fragments(page: &str, quote: &str) -> Vec<String> {
    let mut units = Vec::new();
    let rest = match quote.split_once(": ") {
        Some((intro, items)) if page.contains(&format!("{intro}:")) => {
            units.push(format!("{intro}:"));
            items
        }
        _ => quote,
    };
    units.extend(rest.split("; ").map(str::to_owned));
    units
}

/// The passage of `page` sharing the most words with `fragment`, as (percent
/// of the fragment's words it shares, the passage trimmed to its first and
/// last shared word). Windows run half again the fragment's length, so a
/// clause that gained words is still found whole.
fn closest(page: &[&str], fragment: &str) -> (u32, String) {
    let wanted: Vec<&str> = fragment.split_whitespace().collect();
    if wanted.is_empty() || page.is_empty() {
        return (0, String::new());
    }
    let mut need: HashMap<&str, u32> = HashMap::new();
    for word in &wanted {
        *need.entry(word).or_default() += 1;
    }
    let width = (wanted.len() * 3).div_ceil(2).min(page.len());
    let mut have: HashMap<&str, u32> = HashMap::new();
    let mut matched = 0u32;
    let mut best = (0u32, 0usize);
    for (index, word) in page.iter().enumerate() {
        let count = have.entry(word).or_default();
        if *count < need.get(word).copied().unwrap_or(0) {
            matched += 1;
        }
        *count += 1;
        if index >= width {
            let leaving = page[index - width];
            let count = have.entry(leaving).or_default();
            *count -= 1;
            if *count < need.get(leaving).copied().unwrap_or(0) {
                matched -= 1;
            }
        }
        if index + 1 >= width && matched > best.0 {
            best = (matched, index + 1 - width);
        }
    }
    let window = &page[best.1..best.1 + width];
    let first = window.iter().position(|word| need.contains_key(word));
    let last = window.iter().rposition(|word| need.contains_key(word));
    let passage = match (first, last) {
        (Some(first), Some(last)) => window[first..=last].join(" "),
        _ => String::new(),
    };
    let total = u32::try_from(wanted.len()).unwrap_or(u32::MAX);
    (best.0 * 100 / total, passage)
}

/// How the draft's published page set differs from the skeleton's lists.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct PageDelta {
    /// Published by the draft, in neither of the skeleton's lists.
    pub added: Vec<String>,
    /// In one of the skeleton's lists, no longer published by the draft.
    pub removed: Vec<String>,
}

/// Both directions, on canonical page names (`basic/index` is `basic`).
pub(super) fn page_delta<'p>(
    listed: impl IntoIterator<Item = &'p String>,
    live: &BTreeSet<String>,
) -> PageDelta {
    let listed: BTreeSet<&str> = listed.into_iter().map(|page| canonical(page)).collect();
    PageDelta {
        added: live
            .iter()
            .filter(|page| !listed.contains(page.as_str()))
            .cloned()
            .collect(),
        removed: listed
            .into_iter()
            .filter(|page| !live.contains(*page))
            .map(str::to_owned)
            .collect(),
    }
}

/// One family's count on one page that is not what the skeleton committed.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct CensusMove {
    pub page: String,
    pub family: Family,
    /// The skeleton's number, `None` when it has none for the page.
    pub committed: Option<u32>,
    pub live: u32,
}

/// Every family's live count against the skeleton's, page by page.
pub(super) fn census_moves(
    committed: [(Family, &BTreeMap<String, u32>); 3],
    live: &BTreeMap<String, Tally>,
) -> Vec<CensusMove> {
    let mut moves = Vec::new();
    for (page, tally) in live {
        for (family, numbers) in committed {
            let committed = numbers.get(page).copied();
            if committed != Some(tally.get(family)) {
                moves.push(CensusMove {
                    page: page.clone(),
                    family,
                    committed,
                    live: tally.get(family),
                });
            }
        }
    }
    moves
}
