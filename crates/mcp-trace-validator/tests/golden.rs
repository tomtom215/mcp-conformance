// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Golden-corpus tests: every trace in `corpus/` validates to a byte-identical,
//! committed report, and the corpus as a whole falsifies every implemented check.
//!
//! Regenerate goldens deliberately with `BLESS=1 cargo test -p mcp-trace-validator
//! --test golden` (or `cargo xtask bless`) and review the diff like any other code.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mcp_conformance_core::requirement::Registry;
use mcp_trace_validator::report::{Report, Verdict};
use mcp_trace_validator::{engine, reader};

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

fn trace_files(subdir: &str) -> Vec<PathBuf> {
    let dir = corpus_root().join(subdir);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no traces found in {}", dir.display());
    files
}

fn validate_file(registry: &Registry, trace_path: &Path) -> Report {
    let document = fs::read_to_string(trace_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", trace_path.display()));
    let events = reader::parse_trace(&document, &reader::Limits::default())
        .unwrap_or_else(|error| panic!("{} is malformed: {error}", trace_path.display()));
    engine::validate(registry, &events)
}

fn check_golden(trace_path: &Path, report: &Report) {
    let stem = trace_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let golden_path = corpus_root().join("golden").join(format!("{stem}.json"));
    let mut rendered = serde_json::to_string_pretty(report).unwrap();
    rendered.push('\n');

    if std::env::var_os("BLESS").is_some() {
        fs::write(&golden_path, &rendered)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", golden_path.display()));
        return;
    }

    let expected = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}\nhint: regenerate goldens with `cargo xtask bless`",
            golden_path.display()
        )
    });
    assert_eq!(
        rendered,
        expected,
        "report for {} diverges from its golden file {}\nhint: if the change is intended, run `cargo xtask bless` and review the diff",
        trace_path.display(),
        golden_path.display()
    );
}

#[test]
fn good_traces_pass_and_match_goldens() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    for trace_path in trace_files("good") {
        let report = validate_file(&registry, &trace_path);
        assert_eq!(
            report.verdict(),
            Verdict::Pass,
            "{} should pass cleanly:\n{}",
            trace_path.display(),
            report.render_human()
        );
        check_golden(&trace_path, &report);
    }
}

#[test]
fn violation_traces_fail_and_match_goldens() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    for trace_path in trace_files("violations") {
        let report = validate_file(&registry, &trace_path);
        assert_ne!(
            report.verdict(),
            Verdict::Pass,
            "{} is in violations/ but produced no findings",
            trace_path.display()
        );
        check_golden(&trace_path, &report);
    }
}

#[test]
fn corpus_falsifies_every_check() {
    // Every implemented check must be killed by at least one violation trace; a check
    // that has never failed anything is untested code wearing a green badge.
    let registry = Registry::builtin_2025_11_25().unwrap();
    let mut failed_checks = BTreeSet::new();
    for trace_path in trace_files("violations") {
        let report = validate_file(&registry, &trace_path);
        for row in &report.requirements {
            for finding in &row.findings {
                failed_checks.insert(finding.check.clone());
            }
        }
    }
    let implemented: BTreeSet<String> = mcp_trace_validator::checks::ALL
        .iter()
        .map(|check| check.id.to_owned())
        .collect();
    assert_eq!(
        failed_checks, implemented,
        "left: checks falsified by the corpus; right: checks implemented — \
         every implemented check needs a violation trace, and every finding must \
         come from a registered check"
    );
}
