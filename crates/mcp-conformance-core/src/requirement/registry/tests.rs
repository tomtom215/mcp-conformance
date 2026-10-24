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
        ("2026-07-28", super::AREAS_2026_07_28),
        ("2026-07-28/extensions", super::EXTENSIONS_2026_07_28),
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
    assert_eq!(registry.requirements().len(), 321);
    assert!(registry.get("LIFE-001").is_some());
    assert!(registry.get("NOPE-999").is_none());
}

#[test]
fn builtin_registry_quotes_are_normative() {
    // Every entry's quote must contain the keyword its level claims — a cheap
    // tripwire against paraphrased (non-verbatim) quotes sneaking in. RFC 2119
    // gives three levels a synonym ("REQUIRED" and "SHALL" mean MUST, "SHALL
    // NOT" means MUST NOT, "RECOMMENDED" means SHOULD, "NOT RECOMMENDED" means
    // SHOULD NOT, "OPTIONAL" means MAY), and a clause written with one is
    // entered at the level it means, so either spelling satisfies the check.
    use crate::requirement::Level;
    let synonyms = |level: Level| -> &'static [&'static str] {
        match level {
            Level::Must => &["MUST", "REQUIRED", "SHALL"],
            Level::MustNot => &["MUST NOT", "SHALL NOT"],
            Level::Should => &["SHOULD", "RECOMMENDED"],
            Level::ShouldNot => &["SHOULD NOT", "NOT RECOMMENDED"],
            Level::May => &["MAY", "OPTIONAL"],
        }
    };
    // Every revision's entries, not only `2025-11-25`'s: the set holds them all.
    let set = crate::requirement::RegistrySet::builtin().unwrap();
    for requirement in set.requirements() {
        let quote = &requirement.source.quote;
        let spellings = synonyms(requirement.level);
        assert!(
            spellings.iter().any(|keyword| quote.contains(keyword)),
            "{}: quote lacks its level keyword (any of {spellings:?}): {quote}",
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
    "BASE-", "LIFE-", "TRAN-", "TOOL-", "RES-", "PROM-", "LOG-", "COMP-", "PAGE-", "ELIC-",
    "SAMP-", "ROOT-", "AUTH-",
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

/// The built-in extension clauses (ADR-0023): every one names an extension,
/// none leaks into the core list, and the core list is the size it was before
/// any extension was entered.
#[test]
fn builtin_extension_clauses_stay_apart_from_the_core() {
    let set = crate::requirement::RegistrySet::builtin().unwrap();
    let registry = set.registry("2026-07-28".parse().unwrap()).unwrap();
    assert!(
        registry
            .requirements()
            .iter()
            .all(|requirement| requirement.extension.is_none())
    );
    let mut per_extension: Vec<(&str, usize)> = Vec::new();
    for requirement in registry.extension_requirements() {
        let extension = requirement.extension.as_deref().unwrap();
        match per_extension.iter_mut().find(|(id, _)| *id == extension) {
            Some((_, count)) => *count += 1,
            None => per_extension.push((extension, 1)),
        }
    }
    assert_eq!(
        per_extension,
        [
            ("io.modelcontextprotocol/tasks", 49),
            ("io.modelcontextprotocol/skills", 100)
        ]
    );
    assert!(
        registry.get("TASK-001").is_some(),
        "get finds extension clauses"
    );
    // `2025-11-25` has none: the tasks extension attaches to `2026-07-28` only.
    let old = set.registry("2025-11-25".parse().unwrap()).unwrap();
    assert_eq!(old.extension_requirements().len(), 0);
}

#[test]
fn a_document_lists_extension_clauses_under_either_member() {
    let tagged = r#"{"id": "TASK-001", "level": "MUST", "actor": "server",
        "extension": "io.modelcontextprotocol/tasks",
        "source": {"section": "p#a", "quote": "MUST t"}, "checks": ["t"]}"#;
    let core = r#"{"id": "BASE-001", "level": "MUST", "actor": "both",
        "source": {"section": "basic#x", "quote": "MUST x"}, "checks": ["a"]}"#;
    for document in [
        format!(r#"{{"revision": "2026-07-28", "requirements": [{tagged}, {core}]}}"#),
        format!(
            r#"{{"revision": "2026-07-28", "requirements": [{core}], "extensions": [{tagged}]}}"#
        ),
    ] {
        let registry = Registry::from_json(&document).unwrap();
        let core_ids: Vec<&str> = registry
            .requirements()
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        let extension_ids: Vec<&str> = registry
            .extension_requirements()
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            (core_ids, extension_ids),
            (vec!["BASE-001"], vec!["TASK-001"])
        );
        // Serialized, the extension clause moves to its own member.
        let back = serde_json::to_value(&registry).unwrap();
        assert_eq!(back["extensions"][0]["id"], "TASK-001");
        assert_eq!(Registry::from_json(&back.to_string()).unwrap(), registry);
    }
    // A document whose `extensions` holds a core clause is malformed.
    let misplaced =
        format!(r#"{{"revision": "2026-07-28", "requirements": [], "extensions": [{core}]}}"#);
    let error = Registry::from_json(&misplaced).unwrap_err();
    assert!(error.to_string().contains("names no extension"), "{error}");
    // Ids are unique across both lists.
    let twice = format!(
        r#"{{"revision": "2026-07-28", "requirements": [{tagged}], "extensions": [{tagged}]}}"#
    );
    assert!(
        Registry::from_json(&twice)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn rejects_a_malformed_extension_identifier() {
    let document = r#"{"revision": "2026-07-28", "requirements": [
        {"id": "TASK-001", "level": "MUST", "actor": "server", "extension": "tasks",
         "source": {"section": "p#a", "quote": "MUST t"}, "checks": ["t"]}]}"#;
    let error = Registry::from_json(document).unwrap_err();
    assert!(
        error.to_string().contains("not an extension identifier"),
        "{error}"
    );
}

#[test]
fn an_unknown_extension_links_its_bare_section() {
    let requirement: Requirement = serde_json::from_str(
        r#"{"id": "EXT-001", "level": "MUST", "actor": "server", "extension": "com.example/x",
            "source": {"section": "spec#a", "quote": "MUST"}, "checks": ["x"]}"#,
    )
    .unwrap();
    assert_eq!(requirement.url("2026-07-28".parse().unwrap()), "spec#a");
}
