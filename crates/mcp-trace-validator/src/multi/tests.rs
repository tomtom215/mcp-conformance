// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for multi-revision judgment.
//!
//! The fixture set is two clauses with opposite `applies` bounds — one removed
//! at `2026-07-28`, one introduced there — because that pair is the only
//! cross-revision shape the shipped registries can produce. The test that pins
//! *why* reads the real registries instead.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::reader::{Limits, parse_trace};

/// A two-revision set: BASE-001 throughout, LIFE-009 removed at 2026-07-28, DISC-001
/// introduced at 2026-07-28. All use a real check so outcomes are meaningful.
const SET: &str = r#"{
    "revisions": ["2025-11-25", "2026-07-28"],
    "requirements": [
        {"id": "BASE-001", "level": "MUST", "actor": "both",
         "source": {"section": "b#x", "quote": "MUST jsonrpc 2.0"},
         "checks": ["base.jsonrpc-version"]},
        {"id": "LIFE-009", "level": "MUST", "actor": "server",
         "applies": {"removed": "2026-07-28"},
         "source": {"section": "l#y", "quote": "MUST jsonrpc 2.0"},
         "checks": ["base.jsonrpc-version"]},
        {"id": "DISC-001", "level": "MUST", "actor": "server",
         "applies": {"introduced": "2026-07-28"},
         "source": {"section": "d#z", "quote": "MUST jsonrpc 2.0"},
         "checks": ["base.jsonrpc-version"]}
    ]
}"#;

fn set() -> RegistrySet {
    RegistrySet::from_json(SET).unwrap()
}

fn revs() -> [ProtocolRevision; 2] {
    ["2025-11-25".parse().unwrap(), "2026-07-28".parse().unwrap()]
}

#[test]
fn no_revisions_is_an_error() {
    assert_eq!(
        validate_revisions(&set(), &[], &[]),
        Err(MultiError::NoRevisions)
    );
}

#[test]
fn unknown_revision_names_itself() {
    let unknown: ProtocolRevision = "2024-01-01".parse().unwrap();
    assert_eq!(
        validate_revisions(&set(), &[unknown], &[]),
        Err(MultiError::UnknownRevision(unknown))
    );
    assert!(unknown.to_string().contains("2024-01-01"));
}

#[test]
fn rows_align_outcomes_with_revisions_and_mark_absence() {
    let report = validate_revisions(&set(), &revs(), &[]).unwrap();
    assert_eq!(report.revisions, ["2025-11-25", "2026-07-28"]);
    assert_eq!(report.summaries.len(), 2);

    let find = |id: &str| {
        report
            .requirements
            .iter()
            .find(|row| row.id == id)
            .cloned()
            .unwrap()
    };

    // Present throughout: an outcome in both columns, identical, not flagged.
    let base = find("BASE-001");
    assert!(base.outcomes[0].is_some() && base.outcomes[1].is_some());
    assert!(!base.differs());

    // Removed at the boundary: present, then absent.
    let life = find("LIFE-009");
    assert!(life.outcomes[0].is_some());
    assert_eq!(life.outcomes[1], None);
    assert!(life.differs());

    // Introduced at the boundary: absent, then present.
    let disc = find("DISC-001");
    assert_eq!(disc.outcomes[0], None);
    assert!(disc.outcomes[1].is_some());
    assert!(disc.differs());
}

#[test]
fn union_order_follows_the_set_and_drops_clauses_in_no_judged_revision() {
    // Judge only the older revision: DISC-001 (introduced later) appears in no judged
    // revision and must be dropped entirely, not shown as an all-absent row.
    let older: [ProtocolRevision; 1] = ["2025-11-25".parse().unwrap()];
    let report = validate_revisions(&set(), &older, &[]).unwrap();
    let ids: Vec<&str> = report.requirements.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["BASE-001", "LIFE-009"]);
    // A single judged revision can never "differ".
    assert!(report.requirements.iter().all(|row| !row.differs()));
}

#[test]
fn differs_detects_a_non_adjacent_divergence() {
    // Three identical-then-different columns: pins `any` against `all` and the row
    // comparison against equality.
    let uniform = MultiRow {
        id: "X-001".to_owned(),
        level: "MUST".to_owned(),
        outcomes: vec![
            Some(Outcome::Pass),
            Some(Outcome::Pass),
            Some(Outcome::Pass),
        ],
    };
    assert!(!uniform.differs());
    let diverges = MultiRow {
        outcomes: vec![
            Some(Outcome::Pass),
            Some(Outcome::Pass),
            Some(Outcome::Fail),
        ],
        ..uniform
    };
    assert!(diverges.differs());
}

#[test]
fn overall_verdict_is_the_worst_across_revisions() {
    let mut report = validate_revisions(&set(), &revs(), &[]).unwrap();
    // The synthetic trace is empty, so every real check passes vacuously.
    assert_eq!(report.verdict(), Verdict::Pass);
    // Worsen the second revision and confirm the fold tracks the priority order.
    report.summaries[1].verdict = Verdict::PassWithWarnings;
    assert_eq!(report.verdict(), Verdict::PassWithWarnings);
    report.summaries[1].verdict = Verdict::Fail;
    assert_eq!(report.verdict(), Verdict::Fail);
    report.summaries[0].verdict = Verdict::Unsupported;
    assert_eq!(report.verdict(), Verdict::Unsupported);
}

