// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a claim gate is allowed to read: Markdown with its code blanked out.
//!
//! [`super::verdicts`] already skipped fenced blocks, on the rule that sample
//! CLI output is an illustration of the tool's format rather than a claim about
//! this corpus. An inline span is that same rule at a smaller scale, and the
//! documents need it, because a project that records its own corrections quotes
//! numbers it is not asserting: "this line printed `23 pass, 0 fail, 0 warn, 88
//! excluded` and stopped" is history, and row 1.5i of the ecosystem register
//! exists to say what it *used* to claim. Without a way to mark a specimen, a
//! gate that reaches those documents would force the history out of them —
//! which is a worse document, not a safer one.
//!
//! So the marker is the one Markdown already has. Backticks mean "this is
//! literal text, quoted"; prose outside them is the document speaking in its own
//! voice, and only that is checked.

// `unreachable_pub` (rustc) and `redundant_pub_crate` (clippy nursery) make
// opposite demands about items in a binary crate's private modules; this
// follows the rustc lint and quiets the clippy one, per its known-problems note.
#![allow(clippy::redundant_pub_crate)]

/// `text` with every fenced block and code span blanked to spaces.
///
/// Blanking rather than deleting, and newlines kept: a line number computed on
/// the result still points at the line a reader would have to edit, which is
/// the only thing the caller reports.
pub(super) fn without_code(text: &str) -> String {
    spans(&fences(text))
}

/// Blanks the body of every fenced block, and the fence lines themselves.
fn fences(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        let fence = line.trim_start().starts_with("```");
        if fence || fenced {
            blank(&mut out, line);
        } else {
            out.push_str(line);
        }
        if fence {
            fenced = !fenced;
        }
    }
    out
}

/// Blanks the body of every inline code span, leaving its backticks in place.
///
/// A span opens on a run of backticks and closes on a run of the same length
/// (`CommonMark`'s rule), and never crosses a blank line — a paragraph break ends
/// the search, so one stray backtick blanks nothing rather than silently
/// switching the gate off for the rest of the file.
fn spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        rest = &rest[open..];
        let ticks = run(rest);
        out.push_str(&rest[..ticks]);
        rest = &rest[ticks..];
        let Some(close) = closing(rest, ticks) else {
            continue;
        };
        blank(&mut out, &rest[..close]);
        out.push_str(&rest[close..close + ticks]);
        rest = &rest[close + ticks..];
    }
    out.push_str(rest);
    out
}

/// The length of the backtick run `text` starts with.
fn run(text: &str) -> usize {
    text.len() - text.trim_start_matches('`').len()
}

/// The offset of the run of exactly `ticks` backticks that closes a span opened
/// at the start of `text`, when one exists before the paragraph ends.
fn closing(text: &str, ticks: usize) -> Option<usize> {
    let end = text.find("\n\n").unwrap_or(text.len());
    let mut at = 0;
    while let Some(offset) = text[at..end].find('`') {
        at += offset;
        let found = run(&text[at..]);
        if found == ticks {
            return Some(at);
        }
        at += found;
    }
    None
}

/// Appends `text` with every character but a newline replaced by a space.
fn blank(out: &mut String, text: &str) {
    out.extend(text.chars().map(|ch| if ch == '\n' { '\n' } else { ' ' }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property every caller depends on: masking moves no line.
    fn assert_lines_preserved(text: &str) {
        let masked = without_code(text);
        assert_eq!(
            masked.matches('\n').count(),
            text.matches('\n').count(),
            "{masked:?}"
        );
    }

    #[test]
    fn a_code_span_is_a_specimen_and_prose_is_a_claim() {
        let text = "printed `23 pass, 0 fail` and stopped, unlike 58 pass, 1 fail\n";
        let masked = without_code(text);
        assert!(!masked.contains("23 pass"), "{masked}");
        assert!(masked.contains("58 pass, 1 fail"), "{masked}");
        assert_lines_preserved(text);
    }

    #[test]
    fn a_span_closes_across_a_wrapped_line_but_not_across_a_paragraph() {
        // Prose wraps inside a span; the CHANGELOG entry that found this bug
        // wrapped in exactly this place.
        let wrapped = "printed `23 pass, 0 fail, 0 warn, 88 excluded, 14 not\napplicable` here\n";
        let masked = without_code(wrapped);
        assert!(!masked.contains("23 pass"), "{masked}");
        assert!(!masked.contains("applicable"), "{masked}");
        assert_lines_preserved(wrapped);
        // An unmatched backtick blanks nothing: a typo must not switch the gate
        // off for everything after it.
        let stray = "a ` typo\n\nlater: 58 pass, 1 fail\n";
        assert_eq!(without_code(stray), stray);
    }

    #[test]
    fn a_fenced_block_is_blanked_whole() {
        let text = "before 1 pass\n```text\ntotals: 11 pass, 1 fail\n```\nafter 2 pass\n";
        let masked = without_code(text);
        assert!(!masked.contains("11 pass"), "{masked}");
        assert!(masked.contains("before 1 pass"), "{masked}");
        assert!(masked.contains("after 2 pass"), "{masked}");
        assert_lines_preserved(text);
    }

    #[test]
    fn backticks_inside_a_fence_do_not_open_a_span_after_it() {
        // The fence lines are blanked before spans are matched, so the three
        // backticks that open and close a block cannot pair with prose ticks.
        let text = "```\n`x`\n```\n`23 pass, 0 fail` and 58 pass, 1 fail\n";
        let masked = without_code(text);
        assert!(!masked.contains("23 pass"), "{masked}");
        assert!(masked.contains("58 pass, 1 fail"), "{masked}");
        assert_lines_preserved(text);
    }

    #[test]
    fn a_double_backtick_span_closes_only_on_a_double() {
        // CommonMark's equal-length rule: the single tick inside is content.
        let text = "``a ` b 9 pass`` and 3 pass\n";
        let masked = without_code(text);
        assert!(!masked.contains("9 pass"), "{masked}");
        assert!(masked.contains("3 pass"), "{masked}");
        assert_lines_preserved(text);
    }

    #[test]
    fn multibyte_prose_survives_masking() {
        // Blanking is per character, not per byte; an em dash beside a span
        // must not be split into replacement characters.
        let text = "the legs separate — `41 / 0` — and 58 pass, 1 fail\n";
        let masked = without_code(text);
        assert!(masked.contains("separate —"), "{masked}");
        assert!(masked.contains("58 pass, 1 fail"), "{masked}");
        assert_lines_preserved(text);
    }
}
