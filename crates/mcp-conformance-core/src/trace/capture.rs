// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A capture tool's account of what its trace leaves out.
//!
//! A recorder that cannot hold something the session carried — a message over
//! its size limit, an event it had no room to buffer — forwards it anyway
//! rather than break the session, and the trace is then a faithful record of
//! less than happened. Said only on a terminal, that loss is gone by the time
//! anyone judges the trace, and the verdict reads `pass` for a session whose
//! worst moment was not recorded. A [`CaptureNote`] makes the account part of
//! the trace: the `capture` member of its closing lifecycle event.
//!
//! The note is the recorder's own statement and nothing authenticates it. It
//! can say more was left out than it did (a trace is edited) or less (a trace
//! is forged), so it is evidence about a capture, never proof of one. A trace
//! without a note says nothing either way: hand-written traces, recordings by
//! tools that do not write one, and captures that ended without a closing
//! event (a killed process, a failed write) all look alike.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What a trace's recorder reports having left out, as of the moment it closed
/// the trace (the `capture` member of the closing lifecycle event).
///
/// Anything that crossed the wire *after* the close is not counted: the close
/// is the last event, and a note can only say what had happened by then.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CaptureNote {
    /// What the session carried that is not in the trace, counted by kind.
    ///
    /// The kinds are the recorder's own names (`mcp-trace-capture` writes
    /// `oversized`, `over_budget` and `other_events`), so a reader must accept
    /// a kind it does not know: it is still something left out. A kind that
    /// did not occur is left out of the map, so an empty map reports that
    /// nothing was.
    #[serde(default)]
    pub unrecorded: BTreeMap<String, u64>,
}

impl CaptureNote {
    /// A note reporting `unrecorded`.
    #[must_use]
    pub const fn new(unrecorded: BTreeMap<String, u64>) -> Self {
        Self { unrecorded }
    }

    /// Whether the note reports that nothing was left out: no kind, or only
    /// kinds counted at zero.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.unrecorded.values().all(|&count| count == 0)
    }

    /// The kinds that were left out, each with its count, in name order;
    /// those counted at zero are skipped.
    pub fn left_out(&self) -> impl Iterator<Item = (&str, u64)> {
        self.unrecorded
            .iter()
            .filter(|&(_, &count)| count > 0)
            .map(|(kind, &count)| (kind.as_str(), count))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::trace::{Direction, EventBody, LifecycleEvent, TraceEvent, TransportKind};

    fn note(counts: &[(&str, u64)]) -> CaptureNote {
        CaptureNote::new(
            counts
                .iter()
                .map(|&(kind, count)| (kind.to_owned(), count))
                .collect(),
        )
    }

    fn close_event(capture: Option<CaptureNote>) -> TraceEvent {
        let mut event = TraceEvent::new(
            9,
            Direction::ServerToClient,
            TransportKind::Stdio,
            EventBody::Lifecycle {
                event: LifecycleEvent::TransportClose,
            },
        );
        event.capture = capture;
        event
    }

    #[test]
    fn a_closing_event_carries_the_note_as_a_capture_member() {
        let event = close_event(Some(note(&[("oversized", 2), ("over_budget", 1)])));
        let line = serde_json::to_string(&event).unwrap();
        assert_eq!(
            line,
            r#"{"seq":9,"direction":"server-to-client","transport":"stdio","kind":"lifecycle","event":"transport-close","capture":{"unrecorded":{"over_budget":1,"oversized":2}}}"#
        );
        assert_eq!(serde_json::from_str::<TraceEvent>(&line).unwrap(), event);
    }

    #[test]
    fn an_event_without_a_note_is_written_exactly_as_it_always_was() {
        // Every trace written before the note existed has no `capture`; one
        // written without it must stay byte for byte what it was.
        let line = serde_json::to_string(&close_event(None)).unwrap();
        assert!(!line.contains("capture"), "{line}");
        let back: TraceEvent = serde_json::from_str(&line).unwrap();
        assert_eq!(back.capture, None);
    }

    #[test]
    fn a_note_with_nothing_in_it_is_a_complete_one_and_is_not_dropped() {
        // `{}` is a recorder saying it left nothing out; it must not read back
        // as the absence of a note, which says nothing.
        let line = r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"lifecycle","event":"transport-close","capture":{}}"#;
        let event: TraceEvent = serde_json::from_str(line).unwrap();
        assert_eq!(event.capture, Some(CaptureNote::default()));
        assert!(serde_json::to_string(&event).unwrap().contains("capture"));
    }

    #[test]
    fn a_note_of_the_wrong_shape_is_refused_rather_than_read_as_complete() {
        for bad in [
            r#""capture":5"#,
            r#""capture":{"unrecorded":[1]}"#,
            r#""capture":{"unrecorded":{"oversized":-1}}"#,
            r#""capture":{"unrecorded":{"oversized":"two"}}"#,
        ] {
            let line = format!(
                r#"{{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"lifecycle","event":"transport-close",{bad}}}"#
            );
            assert!(serde_json::from_str::<TraceEvent>(&line).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_note_is_complete_when_every_kind_is_counted_at_zero() {
        assert!(note(&[]).is_complete());
        assert!(note(&[("oversized", 0)]).is_complete());
        assert!(!note(&[("oversized", 0), ("over_budget", 1)]).is_complete());
        assert!(!note(&[("a kind nobody has heard of", 3)]).is_complete());
    }

    #[test]
    fn what_was_left_out_is_listed_in_name_order_without_the_zeros() {
        let note = note(&[("oversized", 2), ("none", 0), ("over_budget", 1)]);
        let listed: Vec<(&str, u64)> = note.left_out().collect();
        assert_eq!(listed, [("over_budget", 1), ("oversized", 2)]);
    }
}
