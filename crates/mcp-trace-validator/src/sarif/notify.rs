// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The notifications a run carries beside its results: the requests a trace
//! ended without answering, a trace the run could not judge, and a clause this
//! build cannot judge.

use serde_json::json;

use super::{Artifact, Log, location};
use crate::report::{RequirementReport, Unjudged};

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

    /// A trace the run could not judge at all, located at the trace.
    pub(super) fn unjudged(&mut self, trace: &Unjudged) {
        let at = Artifact {
            uri: Some(&trace.trace),
            events: &[],
        };
        self.notifications.push(json!({
            "level": "error",
            "message": { "text": trace.error },
            "locations": location(at, None),
        }));
    }

    /// An `unsupported` row: the run could not judge this clause.
    pub(super) fn unsupported(&mut self, row: &RequirementReport, revision: &str) {
        self.notifications.push(json!({
            "level": "error",
            "message": {
                "text": format!(
                    "{} ({revision}): the registry references checks this build does not implement: {}",
                    row.id,
                    row.missing_checks.join(", ")
                ),
            },
        }));
    }
}
