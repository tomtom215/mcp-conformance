// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What `watch` writes on stderr: each new finding, a one-line tally, and the
//! report at the end.
//!
//! Everything quoted from the session goes through the validator's
//! [`printable`]: a recording is untrusted input, and an `ESC` in a method name
//! must not reach the terminal raw.

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
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Tally {
    pub(super) sessions: usize,
    pub(super) exchanges: u64,
    pub(super) fail: u64,
    pub(super) warn: u64,
}

impl Tally {
    pub(super) fn count(&mut self, finding: &LiveFinding) {
        if finding.outcome == Outcome::Warn {
            self.warn += 1;
        } else {
            self.fail += 1;
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
            "{PREFIX} {} exchange(s){sessions}; findings so far: {} fail, {} warn",
            self.exchanges, self.fail, self.warn
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
        tally.count(&sample("x"));
        assert_eq!(
            tally.line(),
            "mcp-conformance watch: 3 exchange(s); findings so far: 1 fail, 0 warn"
        );
        tally.sessions = 2;
        assert!(tally.line().contains(" in 2 sessions;"));
        assert!(!tally.line().contains('\n'));
    }
}
