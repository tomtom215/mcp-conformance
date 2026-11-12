// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the trace's recorder said it left out: a note on the report, never a
//! verdict.
//!
//! A recorder that forwards something it cannot hold — a message over its size
//! limit — leaves the trace a faithful record of less than happened, and says
//! so on the trace's closing event ([`CaptureNote`]). The verdict is a verdict
//! on what the trace holds, so the report does not invent a finding: it says
//! what the recorder reported and leaves the verdict and exit status alone.
//! `validate --require-complete-capture` is the opt-in way to make a trace
//! that does not say it is complete fail the run.
//!
//! The kinds are the recorder's own names, read from a file anyone can write,
//! so a kind this build does not know is listed under its own name and every
//! name is made printable before it reaches a terminal.

use mcp_conformance_core::trace::{CaptureNote, EventBody, TraceEvent};

/// Why the note is a note: the sentence every format prints under it.
pub const CAPTURE_WHY: &str = "The note is the recorder's own statement and nothing \
                               authenticates it; the verdict judges what the trace holds, so \
                               it is unchanged.";

/// The note a trace's recorder wrote: the `capture` member of the last
/// lifecycle event that carries one. A `capture` member on any other event is
/// not a recorder's closing statement and is not read.
pub(crate) fn find(events: &[TraceEvent]) -> Option<CaptureNote> {
    events
        .iter()
        .rev()
        .filter(|event| matches!(event.body, EventBody::Lifecycle { .. }))
        .find_map(|event| event.capture.clone())
}

/// What a kind `mcp-trace-capture` writes means, for a reader of the note.
pub(crate) fn gloss(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "oversized" => "over the recorder's size limit, forwarded and not recorded",
        "over_budget" => {
            "arrived while the recorder's buffer budget was spent, forwarded and not recorded"
        }
        "other_events" => {
            "server-sent events of a type other than `message`, which a trace cannot hold"
        }
        _ => return None,
    })
}

/// How many things the note says were left out, over every kind.
fn total(note: &CaptureNote) -> u64 {
    note.left_out()
        .fold(0_u64, |total, (_, count)| total.saturating_add(count))
}

/// The note's first sentence: `the recorder reports that the trace does not
/// hold 3 things the session carried`.
#[must_use]
pub fn summary(note: &CaptureNote) -> String {
    let total = total(note);
    format!(
        "the recorder reports that the trace does not hold {total} {} the session carried",
        if total == 1 { "thing" } else { "things" }
    )
}

/// Each kind that was left out, `kind: count`, then what it means when this
/// build knows: the lines every format lists, in name order.
#[must_use]
pub fn lines(note: &CaptureNote) -> Vec<String> {
    note.left_out()
        .map(|(kind, count)| {
            gloss(kind).map_or_else(
                || format!("{kind}: {count}"),
                |gloss| format!("{kind}: {count} ({gloss})"),
            )
        })
        .collect()
}

/// The note, above the totals, when it reports something left out: what was,
/// why it is a note and not a finding, then each kind on its own line. A note
/// that says nothing was left out writes nothing.
pub(crate) fn write_human(note: &CaptureNote, out: &mut String) {
    use std::fmt::Write as _;
    if note.is_complete() {
        return;
    }
    let _ = writeln!(out, "  NOTE  {}.", summary(note));
    let _ = writeln!(out, "        {CAPTURE_WHY}");
    for line in lines(note) {
        let _ = writeln!(out, "        {}", super::printable(&line));
    }
}

/// The note on one line: the summary, then each kind and its count. What
/// `JUnit` and SARIF carry.
#[must_use]
pub fn one_line(note: &CaptureNote) -> String {
    let kinds: Vec<String> = note
        .left_out()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect();
    format!("{}: {}", summary(note), kinds.join(", "))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mcp_conformance_core::trace::{Direction, LifecycleEvent, TransportKind};

    use super::*;

    fn event(seq: u64, body: EventBody, capture: Option<CaptureNote>) -> TraceEvent {
        let mut event = TraceEvent::new(seq, Direction::ServerToClient, TransportKind::Stdio, body);
        event.capture = capture;
        event
    }

    fn lifecycle(seq: u64, capture: Option<CaptureNote>) -> TraceEvent {
        event(
            seq,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose,
            },
            capture,
        )
    }

    #[test]
    fn the_note_is_the_last_lifecycle_events_and_only_a_lifecycle_events() {
        let first = note(&[("oversized", 1)]);
        let last = note(&[("over_budget", 2)]);
        assert_eq!(find(&[]), None);
        assert_eq!(find(&[lifecycle(0, None)]), None, "no member, no note");
        assert_eq!(
            find(&[lifecycle(0, Some(first.clone())), lifecycle(1, None)]),
            Some(first.clone()),
            "the last event that carries one"
        );
        assert_eq!(
            find(&[lifecycle(0, Some(first)), lifecycle(1, Some(last.clone()))]),
            Some(last)
        );
        // An empty note is a note: the recorder said nothing was left out.
        assert_eq!(
            find(&[lifecycle(0, Some(note(&[])))]),
            Some(CaptureNote::default())
        );
        // A member on a message is not a recorder's closing statement.
        let message = event(
            0,
            EventBody::Message {
                payload: serde_json::json!({}),
            },
            Some(note(&[("oversized", 9)])),
        );
        assert_eq!(find(&[message]), None);
    }

    fn note(counts: &[(&str, u64)]) -> CaptureNote {
        CaptureNote::new(
            counts
                .iter()
                .map(|&(kind, count)| (kind.to_owned(), count))
                .collect::<BTreeMap<_, _>>(),
        )
    }

    #[test]
    fn the_summary_counts_what_was_left_out_over_every_kind() {
        assert_eq!(
            summary(&note(&[("oversized", 2), ("over_budget", 1)])),
            "the recorder reports that the trace does not hold 3 things the session carried"
        );
        assert_eq!(
            summary(&note(&[("oversized", 1), ("other_events", 0)])),
            "the recorder reports that the trace does not hold 1 thing the session carried"
        );
    }

    #[test]
    fn counts_that_would_overflow_saturate_rather_than_wrap() {
        let forged = note(&[("a", u64::MAX), ("b", 5)]);
        assert!(summary(&forged).contains(&u64::MAX.to_string()));
    }

    #[test]
    fn each_kind_is_listed_in_name_order_with_its_meaning_when_known() {
        let lines = lines(&note(&[
            ("zzz-from-the-future", 4),
            ("oversized", 2),
            ("nothing", 0),
        ]));
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(
            lines[0].starts_with("oversized: 2 (over the recorder's size limit"),
            "{lines:?}"
        );
        assert_eq!(lines[1], "zzz-from-the-future: 4");
    }

    #[test]
    fn every_kind_the_capture_tool_writes_has_a_meaning() {
        for kind in ["oversized", "over_budget", "other_events"] {
            assert!(gloss(kind).is_some(), "{kind}");
        }
        assert!(gloss("anything else").is_none());
    }

    #[test]
    fn the_one_line_form_names_each_kind_and_count() {
        assert_eq!(
            one_line(&note(&[("oversized", 2), ("over_budget", 1)])),
            "the recorder reports that the trace does not hold 3 things the session carried: \
             over_budget 1, oversized 2"
        );
    }
}
