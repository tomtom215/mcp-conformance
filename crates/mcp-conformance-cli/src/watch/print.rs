// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What `watch` writes on stderr: each new finding, a one-line tally, and the
//! report at the end.
//!
//! Everything quoted from the session goes through the validator's
//! [`printable`]: a recording is untrusted input, and an `ESC` in a method name
//! must not reach the terminal raw.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use mcp_conformance_cli::live::LiveFinding;
use mcp_trace_validator::report::{HumanOptions, Outcome, printable};

/// The prefix of every line `watch` writes.
pub(super) const PREFIX: &str = "mcp-conformance watch:";

/// The most findings listed under one clause in the final report.
const MAX_FINDINGS: usize = 20;

/// A new finding: the outcome, clause ID and level, `seq` and detail, then
/// the clause's quote and where it is published. `session` names the session
/// when each has its own trace.
pub(super) fn finding(finding: &LiveFinding, session: Option<u64>) -> String {
    let mut text = String::new();
    let marker = if finding.outcome == Outcome::Warn {
        "WARN"
    } else {
        "FAIL"
    };
    let _ = write!(text, "{PREFIX} ");
    if let Some(session) = session {
        let _ = write!(text, "session {session}: ");
    }
    let _ = write!(text, "{marker} {} ({})", finding.requirement, finding.level);
    if let Some(seq) = finding.seq {
        let _ = write!(text, " seq {seq}");
    }
    let _ = writeln!(text, ": {}", printable(&finding.detail));
    if let Some(quote) = &finding.quote {
        let _ = writeln!(text, "    spec: \"{}\"", printable(quote));
    }
    if let Some(url) = &finding.url {
        let _ = writeln!(text, "    see:  {}", printable(url));
    }
    text
}

/// The counts behind the tally.
///
/// It counts *findings* — one per tool schema that declares the wrong
/// `$schema`, say — where the report's `totals` count *clauses*, one per
/// requirement however many findings it has. The tally names both, so the two
/// can be read against each other: `14 warn in 1 clause(s)` is the report's
/// `1 warn`.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Tally {
    pub(super) sessions: usize,
    pub(super) exchanges: u64,
    pub(super) fail: u64,
    pub(super) warn: u64,
    /// The clauses with a finding, one per session, revision and requirement
    /// (a report's rows are per session and revision too).
    clauses: BTreeSet<(u64, String, String)>,
    /// Whether the revision's wire schema has a finding: reported beside the
    /// clauses, not as one of them.
    schema: bool,
}

/// The requirement ID a finding of the wire schema carries.
const WIRE_SCHEMA: &str = "wire-schema";

impl Tally {
    /// Counts `finding`, found in session `trace`.
    pub(super) fn count(&mut self, trace: u64, finding: &LiveFinding) {
        if finding.outcome == Outcome::Warn {
            self.warn += 1;
        } else {
            self.fail += 1;
        }
        if finding.requirement == WIRE_SCHEMA {
            self.schema = true;
        } else {
            self.clauses
                .insert((trace, finding.revision.clone(), finding.requirement.clone()));
        }
    }

    /// ` in 1 clause(s)`, ` in 2 clause(s) and the wire schema`, or nothing
    /// before the first finding.
    fn spread(&self) -> String {
        let mut parts = Vec::new();
        if !self.clauses.is_empty() {
            parts.push(format!("{} clause(s)", self.clauses.len()));
        }
        if self.schema {
            parts.push("the wire schema".to_owned());
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!(" in {}", parts.join(" and "))
        }
    }

    /// One line, no newline.
    pub(super) fn line(&self) -> String {
        let sessions = if self.sessions == 1 {
            String::new()
        } else {
            format!(" in {} sessions", self.sessions)
        };
        format!(
            "{PREFIX} {} exchange(s){sessions}; findings so far: {} fail, {} warn{}",
            self.exchanges,
            self.fail,
            self.warn,
            self.spread()
        )
    }
}

/// The report of one session at its end: the validator's human report, as
/// `mcp-trace-validator validate` prints it, under the trace's name.
pub(super) fn report(name: &str, judgment: &mcp_trace_validator::judge::Judgment) -> String {
    let options = HumanOptions::default()
        .all(false)
        .max_findings(Some(MAX_FINDINGS));
    let mut text = format!("{PREFIX} the validator's report of {}:\n", printable(name));
    for report in &judgment.reports {
        text.push_str(&report.render_with(&options));
    }
    text
}

