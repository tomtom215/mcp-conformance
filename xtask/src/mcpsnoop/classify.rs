// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What the two readings of one trace amount to, rule by rule.
//!
//! A mcpsnoop finding belongs to every rule one of whose templates occurs in
//! its message, and a rule stands for the clauses `rules.json` maps it to at
//! the trace's revision. From there:
//!
//! - **agreed** — a rule fired and we flag one of its clauses;
//! - **mcpsnoop-only** — a rule fired and we flag none of its clauses;
//! - **ours-only** — we flag a clause some rule maps *fully* (not `partial`)
//!   and no such rule fired: mcpsnoop checks this condition and stayed silent;
//! - **beyond mcpsnoop** — we flag a clause no rule maps fully. A difference
//!   in scope, not a disagreement, and counted as such.
//!
//! A finding is **set aside** when the rule reads HTTP context the conversion
//! could not supply: a header or status rule on a frame that arrived without
//! its context, or a pairing rule anywhere in a trace where some frame did.
//! mcpsnoop would be reading an absence the trace never recorded.

use std::collections::{BTreeMap, BTreeSet};

use super::data::{Rule, TraceResult};

/// Whether `template` occurs in `message`, `*` standing for any run.
pub(crate) fn matches(template: &str, message: &str) -> bool {
    let mut rest = message;
    for piece in template.split('*').filter(|piece| !piece.is_empty()) {
        match rest.find(piece) {
            Some(at) => rest = &rest[at + piece.len()..],
            None => return false,
        }
    }
    true
}

/// The rules a message belongs to.
pub(crate) fn rules_for<'r>(rules: &'r [Rule], message: &str) -> Vec<&'r Rule> {
    rules
        .iter()
        .filter(|rule| {
            rule.templates
                .iter()
                .any(|template| matches(template, message))
        })
        .collect()
}

/// Where a trace lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Category {
    /// Neither tool flags anything.
    Clean,
    /// Both flag, and every finding is accounted for on both sides.
    Agree,
    /// mcpsnoop is silent and we flag only clauses it has no full rule for.
    Beyond,
    /// At least one mcpsnoop-only or ours-only divergence.
    Divergent,
}

impl Category {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Clean => "both clean",
            Self::Agree => "agree",
            Self::Beyond => "beyond mcpsnoop",
            Self::Divergent => "**divergent**",
        }
    }
}

/// One trace's two readings, compared.
#[derive(Debug, Clone)]
pub(crate) struct Comparison<'a> {
    pub trace: &'a TraceResult,
    /// Rules that fired, with the events they named.
    pub fired: BTreeMap<&'a str, BTreeSet<Option<u64>>>,
    /// Rules that fired only on frames the conversion could not frame.
    pub set_aside: BTreeSet<&'a str>,
    /// Fired rules none of whose clauses we flag.
    pub snoop_only: Vec<&'a str>,
    /// Clauses we flag that a full rule maps, none of which fired.
    pub ours_only: Vec<&'a str>,
    /// Clauses we flag that no rule maps fully.
    pub beyond: Vec<&'a str>,
    /// Finding messages no rule claims; `--check` refuses these.
    pub unclaimed: Vec<&'a str>,
}

impl Comparison<'_> {
    /// Whether mcpsnoop's conformance reading fails the trace.
    pub(crate) fn snoop_fails(&self) -> bool {
        !self.fired.is_empty()
    }

    /// Whether our report flags anything at all.
    pub(crate) const fn ours_flags(&self) -> bool {
        !self.trace.ours.flagged.is_empty()
    }

    pub(crate) fn category(&self) -> Category {
        if !self.snoop_only.is_empty() || !self.ours_only.is_empty() {
            Category::Divergent
        } else if self.snoop_fails() {
            Category::Agree
        } else if self.ours_flags() {
            Category::Beyond
        } else {
            Category::Clean
        }
    }

    /// Every divergence, as `(kind, subject)`.
    pub(crate) fn divergences(&self) -> Vec<(&'static str, &str)> {
        let mut out: Vec<(&'static str, &str)> = self
            .snoop_only
            .iter()
            .map(|rule| ("mcpsnoop-only", *rule))
            .collect();
        out.extend(self.ours_only.iter().map(|id| ("ours-only", *id)));
        out
    }
}

fn mapped<'r>(rule: &'r Rule, revision: &str) -> &'r [String] {
    rule.maps.get(revision).map_or(&[], Vec::as_slice)
}

/// Which rules each finding belongs to: fired, set aside, or claimed by none.
type Sorted<'a> = (
    BTreeMap<&'a str, BTreeSet<Option<u64>>>,
    BTreeSet<&'a str>,
    Vec<&'a str>,
);

fn sort_findings<'a>(trace: &'a TraceResult, rules: &'a [Rule]) -> Sorted<'a> {
    let mut fired: BTreeMap<&str, BTreeSet<Option<u64>>> = BTreeMap::new();
    let mut set_aside = BTreeSet::new();
    let mut unclaimed = Vec::new();
    for finding in &trace.mcpsnoop.findings {
        let claimed = rules_for(rules, &finding.message);
        if claimed.is_empty() {
            unclaimed.push(finding.message.as_str());
        }
        for rule in claimed {
            let blind = match rule.context.as_deref() {
                Some("http") => !finding.framed,
                Some("pairing") => trace.unframed > 0,
                _ => false,
            };
            if blind {
                set_aside.insert(rule.id.as_str());
            } else {
                fired
                    .entry(rule.id.as_str())
                    .or_default()
                    .insert(finding.seq);
            }
        }
    }
    (fired, set_aside, unclaimed)
}

/// Compares one trace's readings under `rules`.
pub(crate) fn compare<'a>(trace: &'a TraceResult, rules: &'a [Rule]) -> Comparison<'a> {
    let revision = trace.revision.as_str();
    let flagged: BTreeSet<&str> = trace.ours.flagged.iter().map(String::as_str).collect();
    let (fired, mut set_aside, unclaimed) = sort_findings(trace, rules);
    set_aside.retain(|rule| !fired.contains_key(rule));
    let mut snoop_only = Vec::new();
    for rule in rules
        .iter()
        .filter(|rule| fired.contains_key(rule.id.as_str()))
    {
        if !mapped(rule, revision)
            .iter()
            .any(|id| flagged.contains(id.as_str()))
        {
            snoop_only.push(rule.id.as_str());
        }
    }
    let mut ours_only = Vec::new();
    let mut beyond = Vec::new();
    for id in &trace.ours.flagged {
        let full = rules
            .iter()
            .any(|rule| mapped(rule, revision).contains(id) && !rule.partial.contains(id));
        let answered = rules.iter().any(|rule| {
            mapped(rule, revision).contains(id)
                && (fired.contains_key(rule.id.as_str()) || set_aside.contains(rule.id.as_str()))
        });
        if !full {
            beyond.push(id.as_str());
        } else if !answered {
            ours_only.push(id.as_str());
        }
    }
    Comparison {
        trace,
        fired,
        set_aside,
        snoop_only,
        ours_only,
        beyond,
        unclaimed,
    }
}
