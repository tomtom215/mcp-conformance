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

#[test]
fn an_excerpt_keeps_whole_characters_up_to_the_limit() {
    // `"` + the text + `"`: an ASCII string exactly at the limit is kept whole,
    // and one byte more is cut to the limit.
    let exact = Value::String("x".repeat(PAYLOAD_LIMIT - 2));
    assert_eq!(excerpt(&exact).len(), PAYLOAD_LIMIT);
    assert_eq!(excerpt(&exact), serde_json::to_string(&exact).unwrap());
    let over = Value::String("x".repeat(PAYLOAD_LIMIT));
    assert_eq!(excerpt(&over).len(), PAYLOAD_LIMIT);
    assert_eq!(PAYLOAD_LIMIT, 4096);
    // A two-byte character straddling the limit is left out whole.
    let straddling = Value::String(format!("{}é", "x".repeat(PAYLOAD_LIMIT - 2)));
    let shown = excerpt(&straddling);
    assert_eq!(shown.len(), PAYLOAD_LIMIT - 1);
    assert!(shown.ends_with('x'));
    assert_eq!(excerpt(&Value::Null), "null");
}

fn event(seq: u64, payload: &Value) -> TraceEvent {
    serde_json::from_value(serde_json::json!({
        "seq": seq,
        "direction": "client-to-server",
        "transport": "stdio",
        "kind": "message",
        "payload": payload,
    }))
    .unwrap()
}

#[test]
fn notifications_and_batches_are_named_by_what_they_are() {
    let notification = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let batch =
        serde_json::json!([{"jsonrpc": "2.0", "method": "a"}, {"jsonrpc": "2.0", "method": "b"}]);
    let events = [event(0, &notification), event(1, &batch)];
    let (messages, truncated) = messages(&events);
    assert!(!truncated);
    let shown: Vec<(&str, &str, Option<&str>)> = messages
        .iter()
        .map(|(m, _)| (m.kind, m.method.as_str(), m.id.as_deref()))
        .collect();
    assert_eq!(
        shown,
        [
            ("notification", "notifications/initialized", None),
            ("batch", "batch of 2", None)
        ]
    );
}

#[test]
fn alignment_stops_after_its_limit_and_says_so() {
    let ping = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/ping"});
    let events = |count: u64| (0..count).map(|seq| event(seq, &ping)).collect::<Vec<_>>();
    let at_limit = events(ALIGN_LIMIT as u64);
    let (kept, truncated) = messages(&at_limit);
    assert_eq!((kept.len(), truncated), (ALIGN_LIMIT, false));
    let over = events(ALIGN_LIMIT as u64 + 1);
    let (kept, truncated) = messages(&over);
    assert_eq!((kept.len(), truncated), (ALIGN_LIMIT, true));
}

/// A notification whose method is `key`, for aligning by key alone.
fn keyed(seq: u64, key: char) -> Message {
    Message {
        seq,
        direction: "c2s",
        kind: "notification",
        method: key.to_string(),
        id: None,
    }
}

/// The longest common subsequence of `a` and `b`, by plain recursion over
/// suffixes with a table, written independently of `align`.
fn lcs(a: &[char], b: &[char]) -> usize {
    let mut table = vec![vec![0_usize; b.len() + 1]; a.len() + 1];
    for (i, x) in a.iter().enumerate() {
        for (j, y) in b.iter().enumerate() {
            table[i + 1][j + 1] = if x == y {
                table[i][j] + 1
            } else {
                table[i][j + 1].max(table[i + 1][j])
            };
        }
    }
    table[a.len()][b.len()]
}

#[test]
fn alignment_pairs_a_longest_common_subsequence_in_order() {
    // A fixed linear congruential sequence over a three-letter alphabet:
    // deterministic, and varied enough to exercise every branch of the table.
    let mut state = 12_345_u64;
    let mut next = |len: usize| -> Vec<char> {
        (0..len)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                ['x', 'y', 'z'][usize::try_from(state >> 62).unwrap() % 3]
            })
            .collect()
    };
    let payload = Value::Null;
    let other = Value::Bool(true);
    for round in 0..200 {
        let (a, b) = (next(round % 9), next((round * 7) % 11));
        let left: Vec<(Message, &Value)> = a
            .iter()
            .zip(0..)
            .map(|(&key, seq)| (keyed(seq, key), &payload))
            .collect();
        let right: Vec<(Message, &Value)> = b
            .iter()
            .zip(0..)
            .map(|(&key, seq)| {
                (
                    keyed(seq, key),
                    if seq.is_multiple_of(2) {
                        &payload
                    } else {
                        &other
                    },
                )
            })
            .collect();
        let rows = align(&left, &right);
        let paired = rows.iter().filter(|row| row.a.is_some() && row.b.is_some());
        assert_eq!(paired.clone().count(), lcs(&a, &b), "{a:?} {b:?}");
        for row in paired {
            let (x, y) = (row.a.as_ref().unwrap(), row.b.as_ref().unwrap());
            assert_eq!(x.method, y.method);
            let same = y.seq.is_multiple_of(2);
            assert_eq!(row.status, if same { "same" } else { "changed" });
            assert_eq!(row.payloads.is_some(), !same);
        }
        assert_eq!(rows.len(), a.len() + b.len() - lcs(&a, &b));
        // Each side appears whole and in order.
        let seqs = |side: fn(&Row) -> Option<&Message>| -> Vec<u64> {
            rows.iter().filter_map(side).map(|m| m.seq).collect()
        };
        assert_eq!(
            seqs(|row| row.a.as_ref()),
            (0..a.len() as u64).collect::<Vec<_>>()
        );
        assert_eq!(
            seqs(|row| row.b.as_ref()),
            (0..b.len() as u64).collect::<Vec<_>>()
        );
    }
}

#[test]
fn a_tie_takes_the_right_side_first() {
    let payload = Value::Null;
    let a = [(keyed(0, 'x'), &payload), (keyed(1, 'y'), &payload)];
    let b = [(keyed(0, 'y'), &payload), (keyed(1, 'x'), &payload)];
    let statuses: Vec<&str> = align(&a, &b).iter().map(|row| row.status).collect();
    assert_eq!(statuses, ["only-b", "same", "only-a"]);
    assert_eq!(align(&a, &[]).len(), 2);
    assert_eq!(align(&[], &b).len(), 2);
}

#[test]
fn either_side_past_the_limit_marks_the_diff_truncated() {
    let (workbench, _) = fixture::workbench("truncated");
    let entry = workbench.entry("everything-stdio").unwrap();
    let ping = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/ping"});
    let long: Vec<TraceEvent> = (0..=ALIGN_LIMIT as u64)
        .map(|seq| event(seq, &ping))
        .collect();
    let short = [event(0, &ping)];
    assert!(diff((entry, &long), (entry, &short)).truncated);
    assert!(diff((entry, &short), (entry, &long)).truncated);
    assert!(!diff((entry, &short), (entry, &short)).truncated);
}
