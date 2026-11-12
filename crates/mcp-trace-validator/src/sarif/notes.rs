// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The notes a trace carries rather than findings: what it ended without
//! answering, and what its recorder says it left out. Each is a `note`-level
//! notification located at the trace, written once per trace — the reports of
//! one trace judged under several revisions all carry the same note.

use mcp_conformance_core::trace::CaptureNote;
use serde_json::json;

use super::{Artifact, Log, location};

impl Log<'_> {
    /// The requests a trace ended without answering: a `note`-level
    /// notification located at the trace, once per trace — the reports of one
    /// trace judged under several revisions all carry the same note.
    pub(super) fn unanswered(
        &mut self,
        unanswered: &crate::report::UnansweredRequests,
        artifact: Artifact<'_>,
    ) {
        let text = unanswered.one_line();
        let key = (artifact.uri.unwrap_or_default().to_owned(), text.clone());
        if !self.noted.insert(key) {
            return;
        }
        self.notifications.push(json!({
            "level": "note",
            "message": { "text": text },
            "locations": location(artifact, None),
            "properties": {
                "kind": "unanswered-requests",
                "count": unanswered.count,
                "ending": unanswered.ending,
                "requests": unanswered.requests,
            },
        }));
    }

    /// What the trace's recorder says it left out, when it says something was:
    /// the kinds and their counts in the notification's properties.
    pub(super) fn capture(&mut self, note: &CaptureNote, artifact: Artifact<'_>) {
        let text = crate::report::capture::one_line(note);
        let key = (artifact.uri.unwrap_or_default().to_owned(), text.clone());
        if !self.noted.insert(key) {
            return;
        }
        let unrecorded: std::collections::BTreeMap<&str, u64> = note.left_out().collect();
        self.notifications.push(json!({
            "level": "note",
            "message": { "text": text },
            "locations": location(artifact, None),
            "properties": {
                "kind": "capture-unrecorded",
                "unrecorded": unrecorded,
            },
        }));
    }
}
