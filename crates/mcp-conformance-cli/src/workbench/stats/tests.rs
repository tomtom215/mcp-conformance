// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;
use crate::workbench::fixture;

#[test]
fn the_fixture_library_counts_as_its_recordings_say() {
    let (workbench, _dir) = fixture::workbench("stats");
    let stats = compute(workbench.entries.values());
    assert_eq!(stats.sessions, 11);
    assert_eq!(stats.verdicts["pass"], 6);
    // `sdk-rust-2026-07-28` warns on BASE-037 (rmcp's `serverInfo`).
    assert_eq!(stats.verdicts["pass-with-warnings"], 4);
    assert_eq!(stats.verdicts["fail"], 1);
    assert_eq!(stats.verdicts["unjudged"], 0);
    assert_eq!(stats.undated, 0);
    let days: Vec<&str> = stats.over_time.iter().map(|day| day.day.as_str()).collect();
    assert_eq!(
        days,
        [
            "2026-09-29",
            "2026-10-02",
            "2026-10-06",
            "2026-10-09",
            "2026-10-11",
            "2026-10-12"
        ]
    );
    assert_eq!(stats.over_time[3].verdicts["fail"], 1);
    let total: usize = stats
        .over_time
        .iter()
        .flat_map(|day| day.verdicts.values())
        .sum();
    assert_eq!(total, 11);
    let revisions: Vec<&str> = stats.coverage.iter().map(|c| c.revision.as_str()).collect();
    assert_eq!(revisions, ["2025-11-25", "2026-07-28"]);
    for coverage in &stats.coverage {
        assert_eq!(
            coverage.clauses,
            coverage.areas.iter().map(|a| a.clauses).sum::<usize>()
        );
        assert!(coverage.exercised + coverage.excluded <= coverage.clauses);
    }
    assert_eq!(stats.coverage[0].sessions, 10);
    let base = stats
        .findings_by_area
        .iter()
        .find(|a| a.area == "BASE")
        .unwrap();
    // base-001's BASE-001, sdk-typescript's BASE-086 and sdk-rust-2026-07-28's
    // BASE-037.
    assert_eq!(base.sessions, 3);
    assert!(stats.findings_by_area.iter().any(|a| a.area == WIRE_SCHEMA));
    let corpus = stats
        .findings_by_server
        .iter()
        .find(|s| s.server == "corpus-server 0.1.0")
        .unwrap();
    assert_eq!((corpus.sessions, corpus.failing), (4, 1));
    let findings: usize = workbench.entries.values().map(Entry::findings).sum();
    assert_eq!(
        stats
            .findings_by_server
            .iter()
            .map(|s| s.findings)
            .sum::<usize>(),
        findings
    );
}

#[test]
fn an_empty_library_has_every_verdict_at_zero() {
    let stats = compute(core::iter::empty());
    assert_eq!(stats.sessions, 0);
    assert_eq!(stats.verdicts.len(), VERDICTS.len());
    assert!(stats.verdicts.values().all(|count| *count == 0));
    assert!(to_json(&stats).ends_with("}\n"));
    assert!(to_human(&stats).starts_with("0 recordings"));
}

#[test]
fn the_text_form_says_what_the_json_says() {
    let (workbench, _dir) = fixture::workbench("human");
    let stats = compute(workbench.entries.values());
    let text = to_human(&stats);
    assert!(text.contains("coverage 2025-11-25:"), "{text}");
    assert!(
        text.contains("server corpus-server 0.1.0: 4 recordings, 1 failing"),
        "{text}"
    );
    assert_eq!(
        visible("a\u{1b}[2J\u{202e}b\tc"),
        "a\\u{1b}[2J\\u{202e}b\\u{9}c"
    );
    assert_eq!(visible("plain é"), "plain é");
    let json: serde_json::Value = serde_json::from_str(&to_json(&stats)).unwrap();
    assert_eq!(json["sessions"], 11);
    assert_eq!(area("BASE-003"), "BASE");
    assert_eq!(area("plain"), "plain");
}

/// `entry` judged at revision `R` with only `rows` (`[id, outcome,
/// findings]`), and a wire schema with `schema_findings` findings.
fn judged(entry: &Entry, rows: &[(&str, &str, usize)], schema_findings: usize) -> Entry {
    let finding = |n: usize| serde_json::json!({"check": "c", "detail": format!("d{n}")});
    let mut entry = entry.clone();
    entry.reports.truncate(1);
    let report = &mut entry.reports[0];
    report.revision = "R".to_owned();
    report.requirements = rows
        .iter()
        .map(|(id, outcome, findings)| {
            serde_json::from_value(serde_json::json!({
                "id": id, "level": "MUST", "outcome": outcome,
                "findings": (0..*findings).map(finding).collect::<Vec<_>>(),
            }))
            .unwrap()
        })
        .collect();
    let schema = report.wire_schema.as_mut().unwrap();
    schema.findings = (0..schema_findings)
        .map(|n| serde_json::from_value(finding(n)).unwrap())
        .collect();
    entry
}

#[test]
fn clauses_and_findings_are_counted_exactly() {
    let (workbench, _dir) = fixture::workbench("exact");
    let base = workbench.entry("everything-http").unwrap();
    let first = judged(
        base,
        &[
            ("A-1", "pass", 0),
            ("A-2", "excluded", 0),
            ("A-3", "fail", 2),
            ("B-1", "not-applicable", 0),
            ("B-2", "excluded", 0),
        ],
        3,
    );
    // B-2, excluded in one recording, is judged in another: it counts as
    // exercised, not excluded.
    let second = judged(base, &[("B-2", "pass", 0), ("A-3", "warn", 1)], 0);
    let stats = compute([&first, &second]);
    let coverage = &stats.coverage[0];
    assert_eq!(coverage.revision, "R");
    assert_eq!(
        (
            coverage.sessions,
            coverage.clauses,
            coverage.excluded,
            coverage.exercised
        ),
        (2, 5, 1, 3)
    );
    let areas: Vec<(&str, usize, usize, usize)> = coverage
        .areas
        .iter()
        .map(|a| (a.area.as_str(), a.clauses, a.excluded, a.exercised))
        .collect();
    assert_eq!(areas, [("A", 3, 1, 2), ("B", 2, 0, 1)]);
    let found: Vec<(&str, usize, usize)> = stats
        .findings_by_area
        .iter()
        .map(|c| (c.area.as_str(), c.findings, c.sessions))
        .collect();
    assert_eq!(found, [("A", 3, 2), (WIRE_SCHEMA, 3, 1)]);

    let mut text = to_human(&stats);
    assert!(!text.contains("without a recording date"), "{text}");
    let mut undated = stats;
    undated.undated = 1;
    text = to_human(&undated);
    assert!(text.contains("\n  1 without a recording date\n"), "{text}");
}

#[test]
fn an_undated_recording_is_counted_not_placed() {
    let dir = fixture::scratch("undated");
    std::fs::copy(
        fixture::fixtures().join("everything-http.jsonl"),
        dir.join("a.jsonl"),
    )
    .unwrap();
    // No sidecar, no timestamps in the trace: only the file's time is left,
    // which the library uses; a recording with none at all is undated.
    let workbench = crate::workbench::state::Workbench::open(&dir).unwrap();
    let mut entry = workbench.entry("a").unwrap().clone();
    entry.modified = None;
    entry.meta.added_at = None;
    entry.meta.recorded_at = None;
    let stats = compute([&entry]);
    assert_eq!((stats.undated, stats.over_time.len()), (1, 0));
}
