// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;
use crate::workbench::fixture;

#[test]
fn the_fixture_library_counts_as_its_recordings_say() {
    let (workbench, _) = fixture::workbench("stats");
    let stats = compute(workbench.entries.values());
    assert_eq!(stats.sessions, 11);
    assert_eq!(stats.verdicts["pass"], 7);
    assert_eq!(stats.verdicts["pass-with-warnings"], 3);
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
    assert_eq!(base.sessions, 2);
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
    let (workbench, _) = fixture::workbench("human");
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
