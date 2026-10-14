// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tests for the skills extension's serving checks: what the server's
//! declaration decides, and which error answers are judged.

use serde_json::{Value, json};

use super::super::testkit::{error, judge, params, request, result};
use super::SKILLS;

fn discover(settings: Option<Value>, resources: bool) -> [(&'static str, Value); 2] {
    let mut capabilities = json!({});
    if resources {
        capabilities["resources"] = json!({});
    }
    if let Some(settings) = settings {
        capabilities["extensions"] = json!({ SKILLS: settings });
    }
    [
        request(1, "server/discover", params(json!({}), &[])),
        result(
            1,
            json!({"resultType": "complete", "capabilities": capabilities}),
        ),
    ]
}

fn call(id: u64, method: &str, uri: &str) -> (&'static str, Value) {
    request(id, method, params(json!({"uri": uri}), &[]))
}

#[test]
fn without_a_discover_result_the_declaration_is_unknowable() {
    let steps = [
        call(2, "resources/directory/read", "skill://a"),
        error(2, -32601),
    ];
    assert_eq!(judge("skills.directory-read-declared", &steps), (0, vec![]));
    assert_eq!(
        judge("skills.directory-read-implemented", &steps),
        (0, vec![])
    );
}

#[test]
fn directory_reads_are_bound_by_directory_read_alone() {
    let mut steps = discover(Some(json!({})), true).to_vec();
    steps.extend([
        call(2, "resources/directory/read", "skill://a"),
        error(2, -32601),
    ]);
    // Declared without `directoryRead`: the client may not call, the server need not answer.
    assert_eq!(judge("skills.directory-read-declared", &steps).1.len(), 1);
    assert_eq!(
        judge("skills.directory-read-implemented", &steps),
        (0, vec![])
    );

    let mut steps = discover(Some(json!({"directoryRead": true})), true).to_vec();
    steps.extend([
        call(2, "resources/directory/read", "skill://a"),
        error(2, -32601),
    ]);
    assert_eq!(judge("skills.directory-read-declared", &steps), (1, vec![]));
    assert_eq!(
        judge("skills.directory-read-implemented", &steps).1.len(),
        1
    );
}

#[test]
fn an_undeclared_extension_binds_the_server_to_nothing() {
    let mut steps = discover(None, false).to_vec();
    steps.extend([call(2, "skills/list", "x"), error(2, -32601)]);
    assert_eq!(judge("skills.list-implemented", &steps), (0, vec![]));
    assert_eq!(judge("skills.declares-resources", &steps), (0, vec![]));
}

#[test]
fn a_skill_md_uri_refused_with_internal_error_passes_and_another_code_fails() {
    let steps = [
        call(2, "skills/get", "skill://a/SKILL.md"),
        error(2, -32603),
    ];
    assert_eq!(
        judge("skills.get-unknown-invalid-params", &steps),
        (1, vec![])
    );
    let steps = [
        call(2, "skills/get", "skill://a/SKILL.md"),
        error(2, -32002),
    ];
    assert_eq!(
        judge("skills.get-unknown-invalid-params", &steps).1.len(),
        1
    );
    // `Method not found` is SKILL-002's, not this check's.
    let steps = [
        call(2, "skills/get", "skill://a/SKILL.md"),
        error(2, -32601),
    ];
    assert_eq!(
        judge("skills.get-unknown-invalid-params", &steps),
        (0, vec![])
    );
}

#[test]
fn a_directory_learned_from_a_directory_result_must_be_listable() {
    let mut steps = discover(Some(json!({"directoryRead": true})), true).to_vec();
    steps.extend([
        call(2, "resources/directory/read", "skill://a"),
        result(
            2,
            json!({"resultType": "complete", "resources": [
            {"uri": "skill://a/scripts", "name": "scripts", "mimeType": "inode/directory"}]}),
        ),
        call(3, "resources/directory/read", "skill://a/scripts"),
        error(3, -32602),
    ]);
    let (subjects, findings) = judge("skills.directory-read-every-directory", &steps);
    // `skill://a` itself is no skill this session's entries name, so only the
    // child the server called a directory is judged.
    assert_eq!(subjects, 1);
    assert_eq!(findings.len(), 1, "{findings:?}");
}

#[test]
fn a_null_or_false_declaration_declares_nothing() {
    for settings in [Value::Null, json!(false)] {
        let steps = discover(Some(settings), false);
        assert_eq!(judge("skills.declares-resources", &steps), (0, vec![]));
    }
}

#[test]
fn a_null_or_false_resources_capability_is_no_declaration() {
    for resources in [Value::Null, json!(false)] {
        let steps = [
            request(1, "server/discover", params(json!({}), &[])),
            result(
                1,
                json!({"resultType": "complete", "capabilities": {
                    "resources": resources, "extensions": {SKILLS: {}}}}),
            ),
        ];
        assert_eq!(
            judge("skills.declares-resources", &steps),
            (
                1,
                vec![format!(
                    "the server declares `{SKILLS}` without the `resources` capability"
                )]
            )
        );
    }
    let steps = discover(Some(json!({})), true);
    assert_eq!(judge("skills.declares-resources", &steps), (1, vec![]));
}
