// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a baseline says about the run itself, as a suite of its own: the
//! entries that match no finding, and the ones whose day has passed.
//!
//! Those are facts about the baseline file, not about any trace, and a CI that
//! ingests this document has no terminal to read them from — so a stale entry
//! is a failing test case when it fails the run, as it does the exit status.

use core::fmt::Write as _;

use super::text::escape;
use crate::baseline::Notes;

/// `(tests, failures, skipped)` of the notes' suite: a stale entry is a failure
/// unless stale entries are allowed, and every other note is skipped.
pub(super) fn note_counts(notes: &Notes) -> (u32, u32, u32) {
    let total = u32::try_from(notes.stale.len() + notes.lapsed.len()).unwrap_or(u32::MAX);
    let failures = if notes.allow_stale {
        0
    } else {
        u32::try_from(notes.stale.len()).unwrap_or(u32::MAX)
    };
    (total, failures, total - failures)
}

/// The notes as a `<testsuite name="baseline">`; nothing when there are none.
pub(super) fn render_notes(out: &mut String, notes: &Notes) {
    if notes.is_empty() {
        return;
    }
    let (tests, failures, skipped) = note_counts(notes);
    let _ = writeln!(
        out,
        r#"  <testsuite name="baseline" tests="{tests}" failures="{failures}" skipped="{skipped}">"#
    );
    for entry in &notes.stale {
        let name = escape(&format!("stale baseline entry: {}", entry.fingerprint));
        let message = escape(&format!(
            "no finding of this run matches this entry, so the finding it accepted no longer \
             occurs; remove it (its reason was: {})",
            entry.reason
        ));
        let verdict = if notes.allow_stale {
            "skipped"
        } else {
            "failure"
        };
        let _ = writeln!(
            out,
            r#"    <testcase classname="baseline" name="{name}"><{verdict} message="{message}"/></testcase>"#
        );
    }
    for entry in &notes.lapsed {
        let name = escape(&format!("lapsed baseline entry: {}", entry.fingerprint));
        let until = entry
            .expires
            .map_or_else(String::new, |day| format!(" on {day}"));
        let message = escape(&format!(
            "this entry expired{until}: it accepts nothing, and its finding counts (its reason \
             was: {})",
            entry.reason
        ));
        let _ = writeln!(
            out,
            r#"    <testcase classname="baseline" name="{name}"><skipped message="{message}"/></testcase>"#
        );
    }
    out.push_str("  </testsuite>\n");
}
