// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A baseline in SARIF: the native `suppression` of an accepted finding, and a
//! notification for each entry that did not do its job.
//!
//! SARIF 2.1.0 (§3.35) has a `suppression` object for exactly this — a result
//! the project has decided to live with — with a `kind` saying where the
//! decision is kept (`external`: a file beside the code, not an annotation in
//! it), a `status` (`accepted`) and a `justification`. A viewer that honours it
//! hides the result from its counts; the result itself stays in the log.

use serde_json::{Value, json};

use crate::baseline::Notes;
use crate::report::Acceptance;

/// The `suppressions` member of an accepted finding's result.
pub(super) fn suppressions(acceptance: &Acceptance) -> Value {
    let mut suppression = json!({
        "kind": "external",
        "status": "accepted",
        "justification": acceptance.reason,
    });
    if let Some(expires) = &acceptance.expires {
        suppression["properties"] = json!({ "expires": expires });
    }
    if acceptance.unreviewed() {
        suppression["properties"]["unreviewed"] = json!(true);
    }
    json!([suppression])
}

/// A tool execution notification per entry that did not do its job. A stale
/// entry is a `warning` — the run exits `1` for it unless the notes allow it,
/// which is a finding, not a failed execution — and a lapsed one a `note`:
/// its finding is already a result that counts.
pub(super) fn notifications(notes: &Notes) -> Vec<Value> {
    let stale = notes.stale.iter().map(|entry| {
        let text = if notes.allow_stale {
            "baseline entry matches no finding of this run; the finding it accepted no longer \
             occurs, and stale entries are allowed for this run"
        } else {
            "baseline entry matches no finding of this run; the finding it accepted no longer \
             occurs, so this run exits 1 until the entry is removed"
        };
        json!({
            "level": "warning",
            "message": { "text": format!("{text}: {}", entry.fingerprint) },
            "properties": {
                "kind": "baseline-stale",
                "fingerprint": entry.fingerprint,
                "reason": entry.reason,
                "fails": !notes.allow_stale,
            },
        })
    });
    let lapsed = notes.lapsed.iter().map(|entry| {
        let until = entry
            .expires
            .map_or_else(String::new, |day| format!(" on {day}"));
        json!({
            "level": "note",
            "message": { "text": format!(
                "baseline entry expired{until}: it accepts nothing, and its finding counts: {}",
                entry.fingerprint
            ) },
            "properties": {
                "kind": "baseline-lapsed",
                "fingerprint": entry.fingerprint,
                "reason": entry.reason,
                "expires": entry.expires.map(|day| day.to_string()),
            },
        })
    });
    stale.chain(lapsed).collect()
}
