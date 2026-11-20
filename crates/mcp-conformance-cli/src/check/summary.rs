// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What a `check` run exercised: the account printed before the clause
//! report, and written as JSON with `--summary`.
//!
//! The clause report says what the session showed; this says what the session
//! *was* — how many tools were listed, which were called, which were not and
//! why — so a clean report over three requests cannot be mistaken for a clean
//! report over the whole server (ADR-0024, decision 8).

use std::fmt::Write as _;

use mcp_trace_validator::report::printable;
use serde::Serialize;

/// One kind of thing a server exposes: tools, resources or prompts.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Kind {
    /// The server declared the capability, so the kind was listed.
    pub(crate) declared: bool,
    /// How many the server listed.
    pub(crate) listed: usize,
    /// How many were called, read or fetched.
    pub(crate) exercised: usize,
    /// Of those, how many the server answered with a JSON-RPC error.
    pub(crate) errors: usize,
}

/// Something listed and deliberately not exercised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Skipped {
    /// `tool`, `resource template` or `prompt`.
    pub(crate) kind: &'static str,
    /// Its name, as the server listed it.
    pub(crate) name: String,
    /// Why it was left alone.
    pub(crate) reason: String,
}

/// A request sent on purpose to see how the server refuses it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Probe {
    /// What was asked, in words.
    pub(crate) what: String,
    /// `served`, `refused with an error` or `not answered`.
    pub(crate) answer: &'static str,
}

/// The run's account of itself.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Summary {
    /// What was checked, as the user named it (a URL's query is never shown).
    pub(crate) subject: String,
    /// `stdio` or `streamable-http`.
    pub(crate) transport: &'static str,
    /// The protocol revision the session ran under.
    pub(crate) revision: Option<String>,
    /// How that revision was arrived at.
    pub(crate) negotiation: String,
    /// The server's name and version, as it reported them.
    pub(crate) server: Option<String>,
    /// Requests sent.
    pub(crate) requests: usize,
    pub(crate) tools: Kind,
    pub(crate) resources: Kind,
    pub(crate) prompts: Kind,
    /// Listed and not exercised, each with its reason.
    pub(crate) skipped: Vec<Skipped>,
    /// The probes sent, each with how the server answered.
    pub(crate) probes: Vec<Probe>,
    /// Steps the server answered by asking the client for input, which this
    /// client, declaring no capabilities, does not give.
    pub(crate) needed_input: Vec<String>,
    /// Requests the server sent this client.
    pub(crate) server_requests: usize,
    /// Requests that got no JSON-RPC answer, each with what happened instead.
    pub(crate) unanswered: Vec<String>,
    /// The bound that ended the run early, if one did.
    pub(crate) stopped_by: Option<String>,
    /// The recording.
    pub(crate) trace: String,
}

impl Summary {
    /// The account as text, one fact per line.
    pub(crate) fn render(&self) -> String {
        let mut text = format!(
            "mcp-conformance check: {} ({})\n",
            self.subject, self.transport
        );
        let revision = self.revision.as_deref().unwrap_or("none established");
        let _ = writeln!(text, "  revision   {revision} — {}", self.negotiation);
        if let Some(server) = &self.server {
            let _ = writeln!(text, "  server     {}", printable(server));
        }
        kind(&mut text, "tools", "called", &self.tools);
        kind(&mut text, "resources", "read", &self.resources);
        kind(&mut text, "prompts", "fetched", &self.prompts);
        for (label, reasons) in self.skipped_by_kind() {
            let _ = writeln!(text, "  not {label}:");
            for (reason, names) in reasons {
                let _ = writeln!(text, "    {} — {reason}", shown(&names));
            }
        }
        if !self.probes.is_empty() {
            let _ = writeln!(
                text,
                "  probes     {} sent: requests made wrong on purpose, to see how the server \
                 refuses them (--no-probes leaves them out)",
                self.probes.len()
            );
            for probe in &self.probes {
                let _ = writeln!(text, "    {} — {}", printable(&probe.what), probe.answer);
            }
        }
        if !self.needed_input.is_empty() {
            let _ = writeln!(
                text,
                "  asked the client for input, which this client does not give: {}",
                shown(&self.needed_input)
            );
        }
        if self.server_requests > 0 {
            let _ = writeln!(
                text,
                "  the server sent this client {} request(s), though it declared no capabilities",
                self.server_requests
            );
        }
        for one in &self.unanswered {
            let _ = writeln!(text, "  unanswered {}", printable(one));
        }
        if let Some(bound) = &self.stopped_by {
            let _ = writeln!(text, "  stopped early: {bound}");
        }
        let _ = writeln!(text, "  requests   {} sent", self.requests);
        let _ = writeln!(text, "  trace      {}", self.trace);
        text
    }

    /// The skipped things grouped by kind, then by reason, in first-seen order.
    fn skipped_by_kind(&self) -> Vec<(&'static str, ByReason)> {
        let mut kinds: Vec<(&'static str, ByReason)> = Vec::new();
        for one in &self.skipped {
            let label = match one.kind {
                "tool" => "called",
                "prompt" => "fetched",
                _ => "read",
            };
            let position = kinds
                .iter()
                .position(|(known, _)| *known == label)
                .unwrap_or_else(|| {
                    kinds.push((label, Vec::new()));
                    kinds.len() - 1
                });
            let reasons = &mut kinds[position].1;
            match reasons.iter_mut().find(|(reason, _)| *reason == one.reason) {
                Some((_, names)) => names.push(one.name.clone()),
                None => reasons.push((one.reason.clone(), vec![one.name.clone()])),
            }
        }
        kinds
    }
}

/// Names left alone, under the reason they share.
type ByReason = Vec<(String, Vec<String>)>;

/// How many names a line lists before counting the rest.
const NAMES_SHOWN: usize = 8;

/// `names`, the first few spelled out and the rest counted.
fn shown(names: &[String]) -> String {
    let mut text = names
        .iter()
        .take(NAMES_SHOWN)
        .map(|name| printable(name).into_owned())
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > NAMES_SHOWN {
        let _ = write!(text, " and {} more", names.len() - NAMES_SHOWN);
    }
    text
}

fn kind(text: &mut String, label: &str, verb: &str, kind: &Kind) {
    if !kind.declared {
        let _ = writeln!(
            text,
            "  {label:<10} not declared by the server, so not asked for"
        );
        return;
    }
    let _ = write!(
        text,
        "  {label:<10} {} listed, {} {verb}",
        kind.listed, kind.exercised
    );
    if kind.errors > 0 {
        let _ = write!(text, " ({} answered with an error)", kind.errors);
    }
    text.push('\n');
}

#[cfg(test)]
mod tests;
