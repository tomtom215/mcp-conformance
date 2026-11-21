// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The text inside a `JUnit` case: a finding's location and party, a skipped
//! case's reason, the clause under a failure, the `<properties>` of a case with
//! findings, and XML escaping.

use core::fmt::Write as _;

use crate::report::{Finding, Outcome, Party, RequirementReport};

/// How a failing or warning row stands in a report as it is viewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Standing {
    /// A finding of it counts toward the verdict: a baseline does not accept
    /// it and the party view, if any, does not set it aside.
    Counts,
    /// None counts, and a baseline accepts some: known findings, still there.
    /// Reported as skipped, never as passing.
    Accepted,
    /// None counts, and none is accepted: the party view set them all aside.
    SetAside,
}

/// The standing of a row with these `findings` under `view`. Without a view or
/// a baseline every row counts, as it always has.
pub(super) fn standing(findings: &[Finding], view: Option<Party>) -> Standing {
    let counts = |finding: &Finding| {
        !finding.is_baselined() && view.is_none_or(|party| finding.counts_for(party))
    };
    if findings.iter().any(counts) || (findings.is_empty() && view.is_none()) {
        Standing::Counts
    } else if findings.iter().any(Finding::is_baselined) {
        Standing::Accepted
    } else {
        Standing::SetAside
    }
}

/// The line that heads an accepted case's `<system-out>`, and the `<skipped>`
/// message: the findings are the baseline's, and are listed so that they are
/// not lost.
pub(super) fn accepted_line(findings: &[Finding]) -> String {
    let accepted = findings
        .iter()
        .filter(|finding| finding.is_baselined())
        .count();
    format!(
        "accepted by baseline: {accepted} of {} findings are known and do not count; they are \
         still in the trace\n",
        findings.len()
    )
}

/// `accepted by baseline (reason)` for a finding a baseline accepts, so that a
/// reader of the case's output sees why next to the finding itself.
pub(super) fn acceptance_note(finding: &Finding) -> String {
    finding
        .baselined
        .as_ref()
        .map_or_else(String::new, |acceptance| {
            let until = acceptance
                .expires
                .as_deref()
                .map_or_else(String::new, |day| format!(", through {day}"));
            format!(" [accepted by baseline: {}{until}]", acceptance.reason)
        })
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
