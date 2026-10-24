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

const DIRECTORY_READ: &str = "resources/directory/read";

/// `base` with `extra`'s members added.
fn with(mut base: Value, extra: &Value) -> Value {
    for (key, value) in extra.as_object().into_iter().flatten() {
        base[key] = value.clone();
    }
    base
}

#[test]
fn a_directory_result_carries_both_caching_hints_or_is_reported() {
    let read = |hints: Value| {
        let answer = with(json!({"resultType": "complete", "resources": []}), &hints);
        [call(2, DIRECTORY_READ, "skill://a"), result(2, answer)]
    };
    let lacking = |hints: &str| {
        vec![format!(
            "the `{DIRECTORY_READ}` result lacks {hints}, which a cacheable result requires"
        )]
    };
    assert_eq!(
        judge(
            "skills.directory-cacheable",
            &read(json!({"ttlMs": 1000, "cacheScope": "public"}))
        ),
        (1, vec![])
    );
    assert_eq!(
        judge(
            "skills.directory-cacheable",
            &read(json!({"cacheScope": "public"}))
        ),
        (1, lacking("`ttlMs`"))
    );
    assert_eq!(
        judge("skills.directory-cacheable", &read(json!({"ttlMs": 1000}))),
        (1, lacking("`cacheScope`"))
    );
    assert_eq!(
        judge("skills.directory-cacheable", &read(json!({}))),
        (1, lacking("`ttlMs` and `cacheScope`"))
    );
}

#[test]
fn a_directory_result_cache_scope_is_public_or_private() {
    let read = |scope: Value| {
        let answer = json!({"resultType": "complete", "resources": [], "ttlMs": 1000,
                            "cacheScope": scope});
        [call(2, DIRECTORY_READ, "skill://a"), result(2, answer)]
    };
    for scope in ["public", "private"] {
        assert_eq!(
            judge("skills.directory-cacheable", &read(json!(scope))),
            (1, vec![]),
            "{scope}"
        );
    }
    for scope in [json!("shared"), json!(1), json!(null)] {
        assert_eq!(
            judge("skills.directory-cacheable", &read(scope.clone())),
            (
                1,
                vec![format!(
                    "the `{DIRECTORY_READ}` result's `cacheScope` is {scope}, neither \"public\" \
                     nor \"private\""
                )]
            )
        );
    }
}

#[test]
fn only_a_complete_directory_result_no_retry_produced_is_a_subject() {
    // The page's own answer for a non-directory has no result to carry hints.
    let steps = [
        call(2, DIRECTORY_READ, "skill://a/SKILL.md"),
        error(2, -32602),
    ];
    assert_eq!(judge("skills.directory-cacheable", &steps), (0, vec![]));
    // A result of another shape is not cacheable; SKILL-065 judges the shape.
    let steps = [
        call(2, DIRECTORY_READ, "skill://a"),
        result(
            2,
            json!({"resultType": "input_required", "inputRequests": {}, "requestState": "s"}),
        ),
    ];
    assert_eq!(judge("skills.directory-cacheable", &steps), (0, vec![]));
    // A retry's result must not be cached (CACH-003), so it needs no hint.
    for marker in [json!({"inputResponses": {}}), json!({"requestState": "s"})] {
        let steps = [
            request(
                2,
                DIRECTORY_READ,
                params(with(json!({"uri": "skill://a"}), &marker), &[]),
            ),
            result(2, json!({"resultType": "complete", "resources": []})),
        ];
        assert_eq!(
            judge("skills.directory-cacheable", &steps),
            (0, vec![]),
            "{marker}"
        );
    }
    // A session that never read a directory has nothing to judge.
    let steps = [
        call(2, "skills/list", "x"),
        result(2, json!({"resultType": "complete", "skills": []})),
    ];
    assert_eq!(judge("skills.directory-cacheable", &steps), (0, vec![]));
}

#[test]
fn every_page_of_a_directory_listing_is_judged() {
    let steps = [
        call(2, DIRECTORY_READ, "skill://a"),
        result(
            2,
            json!({"resultType": "complete", "resources": [], "nextCursor": "p2",
                   "ttlMs": 1000, "cacheScope": "public"}),
        ),
        request(
            3,
            DIRECTORY_READ,
            params(json!({"uri": "skill://a", "cursor": "p2"}), &[]),
        ),
        result(3, json!({"resultType": "complete", "resources": []})),
    ];
    let (subjects, findings) = judge("skills.directory-cacheable", &steps);
    assert_eq!(subjects, 2);
    assert_eq!(findings.len(), 1, "{findings:?}");
}
