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
use mcp_trace_validator::report::{Outcome, Report, Verdict};
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

/// Whether a trace belongs to the `2026-07-28` corpus (`corpus/draft/…`).
fn is_draft(trace_path: &Path) -> bool {
    trace_path
        .components()
        .any(|component| component.as_os_str() == "draft")
}

/// The committed report for a trace: `corpus/golden/<stem>.json`, and
/// `corpus/golden/draft/<stem>.json` for the `2026-07-28` corpus.
///
/// The revisions get separate directories rather than one keyed by stem. Both
/// corpora name traces after requirement ids, but the ids are drawn from
/// *different* registries — `base-045-…` is one clause at `2025-11-25` and
/// another at `2026-07-28` — so a shared directory would let one revision's
/// golden silently answer for the other revision's trace.
fn golden_path(trace_path: &Path) -> PathBuf {
    let stem = trace_path.file_stem().unwrap().to_string_lossy();
    let dir = corpus_root().join("golden");
    let dir = if is_draft(trace_path) {
        dir.join("draft")
    } else {
        dir
    };
    dir.join(format!("{stem}.json"))
}

fn check_golden(trace_path: &Path, report: &Report) {
    let golden_path = golden_path(trace_path);
    let mut rendered = serde_json::to_string_pretty(report).unwrap();
    rendered.push('\n');

    // Same convention as the coverage manifest's regeneration switch: only
    // the exact value "1" blesses, so `BLESS=0 cargo test` does not silently
    // overwrite goldens.
    if std::env::var("BLESS").is_ok_and(|value| value == "1") {
        if let Some(parent) = golden_path.parent() {
            fs::create_dir_all(parent)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", parent.display()));
        }
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
        assert_falsifies_its_named_requirement(&trace_path, &report);
        check_golden(&trace_path, &report);
    }
}

/// Attribution, not just failure: the trace `area-nnn-…` exists to falsify
/// requirement AREA-NNN and must keep doing so by name.
///
/// A refactor that re-routes the defect to some other requirement — while
/// another trace happens to keep the orphaned check covered — would otherwise
/// re-bless cleanly and be visible only to a human reading the golden diff.
/// That is not hypothetical: splitting a check that bundled its neighbours'
/// rules (`transport.header-value-encoding`, 2026-08-08) moved findings
/// between requirements exactly this way, and only hand-reading caught it.
fn assert_falsifies_its_named_requirement(trace_path: &Path, report: &Report) {
    let stem = trace_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let id = stem
        .split('-')
        .take(2)
        .collect::<Vec<_>>()
        .join("-")
        .to_uppercase();
    let row = report
        .requirements
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("{stem}: no report row for {id}"));
    assert!(
        matches!(row.outcome, Outcome::Fail | Outcome::Warn) && !row.findings.is_empty(),
        "{stem} must falsify {id} by name; got outcome {:?} with {} finding(s)",
        row.outcome,
        row.findings.len()
    );
}

#[test]
fn every_golden_belongs_to_a_living_trace() {
    // Goldens are written per trace; deleting or renaming a trace must not
    // strand its golden as unreviewed dead weight that still looks load-bearing.
    //
    // Not under `BLESS=1`: blessing is exactly when the two sets are being
    // reconciled, and tests within a binary run concurrently, so a golden for a
    // newly added trace may not be on disk when this reads the directory. The
    // ordinary (unblessed) run — the one CI makes — is where the invariant binds.
    if std::env::var("BLESS").is_ok_and(|value| value == "1") {
        return;
    }
    //
    // Both revisions, each against its own golden directory: the `2026-07-28`
    // corpus is byte-pinned on the same terms as the shipped one, so a stranded
    // draft golden is as much a defect as a stranded shipped one.
    for (subdirs, golden_dir) in [
        (["good", "violations"], corpus_root().join("golden")),
        (
            ["draft/good", "draft/violations"],
            corpus_root().join("golden/draft"),
        ),
    ] {
        let traces: BTreeSet<String> = subdirs
            .iter()
            .flat_map(|subdir| trace_files(subdir))
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        let goldens: BTreeSet<String> = fs::read_dir(&golden_dir)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", golden_dir.display()))
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            goldens,
            traces,
            "left: golden files in {}; right: traces — every golden needs its trace \
             and every trace its golden",
            golden_dir.display()
        );
    }
}

