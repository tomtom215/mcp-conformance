// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use super::*;
use crate::workbench::fixture;

fn compare(a: &str, b: &str) -> Diff {
    let (workbench, _) = fixture::workbench("diff");
    let (left, right) = (workbench.events(a).unwrap(), workbench.events(b).unwrap());
    diff(
        (workbench.entry(a).unwrap(), &left),
        (workbench.entry(b).unwrap(), &right),
    )
}

fn count(diff: &Diff, status: &str) -> usize {
    diff.rows.iter().filter(|row| row.status == status).count()
}

#[test]
fn two_servers_behind_one_client_align_message_for_message() {
    let diff = compare("sdk-go-2025-11-25", "sdk-rust-2025-11-25");
    assert_eq!((count(&diff, "same"), count(&diff, "changed")), (8, 9));
    assert_eq!((count(&diff, "only-a"), count(&diff, "only-b")), (0, 0));
    assert!(!diff.truncated);
    let clause = diff
        .clauses
        .iter()
        .find(|clause| clause.id == "RES-006")
        .unwrap();
    assert_eq!(
        (clause.a.as_str(), clause.b.as_str()),
        ("pass", "not-observed")
    );
    // A response takes its request's method, and an error aligns with a result.
    let prompts = diff
        .rows
        .iter()
        .find(|row| {
            row.a
                .as_ref()
                .is_some_and(|m| m.kind == "error" && m.method == "prompts/get")
        })
        .unwrap();
    assert_eq!(prompts.b.as_ref().unwrap().kind, "result");
    let (left, right) = prompts.payloads.as_ref().unwrap();
    assert_ne!(left, right);
    assert_eq!(
        (diff.revisions_only_a.len(), diff.revisions_only_b.len()),
        (0, 0)
    );
    assert_eq!(diff.a["id"], "sdk-go-2025-11-25");
}

#[test]
fn a_session_compared_with_itself_is_all_the_same() {
    let diff = compare("everything-stdio", "everything-stdio");
    assert!(
        diff.rows
            .iter()
            .all(|row| row.status == "same" && row.payloads.is_none())
    );
    assert_eq!(diff.clauses.len(), 0);
}

#[test]
fn messages_one_side_lacks_are_only_on_the_other() {
    let diff = compare("everything-stdio", "base-001-request-id-boolean");
    assert!(count(&diff, "only-a") > 0);
    assert!(count(&diff, "only-b") > 0);
    let draft = compare("sdk-rust-2025-11-25", "sdk-rust-2026-07-28");
    assert_eq!(draft.revisions_only_a, ["2025-11-25"]);
    assert_eq!(draft.revisions_only_b, ["2026-07-28"]);
    assert!(draft.clauses.is_empty(), "no revision in common");
}

#[test]
fn a_long_payload_is_cut_at_a_character_boundary() {
    let long = serde_json::json!({ "text": "é".repeat(PAYLOAD_LIMIT) });
    let shown = excerpt(&long);
    assert!(shown.len() <= PAYLOAD_LIMIT);
    assert!(shown.starts_with('{'));
}
