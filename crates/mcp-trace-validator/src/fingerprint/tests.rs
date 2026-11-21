// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for a finding's identity.

#![allow(clippy::unwrap_used)]

use mcp_conformance_core::requirement::Registry;
use serde_json::Value;

use super::*;
use crate::reader::{Limits, parse_trace};
use crate::sarif;

/// A message with the wrong `jsonrpc` version (`BASE-008`), at `seq`, with `id`.
fn wrong_version(seq: u64, id: u64) -> String {
    format!(
        r#"{{"seq":{seq},"direction":"client-to-server","transport":"stdio","kind":"message","payload":{{"jsonrpc":"1.0","id":{id},"method":"ping"}}}}"#
    )
}

fn report_of(trace: &str) -> (Vec<Report>, Vec<mcp_conformance_core::trace::TraceEvent>) {
    let events = parse_trace(trace, &Limits::default()).unwrap();
    let registry = Registry::builtin_2025_11_25().unwrap();
    (vec![crate::engine::validate(&registry, &events)], events)
}

#[test]
fn numbers_fold_to_one_mark_and_text_stays() {
    assert_eq!(fold_numbers("seq 12: id 3a, 007"), "seq #: id #a, #");
    assert_eq!(fold_numbers("no digits"), "no digits");
    assert_eq!(fold_numbers(""), "");
}

#[test]
fn a_fingerprint_is_rule_check_folded_detail_and_the_count_of_its_stem() {
    let trace = format!("{}\n{}", wrong_version(0, 1), wrong_version(1, 2));
    let (reports, _) = report_of(&trace);
    let prints = Fingerprints::of(&[(Some("t.jsonl"), &reports)]);
    let base_008: Vec<&str> = prints
        .found()
        .iter()
        .map(|one| one.fingerprint.as_str())
        .filter(|print| print.starts_with("BASE-008:"))
        .collect();
    assert_eq!(base_008.len(), 2, "{:#?}", prints.found());
    let (first, second) = (base_008[0], base_008[1]);
    // Same rule, same check, same folded detail: only the count differs.
    assert_eq!(
        first.rsplit_once(':').unwrap().0,
        second.rsplit_once(':').unwrap().0
    );
    assert_eq!(first.rsplit_once(':').unwrap().1, "1");
    assert_eq!(second.rsplit_once(':').unwrap().1, "2");
    assert!(
        first.starts_with("BASE-008:base.jsonrpc-version:"),
        "{first}"
    );
    let stem = first.rsplit_once(':').unwrap().0;
    let detail = stem.split_once("base.jsonrpc-version:").unwrap().1;
    assert_eq!(
        detail,
        r##"jsonrpc member is "#.#", expected the string "#.#""##
    );
}

