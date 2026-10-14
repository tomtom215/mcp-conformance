// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The terminal-oriented rendering of a [`Report`].
//!
//! Split from the data model so the report's shape and the way it is printed
//! can be read separately: everything here is presentation, and nothing here
//! decides an outcome.

use core::fmt::Write as _;

use super::{ExtensionReport, Finding, Outcome, Report, WireSchemaReport};

/// How the human renderers lay a report out: [`Report::render_with`] and
/// [`MultiReport::render_with`](crate::multi::MultiReport::render_with).
///
/// The default is the whole report — every clause, every finding — worded for
/// a library caller; [`Report::render_human`] and [`Report::render_findings`]
/// are its two common settings.
///
/// ```
/// use mcp_trace_validator::report::HumanOptions;
///
/// // What `mcp-trace-validator validate` prints without `--all`.
/// let options = HumanOptions::default().all(false).max_findings(Some(20)).cli(true);
/// assert_eq!(options.max_findings, Some(20));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HumanOptions {
    /// List every clause; otherwise only the failing, warning and unsupported
    /// ones. The totals count every clause either way.
    pub all: bool,
    /// The most findings listed under one clause (per revision), the rest
    /// counted on one line; `None` lists every one. A trace repeating one
    /// defect a quarter of a million times otherwise renders a quarter of a
    /// million lines that say the same thing.
    pub max_findings: Option<usize>,
    /// Word advice as `mcp-trace-validator` command-line flags (`--revision`,
    /// `--all`), for a report the CLI prints; otherwise it names no flag a
    /// library caller does not have.
    pub cli: bool,
}

impl Default for HumanOptions {
    /// Every clause and every finding, in library wording.
    fn default() -> Self {
        Self {
            all: true,
            max_findings: None,
            cli: false,
        }
    }
}

impl HumanOptions {
    /// These options, listing every clause (`true`) or only those needing
    /// attention (`false`).
    #[must_use]
    pub const fn all(mut self, all: bool) -> Self {
        self.all = all;
        self
    }

    /// These options, listing at most `max` findings under each clause.
    #[must_use]
    pub const fn max_findings(mut self, max: Option<usize>) -> Self {
        self.max_findings = max;
        self
    }

    /// These options, worded for the command line (`true`) or not.
    #[must_use]
    pub const fn cli(mut self, cli: bool) -> Self {
        self.cli = cli;
        self
    }
}

impl Report {
    /// Renders the human-readable form: every requirement's row.
    #[must_use]
    pub fn render_human(&self) -> String {
        self.render_with(&HumanOptions::default())
    }

    /// Renders only what needs attention — failing, warning and unsupported rows —
    /// with the header, totals and verdict. The totals still account for every
    /// clause, so nothing is hidden from the arithmetic.
    #[must_use]
    pub fn render_findings(&self) -> String {
        self.render_with(&HumanOptions::default().all(false))
    }

    /// Renders the human-readable form as `options` lay it out.
    #[must_use]
    pub fn render_with(&self, options: &HumanOptions) -> String {
        let mut out = String::new();
        match self.revision_source {
            Some(source) => {
                let _ = writeln!(
                    out,
                    "MCP trace validation — revision {} ({source})",
                    self.revision
                );
            }
            None => {
                let _ = writeln!(out, "MCP trace validation — revision {}", self.revision);
            }
        }
        self.write_revision_mismatch(&mut out, options);
        for row in &self.requirements {
            if !options.all && !row.outcome.needs_attention() {
                continue;
            }
            write_row(&mut out, row, options);
        }
        if let Some(wire_schema) = &self.wire_schema {
            wire_schema.write_human(&mut out, options, None);
        }
        for extension in &self.extensions {
            extension.write_human(&mut out, options, None);
        }
        // Every outcome is named, so the counts sum to the registry's size — a
        // reader can check the arithmetic, and `Totals`' own exhaustive
        // destructuring is what keeps that true as outcomes are added.
        let _ = writeln!(out, "totals: {}", self.totals);
        if let Some(wire_schema) = &self.wire_schema {
            let _ = writeln!(out, "wire schema: {}", wire_schema.summary());
        }
        let _ = writeln!(out, "verdict: {}", self.verdict());
        self.write_revision_mismatch(&mut out, options);
        out
    }

    /// Writes the revision-disagreement note, if there is one.
    ///
    /// Rendered twice — under the header and under the verdict — because both
    /// are where a reader looks, and a note that scrolls past a hundred rows of
    /// findings is a note nobody reads. It is short enough that repeating it
    /// costs less than missing it.
    fn write_revision_mismatch(&self, out: &mut String, options: &HumanOptions) {
        let Some(declared) = &self.revision_mismatch else {
            return;
        };
        let subject = if declared.len() == 1 {
            "revision"
        } else {
            "revisions"
        };
        let suggestion = declared.last().map_or("<revision>", String::as_str);
        let _ = writeln!(
            out,
            "  NOTE  this session declares protocol {subject} {}, not {}.",
            declared.join(", "),
            self.revision
        );
        let _ = writeln!(
            out,
            "        Every outcome here judges it against rules it was not playing by;"
        );
        if options.cli {
            let _ = writeln!(
                out,
                "        re-run with `--revision {suggestion}` to judge it against its own."
            );
        } else {
            let _ = writeln!(
                out,
                "        judge it against revision {suggestion} to hold it to its own."
            );
        }
    }
}

