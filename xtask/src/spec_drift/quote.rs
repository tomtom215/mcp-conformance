// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Quote matching: the normalization a registry quote is written against.
//!
//! Split from the parent module for the 500-line file cap
//! (`docs/plan/04-engineering-standards.md`), at a real seam — the parent decides
//! *what* to verify and fetches it, this decides whether a quote is *present*. The
//! rules here are the contract `SourceRef::quote` documents, so they are the reason a
//! hand-written quote either passes the gate or does not, and they are ported verbatim
//! by `tools/extract-clauses.py` (calibrated against all 140 shipped `2025-11-25`
//! quotes).

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this follows
// the rustc lint and quiets the clippy one, per its own known-problems note.
#![allow(clippy::redundant_pub_crate)]

/// The normalization `SourceRef::quote` documents, applied to page text and
/// quotes alike: markdown bullet/number markers dropped, bold markers
/// dropped, typographic quotes straightened, whitespace runs collapsed —
/// and the quote convention's `"; "` list joins relaxed to single spaces on
/// both sides before matching.
pub(crate) fn normalize(text: &str) -> String {
    let mut joined = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        let without_marker = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| strip_numbered_marker(trimmed))
            .unwrap_or(trimmed);
        joined.push(' ');
        joined.push_str(without_marker);
    }
    let unstyled = strip_italics(&unwrap_links(&joined).replace("**", "").replace("\\_", "_"))
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .replace('\u{2019}', "'");
    let mut collapsed = String::with_capacity(unstyled.len());
    let mut last_space = false;
    for ch in unstyled.chars() {
        if ch.is_whitespace() {
            if !last_space {
                collapsed.push(' ');
            }
            last_space = true;
        } else {
            collapsed.push(ch);
            last_space = false;
        }
    }
    collapsed.trim().to_owned()
}

/// Replaces every markdown link `[text](target)` with its text — quotes cite
/// the rendered words, and links may span source lines (handled because
/// unwrapping runs after line joining).
pub(super) fn unwrap_links(text: &str) -> String {
    let mut out = text.to_owned();
    loop {
        let Some(mid) = out.find("](") else {
            return out;
        };
        let Some(open) = out[..mid].rfind('[') else {
            return out;
        };
        let Some(close_rel) = out[mid + 2..].find(')') else {
            return out;
        };
        let close = mid + 2 + close_rel;
        let mut next = String::with_capacity(out.len());
        next.push_str(&out[..open]);
        next.push_str(&out[open + 1..mid]);
        next.push_str(&out[close + 1..]);
        out = next;
    }
}

/// Drops `_italic_` markers while keeping identifier underscores: an
/// underscore is a marker when a word character sits on exactly one side of
/// it — `_latest_` loses both, `list_changed` keeps its underscore (word
/// characters on both sides), and the rendered `(_)` keeps it (word
/// characters on neither side). Runs after escape unwrapping so MDX's
/// literal `\_` has already become a plain underscore.
pub(super) fn strip_italics(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (index, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            let prev_word = index > 0 && chars[index - 1].is_alphanumeric();
            let next_word = chars.get(index + 1).is_some_and(|c| c.is_alphanumeric());
            if prev_word != next_word {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// `1. ` / `12. ` ordered-list markers.
pub(super) fn strip_numbered_marker(line: &str) -> Option<&str> {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    line.get(digits..)?.strip_prefix(". ")
}

/// Whether `quote` appears in the normalized page text: as one contiguous
/// run when it can, otherwise fragment-by-fragment on the `"; "` separators —
/// `SourceRef::quote`'s documented convention flattens lists and may keep
/// only the normative items, so the fragments are the verbatim units. The
/// fragment path cannot detect reordering, only rewording; the contiguous
/// path is tried first and covers every single-sentence quote.
pub(super) fn quote_present(page_normalized: &str, quote: &str) -> bool {
    let relaxed_page = page_normalized.replace("; ", " ");
    let normalized_quote = normalize(quote);
    if relaxed_page.contains(&normalized_quote.replace("; ", " ")) {
        return true;
    }
    if normalized_quote
        .split("; ")
        .all(|fragment| !fragment.is_empty() && relaxed_page.contains(fragment))
    {
        return true;
    }
    // The convention's full shape: an introducing clause ending `:` whose
    // selected items follow. Verify the intro (with its colon) and each item
    // independently — LIFE-009 quotes the parent plus one of its bullets.
    if let Some((intro, items)) = normalized_quote.split_once(": ") {
        let intro_present = relaxed_page.contains(&format!("{intro}:"));
        let items_present = items
            .split("; ")
            .all(|fragment| !fragment.is_empty() && relaxed_page.contains(fragment));
        if intro_present && items_present {
            return true;
        }
    }
    false
}
