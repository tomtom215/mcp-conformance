// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The published report JSON Schema describes what `validate --format json`
//! emits: every golden report, and both report shapes as the CLI prints them,
//! are valid under it, and the schema is closed, so a member the code emits and
//! the schema does not document fails here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use mcp_trace_validator::report::{JSON_SCHEMA, Report};
use serde_json::{Value, json};

fn validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(JSON_SCHEMA).unwrap();
    assert!(jsonschema::meta::is_valid(&schema));
    jsonschema::draft202012::new(&schema).unwrap()
}

fn errors(validator: &jsonschema::Validator, value: &Value) -> Vec<String> {
    validator
        .iter_errors(value)
        .map(|error| format!("{error} at {}", error.instance_path()))
        .collect()
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

#[test]
fn every_golden_report_is_valid() {
    let validator = validator();
    let mut checked = 0;
    for dir in ["golden", "golden/draft"] {
        for entry in std::fs::read_dir(corpus().join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let report: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let found = errors(&validator, &report);
            assert!(found.is_empty(), "{}: {found:#?}", path.display());
            // The verdict is the one the totals imply.
            let parsed: Report = serde_json::from_value(report.clone()).unwrap();
            assert_eq!(
                report["verdict"],
                json!(parsed.effective_verdict()),
                "{}",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(checked > 100, "checked {checked} goldens");
}

/// `validate --format json` for `trace`, with `extra` arguments.
fn cli_json(trace: &str, extra: &[&str]) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(["validate", "--format", "json", trace])
        .args(extra)
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn both_shapes_the_cli_prints_are_valid() {
    let validator = validator();
    let trace = corpus().join("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let trace = trace.to_str().unwrap();
    let single = cli_json(trace, &[]);
    assert_eq!(single["revision_source"], "declared");
    let multi = cli_json(
        trace,
        &["--revision", "2025-11-25", "--revision", "2026-07-28"],
    );
    assert!(multi["summaries"].is_array());
    // Judged against a revision the trace does not declare: carries the mismatch.
    let mismatched = cli_json(trace, &["--revision", "2025-11-25"]);
    assert!(mismatched["revision_mismatch"].is_array(), "{mismatched}");
    // Several traces: the envelope holding each trace's report.
    let other = corpus().join("good/stdio-minimal-init.jsonl");
    let several = cli_json(trace, &[other.to_str().unwrap()]);
    assert_eq!(
        several["traces"].as_array().map(Vec::len),
        Some(2),
        "{several}"
    );
    assert_eq!(several["verdict"], "fail", "the worst of fail and pass");
    assert_eq!(several["unjudged"], 0);
    // Viewed for one party: the view member, and a verdict the rows explain.
    let viewed = cli_json(trace, &["--party", "server"]);
    assert_eq!(viewed["party_view"], "server", "{viewed}");
    assert_eq!(viewed["verdict"], "pass", "MRTR-019 is the client's");
    let viewed_multi = cli_json(
        trace,
        &[
            "--party",
            "client",
            "--revision",
            "2025-11-25",
            "--revision",
            "2026-07-28",
        ],
    );
    assert_eq!(viewed_multi["party_view"], "client", "{viewed_multi}");
    for report in [
        &single,
        &multi,
        &mismatched,
        &several,
        &viewed,
        &viewed_multi,
    ] {
        let found = errors(&validator, report);
        assert!(found.is_empty(), "{found:#?}");
    }
}

/// Changes a report, given the index of one of its `fail` rows.
type Breaking = fn(&mut Value, usize);

/// The schema is closed and states each outcome's companions: breaking any of
/// them in a real report is caught.
#[test]
fn a_report_that_breaks_the_schema_is_caught() {
    let validator = validator();
    let trace = corpus().join("draft/violations/mrtr-019-retry-reuses-id.jsonl");
    let report = cli_json(trace.to_str().unwrap(), &[]);
    let failing = report["requirements"]
        .as_array()
        .unwrap()
        .iter()
        .position(|row| row["outcome"] == "fail")
        .unwrap();
    let broken: [(&str, Breaking); 6] = [
        ("an undocumented member", |report, _| {
            report["score"] = json!(1);
        }),
        ("a fail row without its clause", |report, row| {
            report["requirements"][row]
                .as_object_mut()
                .unwrap()
                .remove("source");
        }),
        ("a pass row with findings", |report, row| {
            report["requirements"][row]["outcome"] = json!("pass");
        }),
        ("an unknown outcome", |report, row| {
            report["requirements"][row]["outcome"] = json!("maybe");
        }),
        ("a total that is not a count", |report, _| {
            report["totals"]["pass"] = json!(-1);
        }),
        ("a report without its verdict", |report, _| {
            report.as_object_mut().unwrap().remove("verdict");
        }),
    ];
    for (what, breaking) in broken {
        let mut copy = report.clone();
        breaking(&mut copy, failing);
        assert!(!validator.is_valid(&copy), "{what} was accepted");
    }
}

/// The multi-trace envelope ties its verdict and its list of unjudged traces to
/// the count: a run that left a trace out cannot claim a pass, nor list traces
/// it says it judged.
#[test]
fn an_envelope_that_hides_an_unjudged_trace_is_caught() {
    let validator = validator();
    let envelope = |verdict: &str, unjudged: u32, listed: bool| {
        let mut document = json!({ "verdict": verdict, "traces": [], "unjudged": unjudged });
        if listed {
            document["unjudged_traces"] = json!([{ "trace": "b.jsonl", "error": "malformed" }]);
        }
        document
    };
    assert!(validator.is_valid(&envelope("incomplete", 1, true)));
    assert!(validator.is_valid(&envelope("pass", 0, false)));
    for (what, document) in [
        ("a pass that left a trace out", envelope("pass", 1, true)),
        (
            "an unjudged count with no list",
            envelope("incomplete", 1, false),
        ),
        ("a list with no unjudged count", envelope("pass", 0, true)),
        (
            "incomplete with nothing left out",
            envelope("incomplete", 0, false),
        ),
        ("the old null verdict", {
            let mut document = envelope("pass", 0, false);
            document["verdict"] = Value::Null;
            document
        }),
    ] {
        assert!(!validator.is_valid(&document), "{what} was accepted");
    }
}