#[test]
fn every_trace_has_a_provenance_ledger_row() {
    // corpus/README.md is the provenance ledger (it survives history rewrites,
    // unlike commit messages); a trace without a row is an undocumented fixture.
    let ledger = fs::read_to_string(corpus_root().join("README.md"))
        .expect("corpus/README.md exists and is the provenance ledger");
    for subdir in ["good", "violations", "draft/good", "draft/violations"] {
        for trace_path in trace_files(subdir) {
            let name = trace_path
                .file_name()
                .expect("trace files have names")
                .to_string_lossy()
                .into_owned();
            assert!(
                ledger.contains(&format!("`{name}`")),
                "{subdir}/{name} has no row in corpus/README.md's provenance ledger"
            );
        }
    }
}

#[test]
fn corpus_falsifies_every_check() {
    // Every implemented check must be killed by at least one violation trace; a check
    // that has never failed anything is untested code wearing a green badge.
    //
    // Unioned across corpora since the registry describes more than one revision: a
    // `2026-07-28` check is exercised by `corpus/draft/violations` under that
    // revision's registry, and the invariant is that *some* corpus kills each check —
    // not that any single one does.
    let mut failed_checks = BTreeSet::new();
    let mut collect = |registry: &Registry, subdir: &str| {
        for trace_path in trace_files(subdir) {
            let report = validate_file(registry, &trace_path);
            for row in &report.requirements {
                for finding in &row.findings {
                    failed_checks.insert(finding.check.clone());
                }
            }
        }
    };
    collect(&Registry::builtin_2025_11_25().unwrap(), "violations");
    #[cfg(feature = "draft-2026-07-28")]
    {
        let draft = mcp_conformance_core::requirement::RegistrySet::builtin()
            .unwrap()
            .registry("2026-07-28".parse().unwrap())
            .expect("the draft feature describes 2026-07-28");
        collect(&draft, "draft/violations");
    }
    let implemented: BTreeSet<String> = mcp_trace_validator::checks::ALL
        .iter()
        .map(|check| check.id.to_owned())
        .collect();
    assert_eq!(
        failed_checks, implemented,
        "left: checks falsified by the corpora; right: checks implemented — \
         every implemented check needs a violation trace, and every finding must \
         come from a registered check"
    );
}

/// The `2026-07-28` corpus, held to the same contract as the `2025-11-25` one:
/// a conforming session passes everything, and every check that revision's
/// registry names has a trace that kills it.
///
/// Gated on the feature because without it the registry set does not describe
/// the revision — there would be nothing to project and nothing to judge.
#[cfg(feature = "draft-2026-07-28")]
mod draft {
    use super::{assert_falsifies_its_named_requirement, check_golden, trace_files, validate_file};
    use mcp_conformance_core::requirement::Registry;
    use mcp_conformance_core::requirement::RegistrySet;
    use mcp_trace_validator::report::Verdict;

    fn draft_registry() -> Registry {
        RegistrySet::builtin()
            .unwrap()
            .registry("2026-07-28".parse().unwrap())
            .expect("the draft feature describes 2026-07-28")
    }

    #[test]
    fn draft_good_traces_pass_every_named_check() {
        let registry = draft_registry();
        for trace_path in trace_files("draft/good") {
            let report = validate_file(&registry, &trace_path);
            let failures: Vec<_> = report
                .requirements
                .iter()
                .filter(|row| !row.findings.is_empty())
                .map(|row| (row.id.clone(), row.findings.clone()))
                .collect();
            assert!(
                failures.is_empty(),
                "{} should conform: {failures:#?}",
                trace_path.display()
            );
            check_golden(&trace_path, &report);
        }
    }

    #[test]
    fn draft_violation_traces_fail_and_match_goldens() {
        // The `2025-11-25` contract, applied to this revision: a violation trace
        // must fail *the requirement it is named after*, and its whole report is
        // byte-pinned. Until this existed the draft corpus was held only to
        // `corpus_falsifies_every_check` — "some trace kills each check" — which
        // cannot see a finding that has drifted onto a neighbouring requirement.
        let registry = draft_registry();
        for trace_path in trace_files("draft/violations") {
            let report = validate_file(&registry, &trace_path);
            assert_ne!(
                report.verdict(),
                Verdict::Pass,
                "{} is in draft/violations/ but produced no findings",
                trace_path.display()
            );
            assert_falsifies_its_named_requirement(&trace_path, &report);
            check_golden(&trace_path, &report);
        }
    }
}
