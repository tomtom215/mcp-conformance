// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the `SKILL.md` content checks: reading frontmatter without a YAML
//! parser, and judging an entry's frontmatter only against the bytes it digests.

#![allow(clippy::unwrap_used)]

use serde_json::{Value, json};

use super::super::super::testkit::{judge, params, request, result};
use super::frontmatter;

const URI: &str = "skill://pdf/SKILL.md";

fn sha(text: &str) -> String {
    format!(
        "sha256:{}",
        super::hex(&crate::html::digest::sha256(text.as_bytes()))
    )
}

#[test]
fn frontmatter_keys_and_plain_scalars() {
    let text = "---\nname: pdf\ndescription: \"Fill forms\" \nlicense: MIT # SPDX\nmetadata:\n  author: me\n# a comment\nallowed-tools: [Read]\n---\nbody\n";
    let fields = frontmatter(text).unwrap();
    let get = |key: &str| {
        fields
            .iter()
            .find(|(field, _)| field == key)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(get("name"), Some(Some("pdf".to_owned())));
    assert_eq!(get("description"), Some(Some("Fill forms".to_owned())));
    assert_eq!(get("license"), Some(Some("MIT".to_owned())));
    assert_eq!(
        get("metadata"),
        Some(None),
        "a block value is compared by key alone"
    );
    assert_eq!(get("allowed-tools"), Some(None), "a flow collection too");
    assert_eq!(get("author"), None, "nested keys are not top-level");
    assert_eq!(fields.len(), 5);
}

#[test]
fn text_without_a_closed_frontmatter_block_has_none() {
    assert!(frontmatter("# Title\n").is_none());
    assert!(frontmatter("---\nname: pdf\n").is_none(), "never closed");
    assert!(frontmatter("").is_none());
}

fn session(
    entry_text: &str,
    read_text: &str,
    entry_description: &str,
) -> Vec<(&'static str, Value)> {
    let entry = json!({"uri": URI, "frontmatter": {"name": "pdf", "description": entry_description},
                       "resources": [{"uri": URI, "digest": sha(entry_text), "size": entry_text.len()}]});
    vec![
        request(1, "skills/get", params(json!({"uri": URI}), &[])),
        result(1, json!({"resultType": "complete", "skill": entry})),
        request(2, "resources/read", params(json!({"uri": URI}), &[])),
        result(
            2,
            json!({"resultType": "complete", "contents": [{"uri": URI, "text": read_text}]}),
        ),
    ]
}

#[test]
fn a_differing_frontmatter_is_judged_only_against_the_bytes_the_entry_digests() {
    let file = "---\nname: pdf\ndescription: Fill forms\n---\n";
    // Same bytes, same frontmatter: a pass.
    assert_eq!(
        judge(
            "skills.frontmatter-identical",
            &session(file, file, "Fill forms")
        ),
        (1, vec![])
    );
    // Same bytes, different entry: the violation.
    let (_, findings) = judge(
        "skills.frontmatter-identical",
        &session(file, file, "Other"),
    );
    assert_eq!(findings.len(), 1, "{findings:?}");
    // Other bytes than the entry digests: the skill may have changed since, so nothing is judged.
    let changed = "---\nname: pdf\ndescription: Changed\n---\n";
    assert_eq!(
        judge(
            "skills.frontmatter-identical",
            &session(file, changed, "Fill forms")
        ),
        (0, vec![])
    );
}

#[test]
fn a_skill_md_read_as_a_blob_is_decoded() {
    let steps = [
        request(1, "resources/read", params(json!({"uri": URI}), &[])),
        result(
            1,
            json!({"resultType": "complete", "contents": [{"uri": URI, "blob": "IyBubyBmcm9udG1hdHRlcgo="}]}),
        ),
    ];
    let (subjects, findings) = judge("skills.skill-md-frontmatter", &steps);
    assert_eq!((subjects, findings.len()), (1, 1), "{findings:?}");
}

#[test]
fn a_listed_name_falls_back_to_the_path_without_an_entry() {
    let steps = [
        request(1, "resources/list", params(json!({}), &[])),
        result(
            1,
            json!({"resultType": "complete", "resources": [
            {"uri": URI, "name": "pdf", "mimeType": "text/markdown"},
            {"uri": "skill://pdf/notes.md", "name": "notes", "mimeType": "text/plain"}]}),
        ),
    ];
    assert_eq!(judge("skills.skill-md-resource-name", &steps), (1, vec![]));
    assert_eq!(judge("skills.skill-md-mime-type", &steps), (1, vec![]));
    // Without an entry there is no frontmatter description to compare.
    assert_eq!(
        judge("skills.skill-md-resource-description", &steps),
        (0, vec![])
    );
}
