// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! SARIF output over the whole violation corpus: every finding becomes exactly
//! one result, located at the trace line that holds its event, under a rule
//! that carries the clause.
//!
//! Conformance to the SARIF 2.1.0 schema itself is checked by the ignored test
//! below, which CI runs with the OASIS schema downloaded and pinned by hash
//! (`.github/workflows/ci.yml`). The schema is not vendored: its licence is the
//! OASIS IPR policy, not an open-source one.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use mcp_conformance_core::requirement::RegistrySet;
use mcp_trace_validator::report::{Outcome, Report};
use mcp_trace_validator::{declared, engine, reader, sarif};
use serde_json::Value;

fn violation_traces() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut traces = Vec::new();
    for dir in ["violations", "draft/violations"] {
        for entry in std::fs::read_dir(root.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            {
                traces.push(path);
            }
        }
    }
    traces.sort();
    assert!(traces.len() > 50, "found {} traces", traces.len());
    traces
}

/// The reports for `path` under the revisions it declares — or, for the traces
/// that declare only a revision this build refuses to judge, under every
/// supported one, which exercises multi-revision logs too — and its text.
fn judge(path: &Path) -> (Vec<Report>, String) {
    let text = std::fs::read_to_string(path).unwrap();
    let events = reader::parse_trace(&text, &reader::Limits::default()).unwrap();
    let set = RegistrySet::builtin().unwrap();
    let revisions = declared::select(set.revisions(), &events).map_or_else(
        |_| set.revisions().to_vec(),
        |selection| selection.revisions,
    );
    let reports = revisions
        .iter()
        .map(|revision| engine::validate(&set.registry(*revision).unwrap(), &events))
        .collect();
    (reports, text)
}

fn render(path: &Path) -> (Value, Vec<Report>, String) {
    let (reports, text) = judge(path);
    let events = reader::parse_trace(&text, &reader::Limits::default()).unwrap();
    let log = sarif::render(
        &reports,
        sarif::Artifact {
            uri: Some("trace.jsonl"),
            events: &events,
        },
    );
    (serde_json::from_str(&log).unwrap(), reports, text)
}

#[test]
fn every_finding_is_one_result_at_its_events_line_under_its_clause() {
    for path in violation_traces() {
        let (log, reports, text) = render(&path);
        let run = &log["runs"][0];
        let results = run["results"].as_array().unwrap();
        let rules = run["tool"]["driver"]["rules"].as_array().unwrap();
        let findings: Vec<_> = reports
            .iter()
            .flat_map(|report| &report.requirements)
            .filter(|row| matches!(row.outcome, Outcome::Fail | Outcome::Warn))
            .flat_map(|row| row.findings.iter().map(move |finding| (row, finding)))
            .collect();
        assert_eq!(results.len(), findings.len(), "{}", path.display());
        let lines: Vec<&str> = text.lines().collect();
        for (result, (row, finding)) in results.iter().zip(&findings) {
            let context = format!("{}: {result}", path.display());
            assert_eq!(result["ruleId"], row.id.as_str(), "{context}");
            assert_eq!(
                result["message"]["text"],
                finding.detail.as_str(),
                "{context}"
            );
            let expected_level = if row.outcome == Outcome::Fail {
                "error"
            } else {
                "warning"
            };
            assert_eq!(result["level"], expected_level, "{context}");
            let rule = &rules[usize::try_from(result["ruleIndex"].as_u64().unwrap()).unwrap()];
            assert_eq!(rule["id"], result["ruleId"], "{context}");
            let source = row.source.as_ref().unwrap();
            assert_eq!(
                rule["shortDescription"]["text"],
                source.quote.as_str(),
                "{context}"
            );
            assert_eq!(rule["helpUri"], source.url.as_str(), "{context}");
            let location = &result["locations"][0]["physicalLocation"];
            assert_eq!(
                location["artifactLocation"]["uri"], "trace.jsonl",
                "{context}"
            );
            match finding.seq {
                Some(seq) => {
                    let line =
                        usize::try_from(location["region"]["startLine"].as_u64().unwrap()).unwrap();
                    let event: Value = serde_json::from_str(lines[line - 1]).unwrap();
                    assert_eq!(event["seq"], seq, "{context}");
                }
                None => assert!(location.get("region").is_none(), "{context}"),
            }
        }
    }
}

#[test]
fn a_trace_read_from_stdin_has_results_without_locations() {
    let path = violation_traces().remove(0);
    let (reports, text) = judge(&path);
    let events = reader::parse_trace(&text, &reader::Limits::default()).unwrap();
    let log: Value = serde_json::from_str(&sarif::render(
        &reports,
        sarif::Artifact {
            uri: None,
            events: &events,
        },
    ))
    .unwrap();
    let results = log["runs"][0]["results"].as_array().unwrap();
    assert!(!results.is_empty());
    assert!(
        results
            .iter()
            .all(|result| result["locations"] == serde_json::json!([]))
    );
}

/// Validates every violation trace's SARIF against the OASIS SARIF 2.1.0 schema at
/// the path in `SARIF_SCHEMA`. Ignored by default because the schema is fetched,
/// not committed; CI downloads it, checks its SHA-256, and runs this explicitly.
#[test]
#[ignore = "needs SARIF_SCHEMA=<path to sarif-schema-2.1.0.json>; run by CI"]
fn sarif_output_is_valid_under_the_oasis_schema() {
    let path = std::env::var("SARIF_SCHEMA").expect("SARIF_SCHEMA names the schema file");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let validator = jsonschema::draft4::new(&schema).unwrap();
    let mut checked = 0;
    for path in violation_traces() {
        let (log, _, _) = render(&path);
        let errors: Vec<String> = validator
            .iter_errors(&log)
            .map(|error| format!("{error} at {}", error.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{}: {errors:#?}", path.display());
        checked += 1;
    }
    // And a run with no results at all, which must still be a valid log.
    let empty = sarif::render(
        &[],
        sarif::Artifact {
            uri: None,
            events: &[],
        },
    );
    assert!(validator.is_valid(&serde_json::from_str(&empty).unwrap()));
    eprintln!("{checked} SARIF logs valid under the OASIS schema");
}