#[test]
fn a_re_recorded_trace_under_another_name_keeps_every_fingerprint() {
    let (before, _) = report_of(&format!("{}\n{}", wrong_version(0, 1), wrong_version(1, 2)));
    let (after, _) = report_of(&format!(
        "{}\n{}",
        wrong_version(100, 7),
        wrong_version(101, 8)
    ));
    let prints = |name, reports: &Vec<Report>| {
        Fingerprints::of(&[(Some(name), reports.as_slice())])
            .found()
            .iter()
            .map(|one| one.fingerprint.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        prints("a.jsonl", &before),
        prints("elsewhere/b.jsonl", &after)
    );
}

#[test]
fn the_count_restarts_in_each_trace_and_continues_for_one_path_named_twice() {
    let (reports, _) = report_of(&wrong_version(0, 1));
    let separate = Fingerprints::of(&[(Some("a.jsonl"), &reports), (Some("b.jsonl"), &reports)]);
    let ends: Vec<&str> = separate
        .found()
        .iter()
        .filter(|one| one.fingerprint.starts_with("BASE-008:"))
        .map(|one| one.fingerprint.rsplit_once(':').unwrap().1)
        .collect();
    assert_eq!(ends, ["1", "1"], "each trace counts for itself");
    let same = Fingerprints::of(&[(Some("a.jsonl"), &reports), (Some("a.jsonl"), &reports)]);
    let ends: Vec<&str> = same
        .found()
        .iter()
        .filter(|one| one.fingerprint.starts_with("BASE-008:"))
        .map(|one| one.fingerprint.rsplit_once(':').unwrap().1)
        .collect();
    assert_eq!(
        ends,
        ["1", "2"],
        "the path is the trace's name, as in SARIF"
    );
}

#[test]
fn sarif_carries_exactly_the_fingerprints_computed_here_in_order() {
    // The two views of identity cannot drift because there is one function; this
    // pins that SARIF takes its results' fingerprints from it, slot for slot.
    let trace = format!("{}\n{}", wrong_version(0, 1), wrong_version(1, 2));
    let (reports, events) = report_of(&trace);
    let log: Value = serde_json::from_str(&sarif::render(
        &reports,
        sarif::Artifact {
            uri: Some("t.jsonl"),
            events: &events,
        },
    ))
    .unwrap();
    let in_sarif: Vec<String> = log["runs"][0]["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| {
            result["partialFingerprints"][KEY]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    let computed: Vec<String> = Fingerprints::of(&[(Some("t.jsonl"), &reports)])
        .found()
        .iter()
        .map(|one| one.fingerprint.clone())
        .collect();
    assert!(
        in_sarif.len() >= 3,
        "wire-schema and clause findings: {in_sarif:#?}"
    );
    assert_eq!(in_sarif, computed);
}

#[test]
fn an_id_with_findings_under_two_revisions_is_suffixed_with_each() {
    let (mut reports, _) = report_of(&wrong_version(0, 1));
    let mut later = reports[0].clone();
    "2026-07-28".clone_into(&mut later.revision);
    reports.push(later);
    let prints = Fingerprints::of(&[(Some("t.jsonl"), &reports)]);
    assert_eq!(
        prints.rule_id("BASE-008", "2025-11-25"),
        "BASE-008@2025-11-25"
    );
    assert_eq!(
        prints.rule_id("BASE-008", "2026-07-28"),
        "BASE-008@2026-07-28"
    );
    assert!(
        prints
            .found()
            .iter()
            .any(|one| one.fingerprint.starts_with("wire-schema@2026-07-28:")),
        "{:#?}",
        prints.found()
    );
    // And a lone revision's is the plain ID.
    let (alone, _) = report_of(&wrong_version(0, 1));
    assert_eq!(
        Fingerprints::of(&[(None, &alone)]).rule_id("BASE-008", "2025-11-25"),
        "BASE-008"
    );
}

#[test]
fn an_extensions_findings_are_not_scored_and_the_rest_are() {
    let (reports, _) = crate::fixtures::undeclared_task();
    let prints = Fingerprints::of(&[(Some("t.jsonl"), &reports)]);
    let extension: Vec<&Found> = prints.found().iter().filter(|one| !one.scored).collect();
    assert!(!extension.is_empty(), "{:#?}", prints.found());
    assert!(
        extension
            .iter()
            .all(|one| matches!(one.slot.place, Place::Extension { .. })
                && one.fingerprint.starts_with("TASK-")),
        "{extension:#?}"
    );
    assert!(
        prints
            .found()
            .iter()
            .filter(|one| one.scored)
            .all(|one| !matches!(one.slot.place, Place::Extension { .. }))
    );
}

#[test]
fn a_slot_addresses_its_finding_and_a_missing_one_addresses_nothing() {
    let (mut reports, _) = report_of(&wrong_version(0, 1));
    let (first, nowhere, got, missing) = {
        let prints = Fingerprints::of(&[(None, &reports)]);
        let first = prints
            .found()
            .iter()
            .find(|one| one.fingerprint.starts_with("BASE-008:"))
            .unwrap()
            .clone();
        let nowhere = Slot {
            finding: 99,
            ..first.slot
        };
        let got = prints.get(first.slot).map(str::to_owned);
        (first, nowhere, got, prints.get(nowhere).map(str::to_owned))
    };
    assert_eq!(got.as_deref(), Some(first.fingerprint.as_str()));
    assert_eq!(missing, None);
    assert_eq!(nowhere.place, first.slot.place);
    let detail = first
        .slot
        .place
        .findings(&reports[first.slot.report])
        .unwrap()[first.slot.finding]
        .detail
        .clone();
    assert!(detail.contains("jsonrpc"), "{detail}");
    // The same place, mutably, is the same list.
    let findings = first.slot.place.findings_mut(&mut reports[0]).unwrap();
    findings.clear();
    assert_eq!(first.slot.place.findings(&reports[0]), Some(&[][..]));
    // Past the end of everything.
    assert!(Place::Row(9999).findings(&reports[0]).is_none());
    assert!(Place::Row(9999).findings_mut(&mut reports[0]).is_none());
    assert!(
        Place::Extension {
            extension: 0,
            row: 0
        }
        .findings(&reports[0])
        .is_none()
    );
}
