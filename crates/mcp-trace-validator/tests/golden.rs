// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Golden-corpus tests: every trace in `corpus/` validates to a byte-identical,
//! committed report, and the corpus as a whole falsifies every implemented check.
//!
//! A trace's report is pinned across *two* files, because it states two kinds of
//! fact (ADR-0013). `corpus/golden/<stem>.json` holds everything the trace
//! decided — the judged rows, and the totals. The `excluded` rows are not among
//! them: the registry alone decides those, identically for every trace
//! (`engine::build_row`), so they are pinned once per revision in
//! `corpus/golden/exclusions/<revision>.json`. Splicing the two back together
//! reproduces the whole report, and
//! [`assert_reconstructs_the_full_report`] proves it does on every trace.
//!
//! Regenerate both deliberately with `BLESS=1 cargo test -p mcp-trace-validator
//! --all-features --test golden` (or `cargo xtask bless`) and review the diff
//! like any other code.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mcp_conformance_core::requirement::{Registry, Verification};
use mcp_trace_validator::report::{Outcome, Report, RequirementReport, Verdict};
use mcp_trace_validator::{engine, reader};
use serde::{Deserialize, Serialize};

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

/// Whether this run regenerates artifacts rather than checking them.
///
/// Same convention as the coverage manifest's regeneration switch: only the
/// exact value "1" blesses, so `BLESS=0 cargo test` does not silently
/// overwrite goldens.
fn blessing() -> bool {
    std::env::var("BLESS").is_ok_and(|value| value == "1")
}

fn write_artifact(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", parent.display()));
    }
    fs::write(path, contents)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
}

/// Renders a report the way its golden pins it: everything except the
/// `excluded` rows, which belong to the revision rather than to this trace.
///
/// `totals` is left whole — `totals.excluded` staying in every file is what
/// ties a trace back to its revision's ledger, and it is the per-trace
/// assertion that the excluded set is still the size the ledger says.
fn render_golden(report: &Report) -> String {
    let mut pinned = report.clone();
    pinned
        .requirements
        .retain(|row| row.outcome != Outcome::Excluded);
    let mut rendered = serde_json::to_string_pretty(&pinned).unwrap();
    rendered.push('\n');
    rendered
}

fn check_golden(registry: &Registry, trace_path: &Path, report: &Report) {
    let golden_path = golden_path(trace_path);
    let rendered = render_golden(report);

    if blessing() {
        write_artifact(&golden_path, &rendered);
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

    assert_reconstructs_the_full_report(registry, trace_path, report);
}

/// One revision's excluded set: the clauses its registry documents as
/// unjudgeable from a trace, with the reason it gives.
///
/// Pinned once per revision rather than once per trace because that is what it
/// is — `engine::build_row` maps `Verification::Excluded` straight to the
/// outcome and the registry's prose, consulting nothing about the session, and
/// `engine`'s own `happy_path_passes_every_checked_requirement` asserts as much
/// ("every documented exclusion reports as excluded, regardless of trace").
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ExclusionLedger {
    revision: String,
    requirements: Vec<ExcludedRow>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ExcludedRow {
    id: String,
    level: String,
    exclusion: String,
}

impl ExcludedRow {
    /// The row as the full report carries it.
    ///
    /// Built by deserializing rather than by struct literal: `RequirementReport`
    /// is `#[non_exhaustive]`, and round-tripping through the real type is also
    /// what keeps the reconstruction honest — a field added to the report type
    /// lands here in the report's own order, not in one this test invented.
    fn as_report_row(&self) -> RequirementReport {
        serde_json::from_value(serde_json::json!({
            "id": self.id,
            "level": self.level,
            "outcome": "excluded",
            "exclusion": self.exclusion,
        }))
        .expect("an excluded ledger row is a report row")
    }
}

fn exclusions_path(revision: &str) -> PathBuf {
    corpus_root()
        .join("golden/exclusions")
        .join(format!("{revision}.json"))
}

/// Whether the registry declines to judge this clause from a trace.
///
/// One predicate, used by both the ledger and the splice, so the two can never
/// disagree about which rows left the per-trace golden. `Verification` is
/// `#[non_exhaustive]`, and a variant added later is *not* an exclusion until
/// someone says so here — the safe default, since an unrecognised variant that
/// silently joined the ledger would drop a judged row out of every report.
const fn is_excluded(verification: &Verification) -> bool {
    matches!(verification, Verification::Excluded { .. })
}

/// The excluded set as the registry states it, in registry order.
fn exclusions_of(registry: &Registry) -> ExclusionLedger {
    ExclusionLedger {
        revision: registry.revision().to_string(),
        requirements: registry
            .requirements()
            .iter()
            .filter(|requirement| is_excluded(&requirement.verification))
            .map(|requirement| {
                let Verification::Excluded { exclusion } = &requirement.verification else {
                    unreachable!("is_excluded admitted a non-exclusion")
                };
                ExcludedRow {
                    id: requirement.id.to_string(),
                    level: requirement.level.keyword().to_owned(),
                    exclusion: exclusion.clone(),
                }
            })
            .collect(),
    }
}

fn read_exclusion_ledger(revision: &str) -> ExclusionLedger {
    let path = exclusions_path(revision);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}\nhint: regenerate goldens with `cargo xtask bless`",
            path.display()
        )
    });
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not an exclusion ledger: {error}", path.display()))
}

