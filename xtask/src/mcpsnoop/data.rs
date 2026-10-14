// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The three committed files the comparison is made of, as types.
//!
//! - `results.json` is **measured**: written by `cargo xtask mcpsnoop` from a
//!   real run, never by hand. What each tool said about each corpus trace.
//! - `rules.json` is **read**: each mcpsnoop rule, the message template that
//!   identifies it, and the registry clauses its text corresponds to.
//! - `triage.json` is **judged**: who is right about each divergence, and why.
//!
//! The page is a projection of all three, so `--check` can re-derive it.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// `conformance/mcpsnoop/results.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Results {
    /// The mcpsnoop version that produced the findings.
    pub mcpsnoop: String,
    /// The conversion checked against mcpsnoop's own recordings.
    pub native: Vec<NativeCheck>,
    /// One entry per corpus trace, in corpus order.
    pub traces: Vec<TraceResult>,
}

/// One session recorded by both tools at once, read both ways.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativeCheck {
    /// The pair's name under `conformance/mcpsnoop/native/`.
    pub session: String,
    /// Findings `mcpsnoop check` raised on our converted trace.
    pub findings: usize,
    /// Whether they are exactly the findings of mcpsnoop's own capture once
    /// its frames are given one connection, as the conversion gives them.
    pub same_as_native: bool,
    /// Findings mcpsnoop's capture, as recorded, raised beyond those.
    pub only_as_recorded: Vec<String>,
}

/// Both tools' reading of one trace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TraceResult {
    /// The trace's path under `corpus/`, without `.jsonl`.
    pub trace: String,
    /// The revision its registry judges it against.
    pub revision: String,
    /// `stdio` or `http`.
    pub transport: String,
    /// Envelopes the converted capture holds, `meta` included.
    pub frames: usize,
    /// Of those, frames whose HTTP context the trace could not settle.
    pub unframed: usize,
    /// Our report, as its committed golden states it.
    pub ours: Ours,
    /// mcpsnoop's `check`.
    pub mcpsnoop: Snoop,
}

/// What our report says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ours {
    /// `pass`, `pass-with-warnings` or `fail`.
    pub verdict: String,
    /// Every requirement whose row is `fail` or `warn`, plus `wire-schema`
    /// when the messages fail the revision's schema.
    pub flagged: Vec<String>,
}

/// What `mcpsnoop check` says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snoop {
    /// Its exit status with every wire signal in `--fail-on`.
    pub exit: i32,
    /// Its conformance findings (signals `invalid`, `warn`, `mismatch`).
    pub findings: Vec<Finding>,
    /// How many findings each other signal raised; reported, never compared.
    pub other: BTreeMap<String, usize>,
}

/// One mcpsnoop finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Finding {
    /// The signal (`invalid`, `warn` or `mismatch`).
    pub signal: String,
    /// The trace event the finding points at, when it points at one.
    pub seq: Option<u64>,
    /// Whether that frame carried all its HTTP context through conversion.
    pub framed: bool,
    /// The message, without the `session … frame N:` prefix.
    pub message: String,
}

/// `conformance/mcpsnoop/rules.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rules {
    /// The mcpsnoop version the templates were read from.
    pub mcpsnoop: String,
    /// Every rule, in the order the page lists them.
    pub rules: Vec<Rule>,
}

/// One mcpsnoop rule and the clauses it corresponds to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rule {
    /// A stable slug, ours.
    pub id: String,
    /// Message templates; `*` matches any run of characters. A finding belongs
    /// to every rule one of whose templates occurs in its message.
    #[serde(rename = "match")]
    pub templates: Vec<String>,
    /// Revision → the requirement ids the rule's condition violates.
    pub maps: BTreeMap<String, Vec<String>>,
    /// The mapped ids the rule judges only part of, so its silence says nothing
    /// about them.
    #[serde(default)]
    pub partial: Vec<String>,
    /// What the rule reads beyond the message itself: `http` (headers, status)
    /// or `pairing` (which exchange a frame rode). Unset for the body alone.
    #[serde(default)]
    pub context: Option<String>,
    /// The rule in a sentence, and why it maps where it does.
    pub reading: String,
}

/// `conformance/mcpsnoop/triage.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Triage {
    /// One entry per divergence pattern; each covers one or more traces.
    pub divergences: Vec<Verdict>,
    /// Divergences found on sessions recorded natively by both tools.
    pub recorded: Vec<Recorded>,
    /// Upstream issue drafts, keyed by the `upstream` value that cites them.
    pub issues: Vec<Issue>,
}

/// The triage of one divergence pattern.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Verdict {
    /// `mcpsnoop-only` (it flags what we do not) or `ours-only`.
    pub kind: String,
    /// The mcpsnoop rule (for `mcpsnoop-only`) or the requirement id we flag
    /// (for `ours-only`).
    pub subject: String,
    /// Every trace showing it.
    pub traces: Vec<String>,
    /// `mcpsnoop-defect`, `our-defect`, `granularity`, `revision-attribution`
    /// or `spec-ambiguity`.
    pub class: String,
    /// `ours`, `mcpsnoop`, `both` or `undecided`.
    pub right: String,
    /// Requirement ids whose verbatim quotes decide it.
    pub quote: Vec<String>,
    /// The reasoning.
    pub note: String,
    /// What happened: `fixed: …`, `open: …` or `upstream: <issue id>`.
    pub resolution: String,
}

/// A divergence on a session both tools recorded live.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Recorded {
    /// The mcpsnoop rule that fired, or that should have.
    pub rule: String,
    /// The committed evidence, relative to `conformance/mcpsnoop/`.
    pub evidence: Vec<String>,
    /// As [`Verdict::class`].
    pub class: String,
    /// As [`Verdict::right`].
    pub right: String,
    /// As [`Verdict::quote`].
    pub quote: Vec<String>,
    /// The reasoning, and how the evidence shows it.
    pub note: String,
    /// As [`Verdict::resolution`].
    pub resolution: String,
}

/// An issue as it would be filed upstream. Drafted here; filing is the
/// owner's.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Issue {
    /// The key `resolution: upstream: <id>` cites.
    pub id: String,
    /// The issue title.
    pub title: String,
    /// The issue body, Markdown.
    pub body: String,
}

/// Reads and parses one committed JSON file.
pub(crate) fn load<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_str(&text).map_err(|error| format!("{} is malformed: {error}", path.display()))
}
