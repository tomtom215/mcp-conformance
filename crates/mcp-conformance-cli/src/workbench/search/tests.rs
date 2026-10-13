// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The index and its ranking, over the fixture library and over documents
//! built by hand where a score needs pinning.

#![allow(clippy::unwrap_used)]

use super::*;
use crate::workbench::fixture;
use crate::workbench::query::parse;
use crate::workbench::state::Workbench;

fn find(workbench: &Workbench, text: &str) -> Vec<Hit> {
    let query = parse(text).unwrap();
    search(
        &query,
        workbench
            .entries
            .iter()
            .map(|(id, e)| (id.as_str(), &e.doc)),
    )
}

fn ids(hits: &[Hit]) -> Vec<&str> {
    let mut ids: Vec<&str> = hits.iter().map(|hit| hit.id.as_str()).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn each_field_finds_what_it_names() {
    let (workbench, _) = fixture::workbench("fields");
    for (query, expected) in [
        ("req:BASE-001", vec!["base-001-request-id-boolean"]),
        ("req:base-001", vec!["base-001-request-id-boolean"]),
        (
            "code:-32602",
            vec![
                "hostile-markup",
                "sdk-go-2025-11-25",
                "sdk-rust-2025-11-25",
                "sdk-rust-2026-07-28",
            ],
        ),
        ("verdict:fail", vec!["base-001-request-id-boolean"]),
        (
            "verdict:warn",
            vec![
                "hostile-markup",
                // rmcp sets `serverInfo` on two result kinds only (BASE-037).
                "sdk-rust-2026-07-28",
                "sdk-typescript-2025-11-25",
                "tool-005-name-length",
            ],
        ),
        ("tag:needs-redaction", vec!["leaky-session"]),
        ("tool:lookup", vec!["leaky-session"]),
        ("session:fixture-session-7", vec!["leaky-session"]),
        ("session:everything-http", vec!["everything-http"]),
        ("server:leaky-server*", vec!["leaky-session"]),
        (
            "method:tools/call -tag:sdk -tag:violation -tag:security -tag:reference",
            vec!["leaky-session"],
        ),
        (
            "resource:test://static-text tag:draft",
            vec!["sdk-rust-2026-07-28"],
        ),
        (
            "prompt:test_simple_prompt server:rust*",
            vec!["sdk-rust-2025-11-25"],
        ),
        ("nothing-matches-this", vec![]),
    ] {
        assert_eq!(ids(&find(&workbench, query)), expected, "{query}");
    }
    assert_eq!(find(&workbench, "").len(), 11);
}

#[test]
fn a_hit_carries_the_events_its_terms_matched() {
    let (workbench, _) = fixture::workbench("hits");
    let hits = find(&workbench, "req:BASE-001");
    assert_eq!(hits[0].events, [3]);
    let hits = find(&workbench, "method:ping");
    let base = hits
        .iter()
        .find(|hit| hit.id == "base-001-request-id-boolean")
        .unwrap();
    assert_eq!(base.events, [3]);
    // Free text in a finding points at the finding's event.
    let hits = find(&workbench, "boolean");
    let base = hits
        .iter()
        .find(|hit| hit.id == "base-001-request-id-boolean")
        .unwrap();
    assert!(base.events.contains(&3), "{base:?}");
}

fn doc(id: &str, methods: &[(&str, u64)], findings: &[&str], tags: &[&str]) -> Doc {
    let mut doc = Doc::default();
    for (method, seq) in methods {
        doc.add(Field::Method, method, Some(*seq));
    }
    for (index, finding) in findings.iter().enumerate() {
        doc.findings
            .push(((*finding).to_lowercase(), Some(index as u64)));
    }
    let mut name = vec![id];
    name.extend_from_slice(tags);
    doc.name = name.join(" ").to_lowercase();
    doc
}

#[test]
fn ranking_counts_occurrences_and_breaks_ties_by_id() {
    let many = doc("b", &[("ping", 1), ("ping", 2), ("ping", 3)], &[], &[]);
    let one = doc("a", &[("ping", 1)], &[], &[]);
    let also_one = doc("c", &[("ping", 4)], &[], &[]);
    let hits = search(
        &parse("method:ping").unwrap(),
        [("b", &many), ("a", &one), ("c", &also_one)],
    );
    assert_eq!(
        hits.iter()
            .map(|h| (h.id.as_str(), h.score))
            .collect::<Vec<_>>(),
        [("b", 12), ("a", 10), ("c", 10)]
    );
    assert_eq!(hits[0].events, [1, 2, 3]);
    // Occurrences add at most five.
    let lots: Vec<(&str, u64)> = (0..20).map(|seq| ("ping", seq)).collect();
    let flood = doc("f", &lots, &[], &[]);
    assert_eq!(
        search(&parse("method:ping").unwrap(), [("f", &flood)])[0].score,
        15
    );
}

#[test]
fn free_text_scores_by_where_it_is_found() {
    let named = doc("timeout-run", &[], &[], &[]);
    let found = doc("x", &[], &["request timed out: timeout"], &[]);
    let elsewhere = doc("y", &[("notifications/timeout", 0)], &[], &[]);
    let everywhere = doc("timeout", &[("timeout", 0)], &["timeout"], &[]);
    let hits = search(
        &parse("timeout").unwrap(),
        [
            ("timeout-run", &named),
            ("x", &found),
            ("y", &elsewhere),
            ("timeout", &everywhere),
        ],
    );
    let scores: Vec<(&str, u32)> = hits.iter().map(|h| (h.id.as_str(), h.score)).collect();
    assert_eq!(
        scores,
        [("timeout", 6), ("timeout-run", 3), ("x", 2), ("y", 1)]
    );
}

#[test]
fn every_term_must_match_and_a_negated_one_must_not() {
    let a = doc("a", &[("ping", 0), ("tools/call", 1)], &[], &[]);
    let b = doc("b", &[("ping", 0)], &[], &[]);
    let docs = || [("a", &a), ("b", &b)];
    assert_eq!(
        ids(&search(
            &parse("method:ping method:tools/call").unwrap(),
            docs()
        )),
        ["a"]
    );
    assert_eq!(
        ids(&search(
            &parse("method:ping -method:tools/call").unwrap(),
            docs()
        )),
        ["b"]
    );
    assert_eq!(
        ids(&search(&parse("-method:nothing").unwrap(), docs())),
        ["a", "b"]
    );
    // A field the document has no value for matches nothing.
    assert_eq!(search(&parse("code:1").unwrap(), docs()), []);
}
