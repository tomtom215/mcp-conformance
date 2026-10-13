// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Every committed corpus trace, replayed line by line through the live judge
//! `watch` uses, held to the validator's own judgment of the same file.
//!
//! Three properties, each over every trace the validator judges:
//!
//! 1. **Nothing false is printed.** No finding reported live is missing from
//!    the final judgment (none is withdrawn).
//! 2. **Nothing is missed, nothing twice.** Live findings plus the end's are
//!    exactly the final judgment's `fail` and `warn` findings, each once.
//! 3. **Within the exchange.** Let line `v` be the first line whose prefix of
//!    the trace, judged whole by the validator, shows a finding — usually the
//!    violating message itself; later when the evidence arrives later (a
//!    session that turns out to speak a second revision). The finding is
//!    reported by the first judging pass at or after line `v`: the pass of the
//!    exchange that line completes, or that completes after it. When no pass
//!    follows, and for the [`EVENTUAL_CHECKS`] (which a prefix cannot settle),
//!    it is reported at the end.

#![allow(clippy::expect_used, clippy::too_many_lines)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use mcp_conformance_cli::live::{EVENTUAL_CHECKS, Live, LiveFinding};
use mcp_trace_validator::judge::{self, Judgment};
use mcp_trace_validator::report::Outcome;

fn traces() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut found = Vec::new();
    let mut dirs = vec![root];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("the corpus is readable") {
            let path = entry.expect("a corpus entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

type Key = (String, String, String, Option<u64>, String);

fn key(finding: &LiveFinding) -> Key {
    (
        finding.revision.clone(),
        finding.requirement.clone(),
        finding.check.clone(),
        finding.seq,
        finding.detail.clone(),
    )
}

/// The `fail` and `warn` findings of a judgment, as the live judge keys them.
fn findings(judgment: &Judgment) -> BTreeSet<Key> {
    let mut found = BTreeSet::new();
    for report in &judgment.reports {
        for row in &report.requirements {
            if !matches!(row.outcome, Outcome::Fail | Outcome::Warn) {
                continue;
            }
            for finding in &row.findings {
                found.insert((
                    report.revision.clone(),
                    row.id.clone(),
                    finding.check.clone(),
                    finding.seq,
                    finding.detail.clone(),
                ));
            }
        }
        if let Some(schema) = &report.wire_schema
            && schema.outcome == Outcome::Fail
        {
            for finding in &schema.findings {
                found.insert((
                    report.revision.clone(),
                    "wire-schema".to_owned(),
                    finding.check.clone(),
                    finding.seq,
                    finding.detail.clone(),
                ));
            }
        }
    }
    found
}

/// For each line, the findings the trace up to and including it shows.
fn visible_by_line(lines: &[&str]) -> Vec<BTreeSet<Key>> {
    (0..lines.len())
        .map(|last| {
            let prefix = lines[..=last].join("\n");
            judge::judge(&prefix)
                .map(|judgment| findings(&judgment))
                .unwrap_or_default()
        })
        .collect()
}

#[test]
fn every_corpus_trace_judged_live_agrees_with_the_validator_within_the_exchange() {
    let at = |index: usize| UNIX_EPOCH + Duration::from_millis(index as u64);
    let mut judged = 0;
    let mut live_reports = 0;
    let mut late = Vec::new();
    for path in traces() {
        let document = std::fs::read_to_string(&path).expect("a corpus trace");
        let Ok(judgment) = judge::judge(&document) else {
            continue; // a trace the validator refuses has no findings to agree with
        };
        judged += 1;
        let expected = findings(&judgment);

        let mut live = Live::new().expect("the built-in registries load");
        let lines: Vec<&str> = document.lines().collect();
        let mut passes: Vec<usize> = Vec::new();
        let mut reported: Vec<(Key, Option<usize>)> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if live.push(line.as_bytes(), at(index)).due {
                passes.push(index);
                for finding in live.judge().new {
                    reported.push((key(&finding), Some(index)));
                }
            }
        }
        let ended = live.finish(at(lines.len()));
        assert!(
            ended.withdrawn.is_empty(),
            "{}: printed live, then withdrawn: {:?}",
            path.display(),
            ended.withdrawn
        );
        reported.extend(ended.new.iter().map(|finding| (key(finding), None)));
        let got: BTreeSet<Key> = reported.iter().map(|(key, _)| key.clone()).collect();
        assert_eq!(got, expected, "{}", path.display());
        assert_eq!(got.len(), reported.len(), "{}: twice", path.display());

        let line_of = |seq: u64| {
            lines.iter().position(|line| {
                serde_json::from_str::<serde_json::Value>(line)
                    .ok()
                    .and_then(|event| event.get("seq").and_then(serde_json::Value::as_u64))
                    == Some(seq)
            })
        };
        let mut visible: Option<Vec<BTreeSet<Key>>> = None;
        for (key, when) in &reported {
            live_reports += usize::from(when.is_some());
            if EVENTUAL_CHECKS.contains(&key.2.as_str()) {
                assert!(when.is_none(), "{}: {key:?} live", path.display());
                continue;
            }
            // The common case, cheaply: reported by the pass at or after the
            // event the finding points at, which no prefix can show it before.
            let at_event = key
                .3
                .and_then(line_of)
                .and_then(|line| passes.iter().find(|pass| **pass >= line));
            if when.is_some() && when.as_ref() <= at_event {
                continue;
            }
            let visible = visible.get_or_insert_with(|| visible_by_line(&lines));
            let shown = visible.iter().position(|found| found.contains(key));
            let bound = shown.and_then(|shown| passes.iter().find(|pass| **pass >= shown));
            let in_time = match (when, bound) {
                (Some(when), Some(bound)) => when <= bound,
                (None, Some(_)) => false,
                (_, None) => true,
            };
            if !in_time {
                late.push(format!(
                    "{}: {key:?} at {when:?}, visible from line {shown:?}, bound {bound:?}",
                    path.display()
                ));
            }
        }
    }
    assert!(judged >= 60, "only {judged} corpus traces judged");
    assert!(
        live_reports >= 40,
        "only {live_reports} findings reported live"
    );
    assert!(late.is_empty(), "reported late:\n{}", late.join("\n"));
}