/// Byte-pins one revision's exclusion ledger against its registry.
///
/// This is the single place the exclusion prose is asserted, so an edit to a
/// reason — or a clause that stops being excluded because a check now judges
/// it — moves exactly one file instead of all 132.
fn check_exclusion_ledger(registry: &Registry) {
    let path = exclusions_path(&registry.revision().to_string());
    let mut rendered = serde_json::to_string_pretty(&exclusions_of(registry)).unwrap();
    rendered.push('\n');

    if blessing() {
        write_artifact(&path, &rendered);
        return;
    }

    let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "cannot read {}: {error}\nhint: regenerate goldens with `cargo xtask bless`",
            path.display()
        )
    });
    assert_eq!(
        rendered,
        expected,
        "revision {}'s excluded set diverges from its ledger {}\nhint: if the change is intended, run `cargo xtask bless` and review the diff",
        registry.revision(),
        path.display()
    );
}

/// The guarantee the two-file split has to keep: golden + ledger, spliced in
/// registry order, *is* the whole report — every row, plus the revision and
/// totals, exactly what the single-file format used to pin.
///
/// Without this the split would be a claim rather than a fact. It reads only
/// committed artifacts and the live report, so it fails if a judged row went
/// missing from the golden, if the ledger drifted from the registry, or if the
/// two interleave in any order other than the registry's.
fn assert_reconstructs_the_full_report(registry: &Registry, trace_path: &Path, report: &Report) {
    let golden: Report =
        serde_json::from_str(&fs::read_to_string(golden_path(trace_path)).unwrap()).unwrap();
    let ledger = read_exclusion_ledger(&registry.revision().to_string());

    let mut judged = golden.requirements.iter();
    let mut excluded = ledger.requirements.iter();
    let rebuilt: Vec<RequirementReport> = registry
        .requirements()
        .iter()
        .map(|requirement| {
            if is_excluded(&requirement.verification) {
                excluded
                    .next()
                    .unwrap_or_else(|| {
                        panic!("{}: ledger ran out of excluded rows", requirement.id)
                    })
                    .as_report_row()
            } else {
                judged
                    .next()
                    .unwrap_or_else(|| panic!("{}: golden ran out of judged rows", requirement.id))
                    .clone()
            }
        })
        .collect();
    assert!(
        judged.next().is_none() && excluded.next().is_none(),
        "{}: golden and ledger carry rows the registry does not name",
        trace_path.display()
    );

    for (rebuilt, live) in rebuilt.iter().zip(&report.requirements) {
        assert_eq!(
            rebuilt,
            live,
            "{}: splicing {} back together does not reproduce its row",
            trace_path.display(),
            live.id
        );
    }
    assert_eq!(
        rebuilt.len(),
        report.requirements.len(),
        "{}: the splice has {} rows; the report has {}",
        trace_path.display(),
        rebuilt.len(),
        report.requirements.len()
    );
    assert_eq!(
        (&golden.revision, golden.totals),
        (&report.revision, report.totals),
        "{}: the golden's revision and totals must be the report's",
        trace_path.display()
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
        check_golden(&registry, &trace_path, &report);
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
        check_golden(&registry, &trace_path, &report);
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
fn exclusion_ledger_matches_the_registry() {
    check_exclusion_ledger(&Registry::builtin_2025_11_25().unwrap());
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
    //
    // The revision each directory answers for is named alongside it: its
    // exclusion ledger is half of every report pinned there, so a missing one
    // strands the whole directory the same way a missing golden strands a trace.
    let mut ledgers = BTreeSet::new();
    for (subdirs, golden_dir, revision) in [
        (
            ["good", "violations"].as_slice(),
            corpus_root().join("golden"),
            "2025-11-25",
        ),
        (
            ["draft/good", "draft/violations", "draft/captured"].as_slice(),
            corpus_root().join("golden/draft"),
            "2026-07-28",
        ),
    ] {
        let ledger = exclusions_path(revision);
        assert!(
            ledger.is_file(),
            "{} pins reports for {revision}, whose exclusion ledger {} is missing",
            golden_dir.display(),
            ledger.display()
        );
        ledgers.insert(revision.to_owned());
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

    // And no ledger without a golden directory to serve: a revision whose
    // corpus was removed leaves prose that still reads as load-bearing.
    let committed: BTreeSet<String> = fs::read_dir(corpus_root().join("golden/exclusions"))
        .expect("corpus/golden/exclusions holds one ledger per pinned revision")
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        committed, ledgers,
        "left: exclusion ledgers on disk; right: revisions with a golden directory"
    );
}

#[test]
fn every_trace_has_a_provenance_ledger_row() {
    // corpus/README.md is the provenance ledger (it survives history rewrites,
    // unlike commit messages); a trace without a row is an undocumented fixture.
    let ledger = fs::read_to_string(corpus_root().join("README.md"))
        .expect("corpus/README.md exists and is the provenance ledger");
    for subdir in [
        "good",
        "violations",
        "draft/good",
        "draft/violations",
        "draft/captured",
    ] {
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

#[test]
fn checks_count_their_subjects() {
    // The other half of `corpus_falsifies_every_check`. That one proves a check
    // *can fail*; this one proves it can say **why a pass is a pass** — that on
    // some committed trace it found subjects to judge, rather than reporting
    // green because the session never came near its clause.
    //
    // Without this the sink's counting is advisory: a new check that never
    // calls `examined` reports every requirement it backs as *not observed*,
    // silently and forever. Run against every corpus, since a check may only be
    // reachable under one revision's traffic.
    let mut examined_somewhere = BTreeSet::new();
    for subdir in [
        "good",
        "violations",
        "draft/good",
        "draft/violations",
        "draft/captured",
    ] {
        for trace_path in trace_files(subdir) {
            let text = fs::read_to_string(&trace_path).unwrap();
            let events = reader::parse_trace(&text, &reader::Limits::default()).unwrap();
            let context = mcp_trace_validator::context::TraceContext::new(&events);
            for check in mcp_trace_validator::checks::ALL {
                if check.run(&context).subjects > 0 {
                    examined_somewhere.insert(check.id.to_owned());
                }
            }
        }
    }
    let implemented: BTreeSet<String> = mcp_trace_validator::checks::ALL
        .iter()
        .map(|check| check.id.to_owned())
        .collect();
    let never: Vec<&String> = implemented.difference(&examined_somewhere).collect();
    assert!(
        never.is_empty(),
        "these checks examined no subject on any corpus trace, so every clause they \
         back can only ever report `not observed`: {never:#?}"
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
    use super::{
        assert_falsifies_its_named_requirement, check_exclusion_ledger, check_golden, trace_files,
        validate_file,
    };
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
            check_golden(&registry, &trace_path, &report);
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
            check_golden(&registry, &trace_path, &report);
        }
    }

    #[test]
    fn captured_traces_match_goldens() {
        // The corpus's independent half: sessions this repository did not write,
        // recorded off the wire from an implementation that is not ours. Every
        // other draft fixture is hand-authored, which means it can only ever
        // confirm the author's reading of the specification — a check that is
        // wrong in the same way the author is wrong passes its unit tests and
        // its corpus alike. These traces are the cross-check that reading
        // cannot provide.
        //
        // No verdict is asserted, deliberately. A captured session is whatever
        // the implementations actually did, and the ones recorded here are a
        // `2025-11-25` server driven by the official suite's `2026-07-28`
        // scenarios, so real non-conformance is the expected content. What is
        // pinned is the *report*: which requirements fire, on which events, with
        // which detail. A check that starts misfiring on real traffic — the
        // failure mode authored fixtures are blindest to — moves this golden.
        let registry = draft_registry();
        for trace_path in trace_files("draft/captured") {
            let report = validate_file(&registry, &trace_path);
            check_golden(&registry, &trace_path, &report);
        }
    }

    #[test]
    fn draft_exclusion_ledger_matches_the_registry() {
        check_exclusion_ledger(&draft_registry());
    }
}
