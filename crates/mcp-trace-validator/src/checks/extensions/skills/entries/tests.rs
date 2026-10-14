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
    assert_eq!(judged("skill://team%G0/x/SKILL.md").len(), 1);
    assert_eq!(judged("skill://team%0G/x/SKILL.md").len(), 1);
    assert_eq!(judged("skill://team%/x/SKILL.md").len(), 1);
    // Every byte is judged, before and after an escape.
    assert_eq!(
        judged("skill://a%20b~c-d.e_f!$&'()*+,;=/x/SKILL.md"),
        Vec::<String>::new()
    );
    assert_eq!(judged("skill://%41%42\"/x/SKILL.md").len(), 1);
    assert_eq!(judged("skill://\"%41/x/SKILL.md").len(), 1);
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
fn two_listings_may_name_different_skills_alike() {
    // Each listing alone is unique; only merged would `pdf` collide.
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
            entry("skill://c/pdf/SKILL.md", "pdf"),
            entry("skill://d/SKILL.md", "d"),
        ],
    ));
    assert_eq!(
        judge("skills.listing-names-unique", &steps),
        (4, Vec::<String>::new())
    );
}

#[test]
fn one_skill_repeated_within_a_listing_is_no_collision() {
    let mut steps = page(1, None, &[entry("skill://a/pdf/SKILL.md", "pdf")]).to_vec();
    steps.extend(page(
        2,
        Some("p2"),
        &[entry("skill://a/pdf/SKILL.md", "pdf")],
    ));
    assert_eq!(
        judge("skills.listing-names-unique", &steps),
        (2, Vec::<String>::new())
    );
}

#[test]
fn a_skill_md_has_a_directory_before_it() {
    assert!(super::is_skill_md("skill://pdf/SKILL.md"));
    assert!(super::is_skill_md("x/SKILL.md"));
    assert!(!super::is_skill_md("/SKILL.md"));
    assert!(!super::is_skill_md("skill://pdf/README.md"));
}

fn manifested(files: &[Value]) -> [(&'static str, Value); 2] {
    page(
        1,
        None,
        &[
            json!({"uri": "skill://pdf/SKILL.md", "frontmatter": {"name": "pdf", "description": "d"},
                 "resources": files}),
        ],
    )
}

#[test]
fn a_manifest_listing_a_file_twice_names_it_and_its_count() {
    let file = |name: &str| json!({"uri": format!("skill://pdf/{name}"), "size": 1});
    let steps = manifested(&[
        file("SKILL.md"),
        file("a.txt"),
        file("a.txt"),
        file("b.txt"),
    ]);
    assert_eq!(
        judge("skills.manifest-lists-skill-md", &steps),
        (
            1,
            vec![
                "the manifest of \"skill://pdf/SKILL.md\" lists \"skill://pdf/a.txt\" 2 times, not once"
                    .to_owned()
            ]
        )
    );
    let steps = manifested(&[file("SKILL.md"), file("a.txt")]);
    assert_eq!(
        judge("skills.manifest-lists-skill-md", &steps),
        (1, Vec::<String>::new())
    );
}

#[test]
fn the_limits_are_512_files_and_16_mib_inclusive() {
    let files = |count: usize, size: u64| -> Vec<Value> {
        (0..count)
            .map(|index| json!({"uri": format!("skill://pdf/f{index}"), "size": size}))
            .collect()
    };
    let findings = |files: &[Value]| judge("skills.within-limits", &manifested(files));
    assert_eq!(findings(&files(512, 0)), (1, Vec::<String>::new()));
    assert_eq!(
        findings(&files(513, 0)),
        (
            1,
            vec![
                "the skill \"skill://pdf/SKILL.md\" lists 513 files totalling 0 bytes, beyond the \
                 limits of 512 files and 16777216 bytes"
                    .to_owned()
            ]
        )
    );
    assert_eq!(findings(&files(1, 16_777_216)), (1, Vec::<String>::new()));
    assert_eq!(findings(&files(2, 8_388_609)).1.len(), 1);
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
