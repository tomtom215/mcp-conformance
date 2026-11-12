// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! One library entry in memory: its sidecar, its verdict, the facts the
//! library list shows, and its search index.
//!
//! The verdict is computed from the trace's bytes by [`mcp_trace_validator::judge`]
//! — the rules `mcp-trace-validator validate` applies — every time an entry is
//! loaded, and is never read from the sidecar ([`super::meta`] says why). The
//! one place it differs from the command line is deliberate and marked: a trace
//! still being written has its unfinished last line left out of the judgment,
//! and the entry says so (`Entry::provisional`).

use std::time::SystemTime;

use mcp_conformance_core::trace::{Direction, TraceEvent, TransportKind};
use mcp_trace_validator::judge;
use mcp_trace_validator::reader::{self, Limits};
use mcp_trace_validator::report::{Report, Verdict};
use ring::digest;
use serde::Serialize;
use serde_json::Value;

use super::clock;
use super::meta::Meta;
use super::search::{self, Doc};

/// The verdict word of an entry that could not be judged.
pub(crate) const UNJUDGED: &str = "unjudged";

/// A trace's SHA-256, lowercase hex.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    hex(digest::digest(&digest::SHA256, bytes).as_ref())
}

/// The part of `text` a recorder has finished writing, and whether an
/// unfinished last line was left out.
///
/// All of it, unless the last line has no newline yet and is not yet a whole
/// JSON value — a trace still being written, or one that was cut off — in which
/// case everything before that line, and `true`. The exception is a recording
/// that has already ended: when the last complete line is its `transport-close`
/// or `transport-abort`, an unfinished line after it cannot be a write in
/// progress, so it is damage and the text is judged whole (the validator then
/// calls it malformed). A trace cut off at its own closing line cannot be told
/// from one still being written, and is reported as the second.
pub(crate) fn split_unfinished(text: &str) -> (&str, bool) {
    let start = text.rfind('\n').map_or(0, |at| at + 1);
    let tail = &text[start..];
    // An empty text, or one ending in a newline, has an empty last line, which
    // is not a JSON value: everything before it — all of it — is kept.
    if tail.is_empty()
        || serde_json::from_str::<Value>(tail).is_ok()
        || ends_the_recording(&text[..start])
    {
        return (text, false);
    }
    (&text[..start], true)
}

/// Whether the last complete line of `lines` is a recording's closing event.
fn ends_the_recording(lines: &str) -> bool {
    lines
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .is_some_and(|event| {
            event.get("kind").and_then(Value::as_str) == Some("lifecycle")
                && matches!(
                    event.get("event").and_then(Value::as_str),
                    Some("transport-close" | "transport-abort")
                )
        })
}

/// [`split_unfinished`]'s text alone.
pub(crate) fn complete(text: &str) -> &str {
    split_unfinished(text).0
}

/// `bytes` as lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use core::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// One entry.
#[derive(Debug, Clone)]
pub(crate) struct Entry {
    pub(crate) id: String,
    pub(crate) meta: Meta,
    /// Why the sidecar could not be read, when it could not (it is then
    /// never written).
    pub(crate) meta_problem: Option<String>,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) modified: Option<SystemTime>,
    pub(crate) reports: Vec<Report>,
    pub(crate) unjudged: Option<String>,
    /// Why the verdict covers only part of the file, when it does: the last
    /// line was unfinished and was left out of the judgment.
    pub(crate) provisional: Option<String>,
    pub(crate) facts: Facts,
    pub(crate) doc: Doc,
}

/// What the library list shows about a recording beside its verdict.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct Facts {
    pub(crate) events: usize,
    pub(crate) messages: usize,
    pub(crate) transport: &'static str,
    /// `name version` from the server's `serverInfo`.
    pub(crate) server: Option<String>,
    /// `name version` from the client's `clientInfo`.
    pub(crate) client: Option<String>,
    /// The first event's timestamp, when the trace records one.
    pub(crate) first_ts: Option<String>,
}

/// `name version` of an `Implementation` object.
fn implementation(info: &Value) -> Option<String> {
    let name = info.get("name")?.as_str()?;
    Some(
        info.get("version")
            .and_then(Value::as_str)
            .map_or_else(|| name.to_owned(), |version| format!("{name} {version}")),
    )
}

