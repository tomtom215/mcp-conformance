// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The library's numbers: what the dashboards draw and what
//! `mcp-conformance library stats --format json` prints — one computation,
//! serialized once, so a chart and the command cannot disagree.
//!
//! - **Verdicts**, overall and per day (the day a recording was made, as
//!   [`Entry::day`] reads it; a recording with no date is counted as
//!   undated, not placed on a guessed day).
//! - **Clause coverage**, per revision: how many of the registry's clauses
//!   any recording judged (`pass`, `fail` or `warn` in some report), beside
//!   how many the registry excludes from judging at all — the rest were
//!   never observed by any recording in the library. Per area too.
//! - **Findings** by area (a requirement ID's prefix; the wire schema is its
//!   own area) and by server (`serverInfo`'s name and version).

use std::collections::BTreeMap;

use mcp_trace_validator::report::Outcome;
use serde::Serialize;

use super::entry::{Entry, UNJUDGED};
use super::meta::tool_version;

/// The verdict words, in the order they are counted.
pub(crate) const VERDICTS: [&str; 5] = [
    "pass",
    "pass-with-warnings",
    "fail",
    "unsupported",
    UNJUDGED,
];

/// The area the wire schema's findings count under.
pub(crate) const WIRE_SCHEMA: &str = "wire-schema";

/// Everything the dashboards show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Stats {
    pub(crate) tool_version: &'static str,
    pub(crate) sessions: usize,
    pub(crate) verdicts: BTreeMap<&'static str, usize>,
    pub(crate) over_time: Vec<Day>,
    pub(crate) undated: usize,
    pub(crate) coverage: Vec<Coverage>,
    pub(crate) findings_by_area: Vec<Count>,
    pub(crate) findings_by_server: Vec<ServerCount>,
}

/// One day's verdicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Day {
    pub(crate) day: String,
    pub(crate) verdicts: BTreeMap<&'static str, usize>,
}

/// One revision's clause coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Coverage {
    pub(crate) revision: String,
    /// Recordings judged at this revision.
    pub(crate) sessions: usize,
    pub(crate) clauses: usize,
    pub(crate) excluded: usize,
    pub(crate) exercised: usize,
    pub(crate) areas: Vec<AreaCoverage>,
}

/// One area's clause coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AreaCoverage {
    pub(crate) area: String,
    pub(crate) clauses: usize,
    pub(crate) excluded: usize,
    pub(crate) exercised: usize,
}

/// Findings under one name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Count {
    pub(crate) area: String,
    pub(crate) findings: usize,
    /// Recordings with at least one such finding.
    pub(crate) sessions: usize,
}

/// One server's recordings and findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ServerCount {
    pub(crate) server: String,
    pub(crate) sessions: usize,
    /// Recordings whose verdict is `fail`.
    pub(crate) failing: usize,
    pub(crate) findings: usize,
}

/// The area of a requirement ID: its prefix (`BASE` of `BASE-003`).
fn area(id: &str) -> String {
    id.split_once('-').map_or(id, |(area, _)| area).to_owned()
}

fn zeroes() -> BTreeMap<&'static str, usize> {
    VERDICTS.iter().map(|verdict| (*verdict, 0)).collect()
}

/// One clause's state across the library, at one revision.
#[derive(Debug, Default, Clone, Copy)]
struct Clause {
    excluded: bool,
    exercised: bool,
}

/// The running counts while entries are read.
#[derive(Default)]
struct Tally {
    days: BTreeMap<String, BTreeMap<&'static str, usize>>,
    clauses: BTreeMap<String, BTreeMap<String, Clause>>,
    judged_at: BTreeMap<String, usize>,
    areas: BTreeMap<String, (usize, usize)>,
    servers: BTreeMap<String, ServerCount>,
}

impl Tally {
    /// Counts one entry's reports: its clauses and its findings by area.
    fn reports(&mut self, entry: &Entry) {
        let mut found_in: BTreeMap<String, usize> = BTreeMap::new();
        for report in &entry.reports {
            *self.judged_at.entry(report.revision.clone()).or_default() += 1;
            let revision = self.clauses.entry(report.revision.clone()).or_default();
            for row in &report.requirements {
                let clause = revision.entry(row.id.clone()).or_default();
                clause.excluded |= row.outcome == Outcome::Excluded;
                clause.exercised |=
                    matches!(row.outcome, Outcome::Pass | Outcome::Fail | Outcome::Warn);
                if !row.findings.is_empty() {
                    *found_in.entry(area(&row.id)).or_default() += row.findings.len();
                }
            }
            if let Some(schema) = report
                .wire_schema
                .as_ref()
                .filter(|s| !s.findings.is_empty())
            {
                *found_in.entry(WIRE_SCHEMA.to_owned()).or_default() += schema.findings.len();
            }
        }
        for (name, findings) in found_in {
            let (total, sessions) = self.areas.entry(name).or_default();
            *total += findings;
            *sessions += 1;
        }
    }