#[test]
fn human_render_shows_each_revision_cell_and_marks_divergence() {
    let report = validate_revisions(&set(), &revs(), &[]).unwrap();
    let text = report.render_human();
    assert!(text.contains("revisions 2025-11-25, 2026-07-28"), "{text}");
    // The removed clause reads present then absent, and is flagged.
    assert!(text.contains("LIFE-009"), "{text}");
    assert!(text.contains("2026-07-28=absent"), "{text}");
    assert!(text.contains("*differs"), "{text}");
    assert!(text.contains("overall verdict: pass"), "{text}");
}

/// The premise of [`MultiRow::differs`]' documentation, checked rather than
/// asserted: with per-revision extraction no clause is in force at two
/// revisions, so every row is `absent` on one side. If this ever fails, the
/// registries have started sharing entries and both that doc comment and
/// the `*differs` marker become meaningful again — which is a change worth
/// being told about.
#[cfg(feature = "draft-2026-07-28")]
#[test]
fn the_shipped_registries_share_no_clause() {
    use mcp_conformance_core::requirement::RegistrySet;

    let set = RegistrySet::builtin().unwrap();
    let revisions: Vec<ProtocolRevision> = set
        .revisions()
        .iter()
        .map(|revision| revision.to_string().parse().unwrap())
        .collect();
    assert_eq!(revisions.len(), 2, "this test is about the pair");
    let report = validate_revisions(&set, &revisions, &[]).unwrap();
    let shared: Vec<&str> = report
        .requirements
        .iter()
        .filter(|row| row.outcomes.iter().all(Option::is_some))
        .map(|row| row.id.as_str())
        .collect();
    assert!(shared.is_empty(), "in force at both revisions: {shared:?}");
    assert!(
        report.requirements.iter().all(MultiRow::differs),
        "every row differs, which is what makes the marker uninformative"
    );
}

#[test]
fn each_revision_line_accounts_for_every_clause_it_judged() {
    let report = validate_revisions(&set(), &revs(), &[]).unwrap();
    let text = report.render_human();
    for (index, summary) in report.summaries.iter().enumerate() {
        // The line is built from `Totals`' own phrase, so it names every
        // outcome by construction; asserting the phrase is present is what
        // holds it to that source rather than to a hand-written list, which
        // is how this line came to report six of the seven outcomes.
        let line = format!(
            "  {}: {} — verdict {}\n",
            summary.revision, summary.totals, summary.verdict
        );
        assert!(text.contains(&line), "{text}");
        // And the counts account for exactly the clauses that exist at this
        // revision — the rows the table above it shows as not `absent`.
        let judged = report
            .requirements
            .iter()
            .filter(|row| row.outcomes.get(index).is_some_and(Option::is_some))
            .count();
        let counted: u32 = summary
            .totals
            .labelled()
            .iter()
            .map(|&(_, count)| count)
            .sum();
        assert_eq!(counted as usize, judged, "{}: {line}", summary.revision);
    }
}

#[test]
fn judges_a_real_trace_and_is_deterministic() {
    // A real handshake, judged against both revisions, serializes identically twice.
    let trace = r#"{"seq":0,"direction":"client-to-server","transport":"stdio","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"t","version":"0"}}}}"#;
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let a = validate_revisions(&set(), &revs(), &events).unwrap();
    let b = validate_revisions(&set(), &revs(), &events).unwrap();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

// Needs a second shipped registry, for the reason `declared.rs` states.
#[test]
#[cfg(feature = "draft-2026-07-28")]
fn a_run_that_judged_none_of_the_sessions_revisions_says_so() {
    // Naming `--revision` explicitly does not make judging a `2026-07-28`
    // recording against `2025-11-25` any less of a mistake, so the note is
    // rendered here too — and it is worded for a run that chose its revisions.
    let stateless = r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}}
{"seq":1,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}}"#;
    let events = parse_trace(stateless, &Limits::default()).unwrap();
    let set = RegistrySet::builtin().unwrap();

    let wrong: Vec<ProtocolRevision> = vec!["2025-11-25".parse().unwrap()];
    let report = validate_revisions(&set, &wrong, &events).unwrap();
    assert_eq!(
        report.revision_mismatch.as_deref(),
        Some(["2026-07-28".to_owned()].as_slice())
    );
    let rendered = report.render_human();
    assert!(
        rendered.contains("declares protocol revision 2026-07-28"),
        "{rendered}"
    );
    assert!(rendered.contains("which was not judged"), "{rendered}");

    // Judging its own revision alongside the other one is a fair question.
    let both: Vec<ProtocolRevision> =
        vec!["2025-11-25".parse().unwrap(), "2026-07-28".parse().unwrap()];
    let report = validate_revisions(&set, &both, &events).unwrap();
    assert!(report.revision_mismatch.is_none());
    assert!(!report.render_human().contains("NOTE"));
}
