// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the embedded registry loader.

#![allow(clippy::unwrap_used)]

/// Every area document on disk is embedded in the slice for its revision.
///
/// `include_str!` catches the other direction — naming a file that is not
/// there fails to compile — but adding a file and forgetting the `const` is
/// silent, and silently *smaller*: the clauses simply are not judged, the
/// generated coverage table regenerates to the lower number, and
/// `spec-drift` verifies only quotes that are in the registry. Nothing
/// downstream can tell an area that was never entered from one that was
/// entered and dropped, which makes this the one hazard in extraction that
/// understates the work rather than overstating it.
///
/// An area is recognized by carrying a `requirements` member rather than by
/// its name, so `sources.json` — the spec-source manifest that lives beside
/// the areas — is excluded by what it is instead of by a second hand-kept
/// list.
#[test]
fn every_registry_area_document_is_embedded() {
    for (revision, embedded) in [
        ("2025-11-25", super::AREAS_2025_11_25),
        #[cfg(feature = "draft-2026-07-28")]
        ("2026-07-28", super::AREAS_2026_07_28),
    ] {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("registry")
            .join(revision);
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));
        let mut areas = 0_usize;
        for entry in entries {
            let Ok(entry) = entry else {
                panic!("cannot read an entry of {}", directory.display())
            };
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                panic!("cannot read {}", path.display())
            };
            if !text.contains("\"requirements\"") {
                continue;
            }
            areas += 1;
            assert!(
                embedded.contains(&text.as_str()),
                "{} is a {revision} area document that no `include_str!` embeds",
                path.display()
            );
        }
        assert_eq!(
            areas,
            embedded.len(),
            "{revision}: {areas} area documents on disk, {} embedded",
            embedded.len()
        );
    }
}

use super::*;

#[test]
fn builtin_registry_parses_and_validates() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    assert_eq!(registry.revision(), REVISION_2025_11_25);
    // The exact entry count, not a floor: a registry document silently
    // dropped from the embed (a forgotten include, a bad merge) shrinks
    // the count without failing any floor check. The README coverage
    // gate pins the same number from the other direction.
    assert_eq!(registry.requirements().len(), 142);
    assert!(registry.get("LIFE-001").is_some());
    assert!(registry.get("NOPE-999").is_none());
}

#[test]
fn builtin_registry_quotes_are_normative() {
    // Every entry's quote must contain the keyword its level claims — a cheap
    // tripwire against paraphrased (non-verbatim) quotes sneaking in.
    let registry = Registry::builtin_2025_11_25().unwrap();
    for requirement in registry.requirements() {
        let quote = &requirement.source.quote;
        let keyword = requirement.level.keyword();
        assert!(
            quote.contains(keyword),
            "{}: quote lacks its level keyword {keyword}: {quote}",
            requirement.id
        );
    }
}

#[test]
fn builtin_registry_areas_merge_in_report_order() {
    // Requirements arrive grouped by area, ordinals ascending within each group —
    // the order reports render in, pinned against accidental file reshuffling.
    let registry = Registry::builtin_2025_11_25().unwrap();
    let ids: Vec<&str> = registry
        .requirements()
        .iter()
        .map(|requirement| requirement.id.as_str())
        .collect();
    let mut sorted_by_area_order = ids.clone();
    let area_rank = |id: &str| {
        AREA_ORDER
            .iter()
            .position(|area| id.starts_with(area))
            .unwrap_or(usize::MAX)
    };
    sorted_by_area_order.sort_by(|a, b| area_rank(a).cmp(&area_rank(b)).then(a.cmp(b)));
    assert_eq!(ids, sorted_by_area_order);
    assert!(ids.contains(&"BASE-001"));
    assert!(ids.contains(&"TRAN-001"));
}

/// Area prefixes in their report order, for the order test above.
const AREA_ORDER: &[&str] = &[
    "BASE-", "LIFE-", "TRAN-", "TOOL-", "RES-", "PROM-", "LOG-", "COMP-", "PAGE-",
];

#[test]
fn rejects_duplicate_ids() {
    let json = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic#x", "quote": "MUST x"},
             "checks": ["a"]},
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic#y", "quote": "MUST y"},
             "checks": ["b"]}
        ]
    }"#;
    assert!(matches!(
        Registry::from_json(json),
        Err(RegistryError::Invalid(reason)) if reason.contains("duplicate")
    ));
}

#[test]
fn rejects_empty_checks_and_exclusions() {
    let empty_checks = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic#x", "quote": "MUST x"},
             "checks": []}
        ]
    }"#;
    assert!(matches!(
        Registry::from_json(empty_checks),
        Err(RegistryError::Invalid(reason)) if reason.contains("empty")
    ));

    let empty_exclusion = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "BASE-001", "level": "MUST", "actor": "both",
             "source": {"section": "basic#x", "quote": "MUST x"},
             "exclusion": "  "}
        ]
    }"#;
    assert!(matches!(
        Registry::from_json(empty_exclusion),
        Err(RegistryError::Invalid(reason)) if reason.contains("exclusion")
    ));
}

#[test]
fn rejects_malformed_capability_gates_at_parse_time() {
    let json = r#"{
        "revision": "2025-11-25",
        "requirements": [
            {"id": "TOOL-001", "level": "MUST", "actor": "server",
             "capability": "tools",
             "source": {"section": "server/tools#x", "quote": "MUST t"},
             "checks": ["a"]}
        ]
    }"#;
    assert!(matches!(
        Registry::from_json(json),
        Err(RegistryError::Parse(_))
    ));
}

#[test]
fn verification_serde_round_trips_both_arms() {
    let registry = Registry::builtin_2025_11_25().unwrap();
    let json = serde_json::to_string(&registry).unwrap();
    let back = Registry::from_json(&json).unwrap();
    assert_eq!(back, registry);
    assert!(
        registry
            .requirements()
            .iter()
            .any(|r| matches!(r.verification, Verification::Excluded { .. }))
    );
    assert!(
        registry
            .requirements()
            .iter()
            .any(|r| matches!(r.verification, Verification::Checks { .. }))
    );
}

#[test]
fn error_display_and_source_carry_real_information() {
    use core::error::Error as _;

    let parse_error = Registry::from_json("{").unwrap_err();
    assert!(
        parse_error.to_string().contains("not valid"),
        "{parse_error}"
    );
    assert!(parse_error.source().is_some());

    let invalid = Registry::from_json(
        r#"{"revision":"2025-11-25","requirements":[
            {"id":"BASE-001","level":"MUST","actor":"both",
             "source":{"section":"basic#x","quote":"MUST x"},"checks":[]}]}"#,
    )
    .unwrap_err();
    assert!(invalid.to_string().contains("invariant"), "{invalid}");
    assert!(invalid.source().is_none());
}
