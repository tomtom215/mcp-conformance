// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The text inside a `JUnit` case: a finding's location and party, a skipped
//! case's reason, the clause under a failure, the `<properties>` of a case with
//! findings, and XML escaping.

use core::fmt::Write as _;

use crate::report::{Finding, Outcome, Party, RequirementReport};

/// Whether a row's findings count under `view`: always, without one;
/// otherwise when any of them does ([`Finding::counts_for`]).
pub(super) fn counted(findings: &[Finding], view: Option<Party>) -> bool {
    view.is_none_or(|party| findings.iter().any(|finding| finding.counts_for(party)))
}

/// The `<properties>` of a case with findings: the party the clause binds, and
/// the parties the findings are held against (in party order, once each).
pub(super) fn properties(out: &mut String, actor: Option<&str>, findings: &[Finding]) {
    let mut parties: Vec<Party> = findings
        .iter()
        .filter_map(|finding| finding.party)
        .collect();
    parties.sort_unstable();
    parties.dedup();
    out.push_str("      <properties>");
    if let Some(actor) = actor {
        let _ = write!(out, r#"<property name="actor" value="{}"/>"#, escape(actor));
    }
    if !parties.is_empty() {
        let value: Vec<&str> = parties.iter().map(|party| party.as_str()).collect();
        let _ = write!(
            out,
            r#"<property name="party" value="{}"/>"#,
            escape(&value.join(", "))
        );
    }
    out.push_str("</properties>\n");
}

/// The `actor` property's value: the registry's token for the party a clause
/// binds, when the row names one.
pub(super) const fn actor_token(row: &RequirementReport) -> Option<&'static str> {
    match row.actor {
        Some(mcp_conformance_core::requirement::Actor::Client) => Some("client"),
        Some(mcp_conformance_core::requirement::Actor::Server) => Some("server"),
        Some(mcp_conformance_core::requirement::Actor::Both) => Some("both"),
        // Foreign #[non_exhaustive] enum: a future party surfaces visibly.
        Some(_) => Some("unknown"),
        None => None,
    }
}

/// The line that heads a set-aside case's `<system-out>`: the view counts
/// `party`'s findings, and every finding here is the other party's.
pub(super) fn set_aside_line(party: Party) -> String {
    format!(
        "set aside: this report counts {party} findings only, and every finding here is \
         held against the {}\n",
        party.other()
    )
}

/// The `<skipped>` message for the four non-judged outcomes.
pub(super) fn skip_reason(row: &RequirementReport) -> String {
    match row.outcome {
        Outcome::Excluded => row
            .exclusion
            .clone()
            .unwrap_or_else(|| "excluded".to_owned()),
        Outcome::NotObserved => {
            "the session carried none of the traffic this clause binds to".to_owned()
        }
        Outcome::NotApplicable => format!(
            "not applicable: capability {} was not declared in this session",
            row.capability.as_deref().unwrap_or("(unknown)")
        ),
        _ => format!(
            "registry references checks this build does not implement: {}",
            row.missing_checks.join(", ")
        ),
    }
}

/// The violated clause as body text — `\nspec: "…"\nsee: <url>` — or nothing when
/// the row carries no source. CI systems show a failure's body beside its message,
/// so this is what puts the clause in front of the person reading the failure.
pub(super) fn clause_lines(row: &RequirementReport) -> String {
    row.source.as_ref().map_or_else(String::new, |source| {
        format!("\nspec: \"{}\"\nsee: {}", source.quote, source.url)
    })
}

/// Where a finding points and who it is held against: `[check] at seq 3
/// (client)`, each part only when known.
pub(super) fn location(finding: &Finding) -> String {
    let mut text = format!("[{}]", finding.check);
    if let Some(seq) = finding.seq {
        let _ = write!(text, " at seq {seq}");
    }
    if let Some(party) = finding.party {
        let _ = write!(text, " ({party})");
    }
    text
}

/// XML escaping for text and attribute content (we always double-quote
/// attributes, so escaping `"` but not `'` is sufficient).
///
/// Findings quote trace strings — method names, ids — that come from untrusted
/// input and may carry characters XML 1.0 forbids entirely. C0 control
/// characters other than tab/LF/CR cannot appear in an XML 1.0 document even as
/// numeric references (XML 1.0 §2.2), so a trace whose method name contains,
/// say, U+0001 would otherwise produce a document a strict CI parser rejects.
/// Those characters are replaced with U+FFFD (the Unicode replacement
/// character) so the output is always well-formed.
pub(super) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(ch),
            // C0 controls (except the three above), U+FFFE and U+FFFF are
            // outside XML 1.0's `Char` production (§2.2) — the only Rust
            // `char`s that are — so substitute rather than emit an invalid
            // document.
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push('\u{FFFD}'),
            _ => out.push(ch),
        }
    }
    out
}