/// A finding reported live that the whole trace does not show.
pub(super) fn withdrawn(finding: &LiveFinding, session: Option<u64>) -> String {
    let shown = self::finding(finding, session);
    let first = shown.lines().next().unwrap_or_default();
    let rest = first.strip_prefix(PREFIX).unwrap_or(first).trim_start();
    format!("{PREFIX} withdrawn — reported live, but the whole trace does not show it: {rest}\n")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn sample(detail: &str) -> LiveFinding {
        let mut live = mcp_conformance_cli::live::Live::new().unwrap();
        for line in [
            r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"c","version":"0"}}}}"#,
            r#"{"seq":1,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"s","version":"0"}}}}"#,
            r#"{"seq":2,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
            r#"{"seq":3,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"method":"ping"}}"#,
            r#"{"seq":4,"direction":"server-to-client","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":2,"error":{"code":-32603}}}"#,
        ] {
            live.push(line.as_bytes(), std::time::SystemTime::now());
        }
        let mut found = live
            .judge()
            .new
            .into_iter()
            .find(|finding| finding.requirement == "BASE-006")
            .unwrap();
        found.detail = detail.to_owned();
        found
    }

    #[test]
    fn a_finding_names_its_clause_level_seq_and_quote_with_controls_escaped() {
        let text = finding(&sample("method \u{1b}[2Jx"), Some(2));
        let mut lines = text.lines();
        assert_eq!(
            lines.next().unwrap(),
            "mcp-conformance watch: session 2: FAIL BASE-006 (MUST) seq 4: method \\u{1b}[2Jx"
        );
        assert!(!text.contains('\u{1b}'), "{text:?}");
        assert_eq!(
            lines.next().unwrap(),
            "    spec: \"Error responses MUST include an `error` field with a `code` and `message`.\""
        );
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("    see:  https://modelcontextprotocol.io/")
        );
        let withdrawn = withdrawn(&sample("x"), None);
        assert_eq!(
            withdrawn,
            "mcp-conformance watch: withdrawn — reported live, but the whole trace does not show \
             it: FAIL BASE-006 (MUST) seq 4: x\n"
        );
    }

    #[test]
    fn the_tally_is_one_line() {
        let mut tally = Tally {
            sessions: 1,
            exchanges: 3,
            ..Tally::default()
        };
        assert_eq!(
            tally.line(),
            "mcp-conformance watch: 3 exchange(s); findings so far: 0 fail, 0 warn",
            "before a finding there is nothing to spread over clauses"
        );
        tally.count(1, &sample("x"));
        assert_eq!(
            tally.line(),
            "mcp-conformance watch: 3 exchange(s); findings so far: 1 fail, 0 warn in 1 clause(s)"
        );
        tally.sessions = 2;
        assert!(tally.line().contains(" in 2 sessions;"));
        assert!(!tally.line().contains('\n'));
    }

    /// The audit's finding 3.9: `findings so far: 0 fail, 14 warn` beside a
    /// report whose totals read `1 warn`. The tally counts findings and the
    /// totals count clauses; the tally now says how many clauses its findings
    /// are, so the two read against each other.
    #[test]
    fn many_findings_of_one_clause_are_one_clause() {
        let mut tally = Tally {
            sessions: 1,
            exchanges: 4,
            ..Tally::default()
        };
        let mut warning = sample("one tool's schema");
        warning.outcome = Outcome::Warn;
        for _ in 0..14 {
            tally.count(1, &warning);
        }
        assert_eq!(
            tally.line(),
            "mcp-conformance watch: 4 exchange(s); findings so far: 0 fail, 14 warn in 1 clause(s)"
        );
        // Another clause; the same clause in another session and in another
        // revision (a report's rows are per session and revision) are each
        // their own row there, so each is its own here.
        let mut other = warning.clone();
        other.requirement = "BASE-007".to_owned();
        tally.count(1, &other);
        tally.count(2, &warning);
        let mut later = warning.clone();
        later.revision = "2026-07-28".to_owned();
        tally.count(1, &later);
        tally.sessions = 2;
        assert!(
            tally
                .line()
                .ends_with("findings so far: 0 fail, 17 warn in 4 clause(s)"),
            "{}",
            tally.line()
        );
    }

    /// The wire schema is reported beside the clauses, not as one of them.
    #[test]
    fn the_wire_schema_is_not_a_clause() {
        let mut tally = Tally::default();
        let mut schema = sample("x");
        schema.requirement = "wire-schema".to_owned();
        tally.count(1, &schema);
        tally.count(1, &schema);
        assert!(
            tally
                .line()
                .ends_with("findings so far: 2 fail, 0 warn in the wire schema"),
            "{}",
            tally.line()
        );
        tally.count(1, &sample("x"));
        assert!(
            tally
                .line()
                .ends_with("3 fail, 0 warn in 1 clause(s) and the wire schema"),
            "{}",
            tally.line()
        );
    }
}
