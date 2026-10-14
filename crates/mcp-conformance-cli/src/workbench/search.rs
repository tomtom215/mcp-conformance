// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The in-memory index the search box queries ([`super::query`] has the
//! language).
//!
//! Each entry's [`Doc`] holds the values a query can name — methods, tool,
//! resource and prompt names, session ids, error codes, requirement IDs with
//! findings, the server's name, tags, the verdict — each with the events
//! (`seq`) it occurs at, plus the text of every finding. It is rebuilt when
//! an entry changes, and never holds a payload: the index is small, and a
//! search answers from memory.
//!
//! **Ranking.** Every term must match (a negated one must not). A matching
//! field term scores 10, plus one per further occurrence up to 5; free text
//! scores 3 when it is in the entry's id or tags, 2 when it is in a finding
//! and 1 when it is in any other value, added together. The
//! highest total ranks first; ties go by id, so a ranking is deterministic.
//! Each result carries the events its terms matched, so a result opens at
//! its first hit.

use std::collections::{BTreeMap, BTreeSet};

use mcp_conformance_core::trace::{EventBody, TraceEvent};
use mcp_trace_validator::report::Report;
use serde_json::Value;

use super::query::{Field, Query, Term};

/// What a query can find in one entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Doc {
    /// Field → lowercased value → the events it occurs at (empty for a
    /// property of the whole session, such as its server or a tag).
    fields: BTreeMap<Field, BTreeMap<String, BTreeSet<u64>>>,
    /// Every finding: its lowercased text and its event.
    findings: Vec<(String, Option<u64>)>,
    /// The entry's id and tags, lowercased, space-separated.
    name: String,
}

/// One entry a query matched.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Hit {
    pub(crate) id: String,
    pub(crate) score: u32,
    /// The events the terms matched, ascending.
    pub(crate) events: Vec<u64>,
}

/// What a session is, for indexing: its events, reports and properties.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Source<'a> {
    pub(crate) id: &'a str,
    pub(crate) events: &'a [TraceEvent],
    pub(crate) reports: &'a [Report],
    pub(crate) tags: &'a [String],
    pub(crate) server: Option<&'a str>,
    pub(crate) verdict: &'a str,
}

impl Doc {
    fn add(&mut self, field: Field, value: &str, seq: Option<u64>) {
        let seqs = self
            .fields
            .entry(field)
            .or_default()
            .entry(value.to_lowercase())
            .or_default();
        seqs.extend(seq);
    }

    /// The index of one session.
    pub(crate) fn build(source: &Source<'_>) -> Self {
        let mut doc = Self::default();
        for event in source.events {
            match &event.body {
                EventBody::Message { payload } => match payload {
                    Value::Array(parts) => {
                        for part in parts {
                            doc.message(part, event.seq);
                        }
                    }
                    part => doc.message(part, event.seq),
                },
                EventBody::Http { headers, .. } => {
                    if let Some(session) = headers.get("mcp-session-id") {
                        doc.add(Field::Session, session, Some(event.seq));
                    }
                }
                _ => {}
            }
        }
        doc.add(Field::Session, source.id, None);
        for report in source.reports {
            for row in &report.requirements {
                for finding in &row.findings {
                    doc.add(Field::Req, &row.id, finding.seq);
                    doc.findings.push((
                        format!("{} {}", row.id, finding.detail).to_lowercase(),
                        finding.seq,
                    ));
                }
            }
            if let Some(schema) = &report.wire_schema {
                for finding in &schema.findings {
                    doc.findings.push((
                        format!("wire schema {}", finding.detail).to_lowercase(),
                        finding.seq,
                    ));
                }
            }
        }
        if let Some(server) = source.server {
            doc.add(Field::Server, server, None);
        }
        for tag in source.tags {
            doc.add(Field::Tag, tag, None);
        }
        doc.add(Field::Verdict, source.verdict, None);
        doc.name = core::iter::once(source.id)
            .chain(source.tags.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        doc
    }

    /// The values one message contributes.
    fn message(&mut self, message: &Value, seq: u64) {
        let seq = Some(seq);
        let text = |value: Option<&Value>| value.and_then(Value::as_str).map(ToOwned::to_owned);
        let params = message.get("params");
        if let Some(method) = text(message.get("method")) {
            self.add(Field::Method, &method, seq);
            let name = text(params.and_then(|p| p.get("name")));
            let uri = text(params.and_then(|p| p.get("uri")));
            match (method.as_str(), name, uri) {
                ("tools/call", Some(name), _) => self.add(Field::Tool, &name, seq),
                ("prompts/get", Some(name), _) => self.add(Field::Prompt, &name, seq),
                (
                    "resources/read" | "resources/subscribe" | "resources/unsubscribe",
                    _,
                    Some(uri),
                ) => self.add(Field::Resource, &uri, seq),
                _ => {}
            }
        }
        if let Some(code) = message.get("error").and_then(|error| error.get("code")) {
            self.add(Field::Code, &code.to_string(), seq);
        }
        let Some(result) = message.get("result") else {
            return;
        };
        for (list, member, field) in [
            ("tools", "name", Field::Tool),
            ("prompts", "name", Field::Prompt),
            ("resources", "uri", Field::Resource),
        ] {
            for item in result
                .get(list)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(value) = text(item.get(member)) {
                    self.add(field, &value, seq);
                }
            }
        }
    }

    /// This entry's score and hits for one term; `None` when it does not
    /// match.
    fn score(&self, term: &Term) -> Option<(u32, BTreeSet<u64>)> {
        if term.field == Field::Text {
            return self.score_text(term);
        }
        let mut occurrences = 0_u32;
        let mut events = BTreeSet::new();
        for (value, seqs) in self.fields.get(&term.field)? {
            if term.accepts(value) {
                occurrences += u32::try_from(seqs.len().max(1)).unwrap_or(u32::MAX);
                events.extend(seqs);
            }
        }
        (occurrences > 0).then(|| (10 + (occurrences - 1).min(5), events))
    }

    fn score_text(&self, term: &Term) -> Option<(u32, BTreeSet<u64>)> {
        let mut events = BTreeSet::new();
        let in_name = term.accepts(&self.name);
        let mut in_findings = false;
        for (text, seq) in &self.findings {
            if term.accepts(text) {
                in_findings = true;
                events.extend(seq);
            }
        }
        let mut elsewhere = false;
        for (value, seqs) in self.fields.values().flatten() {
            if term.accepts(value) {
                elsewhere = true;
                events.extend(seqs);
            }
        }
        let score = 3 * u32::from(in_name) + 2 * u32::from(in_findings) + u32::from(elsewhere);
        (score > 0).then_some((score, events))
    }
}

/// The entries of `docs` that match `query`, best first. An empty query
/// matches every entry with score 0, in id order.
pub(crate) fn search<'a>(
    query: &Query,
    docs: impl IntoIterator<Item = (&'a str, &'a Doc)>,
) -> Vec<Hit> {
    let mut hits = Vec::new();
    'docs: for (id, doc) in docs {
        let mut score = 0;
        let mut events = BTreeSet::new();
        for term in &query.terms {
            match (doc.score(term), term.negated) {
                (Some(_), true) | (None, false) => continue 'docs,
                (None, true) => {}
                (Some((points, found)), false) => {
                    score += points;
                    events.extend(found);
                }
            }
        }
        hits.push(Hit {
            id: id.to_owned(),
            score,
            events: events.into_iter().collect(),
        });
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    hits
}

#[cfg(test)]
mod tests;
