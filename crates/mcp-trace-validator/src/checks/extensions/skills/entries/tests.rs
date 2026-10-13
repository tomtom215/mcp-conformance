// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the skill-entry checks: the naming rules, the URI grammar, and
//! what a listing is.

use serde_json::{Value, json};

use super::super::super::testkit::{judge, params, request, result};
use super::naming_rule_broken;

fn entry(uri: &str, name: &str) -> Value {
    json!({"uri": uri, "frontmatter": {"name": name, "description": "d"}, "resources": "dynamic"})
}

fn page(id: u64, cursor: Option<&str>, skills: &[Value]) -> [(&'static str, Value); 2] {
    let wanted = cursor.map_or_else(|| json!({}), |cursor| json!({"cursor": cursor}));
    [
        request(id, "skills/list", params(wanted, &[])),
        result(id, json!({"resultType": "complete", "skills": skills})),
    ]
}

#[test]
fn the_agent_skills_naming_rules() {
    for good in ["pdf-processing", "a", "x9", "données", &"a".repeat(64)] {
        assert_eq!(naming_rule_broken(good), None, "{good}");
    }
    for (bad, rule) in [
        ("", "1 to 64"),
        (&*"a".repeat(65), "1 to 64"),
        ("PDF", "lowercase"),
        ("pdf_processing", "lowercase"),
        ("-pdf", "start or end"),
        ("pdf-", "start or end"),
        ("pdf--processing", "consecutive"),
    ] {
        let broken = naming_rule_broken(bad).unwrap_or_default();
        assert!(broken.contains(rule), "{bad:?}: {broken}");
    }
}

#[test]
fn percent_escapes_are_admitted_only_when_well_formed() {
    let judged = |uri: &str| {
        judge(
            "skills.authority-reg-name",
            &page(1, None, &[entry(uri, "x")]),
        )
        .1
    };
    assert_eq!(judged("skill://team%20a/x/SKILL.md"), Vec::<String>::new());
    assert_eq!(judged("skill://team%2/x/SKILL.md").len(), 1);
    assert_eq!(
        judged("skill://team:a/x/SKILL.md").len(),
        1,
        "`:` is no reg-name character"
    );
}

#[test]
fn a_skill_without_a_prefix_has_no_prefix_segment_to_judge() {
    let steps = page(1, None, &[entry("skill://pdf/SKILL.md", "pdf")]);
    assert_eq!(judge("skills.prefix-segments", &steps), (0, vec![]));
    let steps = page(1, None, &[entry("skill://acme/a:b@c/pdf/SKILL.md", "pdf")]);
    assert_eq!(judge("skills.prefix-segments", &steps), (1, vec![]));
}

#[test]
fn a_listing_is_its_first_page_and_the_pages_that_follow() {
    // The same skill on a fresh listing is a re-listing, not a collision.
    let mut steps = page(
        1,
        None,
        &[
            entry("skill://a/pdf/SKILL.md", "pdf"),
            entry("skill://b/SKILL.md", "b"),
        ],
    )
    .to_vec();
    steps.extend(page(
        2,
        None,
        &[
            entry("skill://a/pdf/SKILL.md", "pdf"),
            entry("skill://c/SKILL.md", "c"),
        ],
    ));
    assert_eq!(
        judge("skills.listing-names-unique", &steps).1,
        Vec::<String>::new()
    );
    // Across the pages of one listing, two skills named alike collide.
    let mut steps = page(1, None, &[entry("skill://a/pdf/SKILL.md", "pdf")]).to_vec();
    steps.extend(page(
        2,
        Some("p2"),
        &[entry("skill://b/pdf/SKILL.md", "pdf")],
    ));
    assert_eq!(judge("skills.listing-names-unique", &steps).1.len(), 1);
}

#[test]
fn a_dynamic_skill_has_no_manifest_to_judge() {
    let steps = page(1, None, &[entry("skill://pdf/SKILL.md", "pdf")]);
    for check in [
        "skills.manifest-lists-skill-md",
        "skills.manifest-within-skill",
        "skills.manifest-entry-size",
        "skills.within-limits",
    ] {
        assert_eq!(judge(check, &steps), (0, vec![]), "{check}");
    }
    assert_eq!(judge("skills.entry-resources-form", &steps), (1, vec![]));
}