    /// Counts one entry under its server.
    fn server(&mut self, entry: &Entry, word: &str) {
        let server = entry
            .facts
            .server
            .clone()
            .unwrap_or_else(|| "(server not recorded)".to_owned());
        let count = self.servers.entry(server.clone()).or_insert(ServerCount {
            server,
            sessions: 0,
            failing: 0,
            findings: 0,
        });
        count.sessions += 1;
        count.failing += usize::from(word == "fail");
        count.findings += entry.findings();
    }
}

/// The numbers of `entries`.
pub(crate) fn compute<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Stats {
    let mut stats = Stats {
        tool_version: tool_version(),
        sessions: 0,
        verdicts: zeroes(),
        over_time: Vec::new(),
        undated: 0,
        coverage: Vec::new(),
        findings_by_area: Vec::new(),
        findings_by_server: Vec::new(),
    };
    let mut tally = Tally::default();
    for entry in entries {
        stats.sessions += 1;
        let verdict = entry.verdict();
        let Some(word) = VERDICTS.iter().copied().find(|word| *word == verdict) else {
            continue;
        };
        *stats.verdicts.entry(word).or_default() += 1;
        match entry.day() {
            Some(day) => {
                *tally
                    .days
                    .entry(day)
                    .or_insert_with(zeroes)
                    .entry(word)
                    .or_default() += 1;
            }
            None => stats.undated += 1,
        }
        tally.reports(entry);
        tally.server(entry, word);
    }
    let Tally {
        days,
        clauses,
        judged_at,
        areas,
        servers,
    } = tally;
    stats.over_time = days
        .into_iter()
        .map(|(day, verdicts)| Day { day, verdicts })
        .collect();
    stats.coverage = clauses
        .into_iter()
        .map(|(revision, ids)| coverage(revision, &ids, &judged_at))
        .collect();
    stats.findings_by_area = areas
        .into_iter()
        .map(|(area, (findings, sessions))| Count {
            area,
            findings,
            sessions,
        })
        .collect();
    stats.findings_by_server = servers.into_values().collect();
    stats
}

fn coverage(
    revision: String,
    ids: &BTreeMap<String, Clause>,
    judged_at: &BTreeMap<String, usize>,
) -> Coverage {
    let mut by_area: BTreeMap<String, AreaCoverage> = BTreeMap::new();
    for (id, clause) in ids {
        let name = area(id);
        let row = by_area.entry(name.clone()).or_insert(AreaCoverage {
            area: name,
            clauses: 0,
            excluded: 0,
            exercised: 0,
        });
        row.clauses += 1;
        row.excluded += usize::from(clause.excluded && !clause.exercised);
        row.exercised += usize::from(clause.exercised);
    }
    let areas: Vec<AreaCoverage> = by_area.into_values().collect();
    let sum = |pick: fn(&AreaCoverage) -> usize| areas.iter().map(pick).sum();
    Coverage {
        sessions: judged_at.get(&revision).copied().unwrap_or_default(),
        clauses: sum(|area| area.clauses),
        excluded: sum(|area| area.excluded),
        exercised: sum(|area| area.exercised),
        revision,
        areas,
    }
}

/// The JSON both the dashboards and `library stats --format json` read.
pub(crate) fn to_json(stats: &Stats) -> String {
    let mut text = serde_json::to_string_pretty(stats).unwrap_or_default();
    text.push('\n');
    text
}

/// `text` for a terminal: control characters (an escape sequence in a
/// recorded server name would otherwise drive the terminal) and the Unicode
/// bidirectional controls shown as `\u{…}`, as the HTML report shows them.
pub(crate) fn visible(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                character.escape_unicode().to_string()
            } else {
                character.to_string()
            }
        })
        .collect()
}

/// The same numbers as text, for a terminal.
pub(crate) fn to_human(stats: &Stats) -> String {
    use core::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} recordings (mcp-conformance {})",
        stats.sessions, stats.tool_version
    );
    for (verdict, count) in &stats.verdicts {
        let _ = writeln!(out, "  {verdict:<20} {count}");
    }
    if stats.undated > 0 {
        let _ = writeln!(out, "  {} without a recording date", stats.undated);
    }
    for coverage in &stats.coverage {
        let _ = writeln!(
            out,
            "coverage {}: {} of {} clauses judged by some recording ({} excluded from judging, {} recordings)",
            coverage.revision,
            coverage.exercised,
            coverage.clauses,
            coverage.excluded,
            coverage.sessions
        );
    }
    for area in &stats.findings_by_area {
        let _ = writeln!(
            out,
            "findings {:<12} {} in {} recordings",
            area.area, area.findings, area.sessions
        );
    }
    for server in &stats.findings_by_server {
        let _ = writeln!(
            out,
            "server {}: {} recordings, {} failing, {} findings",
            visible(&server.server),
            server.sessions,
            server.failing,
            server.findings
        );
    }
    out
}

#[cfg(test)]
mod tests;