/// The facts of a trace's events.
pub(crate) fn facts(events: &[TraceEvent]) -> Facts {
    let mut facts = Facts {
        events: events.len(),
        first_ts: events.first().and_then(|event| event.ts.clone()),
        ..Facts::default()
    };
    let mut transports = events.iter().map(|event| event.transport);
    facts.transport = match transports.next() {
        None => "none",
        Some(first) if transports.all(|kind| kind == first) => match first {
            TransportKind::Stdio => "stdio",
            TransportKind::StreamableHttp => "streamable-http",
            _ => "other",
        },
        Some(_) => "mixed",
    };
    for event in events {
        let Some(payload) = event.message_payload() else {
            continue;
        };
        facts.messages += 1;
        let (role, member) = match event.direction {
            Direction::ServerToClient => (&mut facts.server, "serverInfo"),
            Direction::ClientToServer => (&mut facts.client, "clientInfo"),
        };
        if role.is_none() {
            let info = payload
                .get("result")
                .and_then(|result| result.get(member))
                .or_else(|| payload.get("params").and_then(|params| params.get(member)));
            *role = info.and_then(implementation);
        }
    }
    facts
}

/// The verdict of a trace, by the validator's rules: its reports, or why it
/// could not be judged.
pub(crate) fn judge_text(text: &str) -> (Vec<Report>, Option<String>) {
    match judge::judge(text) {
        Ok(judgment) => (judgment.reports, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    }
}

impl Entry {
    /// The entry for `id`, from its trace's bytes and its sidecar; the
    /// second value is `true` when the sidecar carried something to drop (a
    /// verdict an earlier build wrote) and should be written back without it.
    pub(crate) fn load(
        id: &str,
        bytes: &[u8],
        meta: Meta,
        meta_problem: Option<String>,
        modified: Option<SystemTime>,
    ) -> (Self, bool) {
        let sha256 = sha256(bytes);
        let whole = String::from_utf8_lossy(bytes);
        let (text, cut) = split_unfinished(&whole);
        let events = reader::parse_trace(text, &Limits::default()).unwrap_or_default();
        let mut meta = meta;
        let rewrite = meta.drop_legacy_cache();
        let (reports, unjudged) = judge_text(text);
        let mut entry = Self {
            id: id.to_owned(),
            meta,
            meta_problem,
            sha256,
            size: bytes.len() as u64,
            modified,
            reports,
            unjudged,
            provisional: cut.then(|| {
                "the last line is incomplete (a trace still being written, or one cut off) \
                 and is not judged"
                    .to_owned()
            }),
            facts: facts(&events),
            doc: Doc::default(),
        };
        entry.reindex(&events);
        (entry, rewrite)
    }

    /// Rebuilds the search index (after a tag changes, say).
    pub(crate) fn reindex(&mut self, events: &[TraceEvent]) {
        let verdict = self.verdict();
        self.doc = Doc::build(&search::Source {
            id: &self.id,
            events,
            reports: &self.reports,
            tags: &self.meta.tags,
            server: self.facts.server.as_deref(),
            verdict: &verdict,
        });
    }

    /// The verdict word: the validator's (`pass`, `pass-with-warnings`,
    /// `fail`, `unsupported`), or [`UNJUDGED`].
    pub(crate) fn verdict(&self) -> String {
        if self.unjudged.is_some() {
            return UNJUDGED.to_owned();
        }
        // Nothing judged and no reason given is not a pass: a verdict is claimed
        // only for something that was judged.
        let Some(verdict) = Verdict::most_severe(self.reports.iter().map(Report::verdict)) else {
            return UNJUDGED.to_owned();
        };
        serde_json::to_value(verdict)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_default()
    }

    /// When the recording was made, as well as the library knows: the
    /// sidecar's `recorded_at`, else the first event's timestamp, else when
    /// it was added, else the file's modification time.
    pub(crate) fn when(&self) -> Option<String> {
        self.meta
            .recorded_at
            .clone()
            .or_else(|| self.facts.first_ts.clone())
            .or_else(|| self.meta.added_at.clone())
            .or_else(|| self.modified.map(clock::rfc3339))
    }

    /// The day of [`Self::when`].
    pub(crate) fn day(&self) -> Option<String> {
        self.when()
            .as_deref()
            .and_then(clock::day)
            .map(ToOwned::to_owned)
    }

    /// The number of findings across the reports, the wire schema's
    /// included.
    pub(crate) fn findings(&self) -> usize {
        self.reports
            .iter()
            .map(|report| {
                report
                    .requirements
                    .iter()
                    .map(|row| row.findings.len())
                    .sum::<usize>()
                    + report
                        .wire_schema
                        .as_ref()
                        .map_or(0, |schema| schema.findings.len())
            })
            .sum()
    }

    /// The entry as the library list shows it.
    pub(crate) fn summary(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "verdict": self.verdict(),
            "unjudged": self.unjudged,
            "provisional": self.provisional,
            "revisions": self.reports.iter().map(|report| report.revision.as_str()).collect::<Vec<_>>(),
            "findings": self.findings(),
            "tags": self.meta.tags,
            "source": self.meta.source,
            "sidecar_problem": self.meta_problem,
            "when": self.when(),
            "day": self.day(),
            "sha256": self.sha256,
            "bytes": self.size,
            "facts": self.facts,
        })
    }
}

#[cfg(test)]
mod tests;