/// One requirement's row and its detail lines.
fn write_row(out: &mut String, row: &super::RequirementReport, options: &HumanOptions) {
    let marker = match row.outcome {
        Outcome::Pass => "PASS",
        Outcome::Fail => "FAIL",
        Outcome::Warn => "WARN",
        Outcome::Excluded => "EXCL",
        Outcome::Unsupported => "UNSUP",
        Outcome::NotApplicable => "N/A",
        Outcome::NotObserved => "NOBS",
    };
    let _ = writeln!(out, "  {marker:<5} {} ({})", row.id, row.level);
    options.write_findings(out, &row.findings, None);
    if let Some(source) = &row.source {
        source.write_human(out);
    }
    if let Some(exclusion) = &row.exclusion {
        let _ = writeln!(out, "        excluded: {exclusion}");
    }
    for check in &row.missing_checks {
        let _ = writeln!(out, "        unsupported check: {check}");
    }
    if let Some(capability) = &row.capability {
        let _ = writeln!(
            out,
            "        not applicable: capability {capability} was not declared in this session"
        );
    }
    if row.outcome == Outcome::NotObserved {
        let _ = writeln!(
            out,
            "        not observed: the session carried none of the traffic this clause binds to"
        );
    }
}

impl HumanOptions {
    /// A row's findings, one line each — `seq 3: …`, or `2025-11-25 seq 3: …`
    /// under a `revision` — up to [`Self::max_findings`], then one line
    /// counting the rest. Shared with the multi-revision report.
    pub(crate) fn write_findings(
        &self,
        out: &mut String,
        findings: &[Finding],
        revision: Option<&str>,
    ) {
        let shown = self.max_findings.unwrap_or(usize::MAX);
        for finding in findings.iter().take(shown) {
            let detail = printable(&finding.detail);
            let _ = match (revision, finding.seq) {
                (Some(revision), Some(seq)) => {
                    writeln!(out, "        {revision} seq {seq}: {detail}")
                }
                (Some(revision), None) => writeln!(out, "        {revision}: {detail}"),
                (None, Some(seq)) => writeln!(out, "        seq {seq}: {detail}"),
                (None, None) => writeln!(out, "        {detail}"),
            };
        }
        let rest = findings.len().saturating_sub(shown);
        if rest > 0 {
            let label = revision.map_or_else(String::new, |revision| format!("{revision}: "));
            let how = if self.cli {
                " (pass --all, or use --format json, for every finding)"
            } else {
                ""
            };
            let _ = writeln!(out, "        {label}… and {rest} more{how}");
        }
    }
}

/// `text` with every control character escaped as `\u{..}`.
///
/// A finding quotes the trace — a method name, an id, a header — and a
/// recording is untrusted input: an `ESC` sequence in a method name reached
/// the terminal or the CI log raw, where it can rewrite what the reader sees.
/// Machine formats are unaffected; JSON escapes these itself.
pub fn printable(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.chars().any(char::is_control) {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(
        text.chars()
            .map(|character| {
                if character.is_control() {
                    character.escape_unicode().to_string()
                } else {
                    character.to_string()
                }
            })
            .collect(),
    )
}

impl super::ClauseSource {
    /// The violated clause under a failing or warning row: the quote, then where
    /// it is published. Shared with the multi-revision report, which prints the
    /// same two lines.
    pub(crate) fn write_human(&self, out: &mut String) {
        let _ = writeln!(out, "        spec: \"{}\"", self.quote);
        let _ = writeln!(out, "        see:  {}", self.url);
    }
}

impl WireSchemaReport {
    /// One line for under the totals: the outcome and the counts behind it.
    #[must_use]
    pub fn summary(&self) -> String {
        match self.outcome {
            Outcome::NotObserved => "not observed (the trace holds no message)".to_owned(),
            Outcome::Fail => format!(
                "fail ({} of {} messages invalid)",
                self.findings.len(),
                self.messages
            ),
            _ => format!("pass ({} messages valid)", self.messages),
        }
    }

    /// The schema's row, shown when it failed or when every row is listed; its
    /// findings carry the revision they were judged at under `revision` (the
    /// multi-revision report).
    pub(crate) fn write_human(
        &self,
        out: &mut String,
        options: &HumanOptions,
        revision: Option<&str>,
    ) {
        if !options.all && self.outcome != Outcome::Fail {
            return;
        }
        let marker = match self.outcome {
            Outcome::Fail => "FAIL",
            Outcome::NotObserved => "NOBS",
            _ => "PASS",
        };
        let label = revision.map_or_else(String::new, |revision| format!(" {revision}"));
        let _ = writeln!(out, "  {marker:<5} wire schema{label} — {}", self.summary());
        options.write_findings(out, &self.findings, revision);
        if self.outcome == Outcome::Fail {
            self.source.write_human(out);
            let _ = writeln!(out, "        schema: {}", self.schema);
        }
    }
}

impl ExtensionReport {
    /// The extension's block: a header saying it is not scored, the rows the
    /// options select, and its own totals and verdict — under a `revision` in
    /// the multi-revision report. Written before the report's totals, which it
    /// is not part of; the header and the last line are written whatever the
    /// options, so a reader sees the extension was exercised.
    pub(crate) fn write_human(
        &self,
        out: &mut String,
        options: &HumanOptions,
        revision: Option<&str>,
    ) {
        let label = revision.map_or_else(String::new, |revision| format!(" at {revision}"));
        let _ = writeln!(
            out,
            "extension {}{label} — not scored: its outcomes never change the verdict",
            self.extension
        );
        for row in &self.requirements {
            if options.all || row.outcome.needs_attention() {
                write_row(out, row, options);
            }
        }
        let _ = writeln!(
            out,
            "  extension totals: {} — extension verdict: {}",
            self.totals,
            self.verdict()
        );
    }
}
