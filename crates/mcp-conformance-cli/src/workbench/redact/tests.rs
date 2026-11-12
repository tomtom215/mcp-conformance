// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A redacted copy must hold no byte of a secret it was told to remove —
//! checked against the fixture recording that carries four, and rule by rule.

#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::*;
use crate::workbench::fixture;

const SECRETS: [&str; 4] = [
    "fixture-not-a-real-token-1",
    "fixture-cookie-2",
    "fixture-api-key-3",
    "fixture-meta-4",
];

fn leaky() -> String {
    std::fs::read_to_string(fixture::fixtures().join("leaky-session.jsonl")).unwrap()
}

fn defaults() -> Vec<String> {
    DEFAULT_HEADERS.iter().map(|h| (*h).to_owned()).collect()
}

#[test]
fn the_redacted_copy_holds_no_secret_byte() {
    let text = leaky();
    for secret in SECRETS {
        assert!(text.contains(secret), "the fixture carries {secret}");
    }
    let rules = Rules {
        headers: defaults(),
        pointers: vec!["/params/arguments/apiKey".into()],
        meta: true,
        strings: vec!["fixture-api-key-3".into()],
    };
    let redacted = redact(&text, &rules).unwrap();
    for secret in SECRETS {
        assert!(!redacted.text.contains(secret), "{secret} survived");
        for line in &redacted.summary {
            assert!(!line.contains(secret), "the summary names {secret}");
        }
    }
    // Still a trace, judged like any other, with the same events.
    let events = mcp_trace_validator::reader::parse_trace(
        &redacted.text,
        &mcp_trace_validator::reader::Limits::default(),
    )
    .unwrap();
    assert_eq!(events.len(), text.lines().count());
    assert_eq!(
        redacted.summary,
        [
            "headers authorization, cookie, set-cookie, proxy-authorization, mcp-session-id: \
             10 values",
            "pointer /params/arguments/apiKey: 1 values",
            "_meta members: 2 values",
            "literal string 1: 1 occurrences",
            "numbered stand-ins ([redacted:N]) keep equal values equal and different values \
             different: 6 distinct",
        ]
    );
    // What no rule named is left as it was.
    assert!(redacted.text.contains("\"q\":\"weather\""));
    // The session id is a credential for the session, so the defaults take
    // it too (its three occurrences: the server's assignment, two requests).
    assert!(
        !redacted.text.contains("fixture-session-7"),
        "the session id survived the default headers"
    );
    assert!(DEFAULT_HEADERS.contains(&"mcp-session-id"));
}

fn line(value: &serde_json::Value) -> String {
    format!("{value}\n")
}

#[test]
fn headers_are_matched_without_regard_to_case_and_only_on_http_events() {
    let http = json!({"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"Authorization":"a","x-key":"b"}});
    let message = json!({"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"headers":{"authorization":"c"}}});
    let rules = Rules {
        headers: vec!["AUTHORIZATION".into(), "X-Key".into()],
        ..Rules::default()
    };
    let redacted = redact(&(line(&http) + &line(&message)), &rules).unwrap();
    assert!(!redacted.text.contains("\"a\"") && !redacted.text.contains("\"b\""));
    assert!(redacted.text.contains("\"c\""), "a payload is not a header");
}

#[test]
fn pointers_reach_into_batches_and_through_wildcards() {
    let message = json!({"seq":0,"direction":"server-to-client","transport":"stdio","kind":"message","payload":[
        {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"s1"},{"type":"text","text":"s2"}]}},
        {"jsonrpc":"2.0","id":2,"result":{"a/b":{"~":"s3"}}}
    ]});
    let rules = Rules {
        pointers: vec![
            "/result/content/*/text".into(),
            "/result/a~1b/~0".into(),
            "/result/content/9".into(),
            "/result/missing/x".into(),
            "/id/x".into(),
        ],
        ..Rules::default()
    };
    let redacted = redact(&line(&message), &rules).unwrap();
    for secret in ["s1", "s2", "s3"] {
        assert!(!redacted.text.contains(secret), "{secret}");
    }
    assert_eq!(
        redacted.summary[0],
        "pointer /result/content/*/text: 2 values"
    );
    assert_eq!(redacted.summary[1], "pointer /result/a~1b/~0: 1 values");
    assert!(
        redacted.summary[2..]
            .iter()
            .all(|line| line.ends_with(": 0 values"))
    );
    let indexed = Rules {
        pointers: vec!["/0".into()],
        ..Rules::default()
    };
    let array = json!({"kind":"message","payload":{"list":["x"]}});
    assert!(
        redact(
            &line(&array),
            &Rules {
                pointers: vec!["/list/0".into()],
                ..indexed
            }
        )
        .unwrap()
        .text
        .contains(REDACTED)
    );
}

#[test]
fn a_wildcard_reaches_every_member_of_an_object() {
    let message = json!({"kind":"message","payload":{"result":{"a":{"k":"s4"},"b":{"k":"s5"},"*":{"k":"s6"}}}});
    let redacted = redact(
        &line(&message),
        &Rules {
            pointers: vec!["/result/*/k".into()],
            ..Rules::default()
        },
    )
    .unwrap();
    for secret in ["s4", "s5", "s6"] {
        assert!(!redacted.text.contains(secret), "{secret}");
    }
    assert_eq!(redacted.summary, ["pointer /result/*/k: 3 values"]);
}

#[test]
fn meta_values_are_redacted_wherever_meta_is() {
    let message = json!({"kind":"message","payload":{"params":{"_meta":{"k":"m1"},"nested":[{"_meta":{"j":{"deep":"m2"}}}],"_meta2":"keep"}}});
    let redacted = redact(
        &line(&message),
        &Rules {
            meta: true,
            ..Rules::default()
        },
    )
    .unwrap();
    assert!(!redacted.text.contains("m1") && !redacted.text.contains("m2"));
    assert!(redacted.text.contains("keep"));
    assert!(
        redacted.text.contains("\"k\"") && redacted.text.contains("\"j\""),
        "names are kept"
    );
}

#[test]
fn literal_strings_go_from_values_and_member_names_alike() {
    let event = json!({"kind":"lifecycle","event":"transport-open","note":"xSECRETx SECRET","SECRET-name":1});
    let redacted = redact(
        &line(&event),
        &Rules {
            strings: vec!["SECRET".into()],
            ..Rules::default()
        },
    )
    .unwrap();
    assert!(!redacted.text.contains("SECRET"));
    assert_eq!(redacted.summary, ["literal string 1: 3 occurrences"]);
}

#[test]
fn bad_rules_and_unreadable_lines_are_refused_whole() {
    let none = redact("{}\n", &Rules::default()).unwrap_err();
    assert!(none.0.contains("no rule"));
    let pointer = redact(
        "{}\n",
        &Rules {
            pointers: vec!["params".into()],
            ..Rules::default()
        },
    )
    .unwrap_err();
    assert!(pointer.0.contains("not a JSON pointer"));
    let empty = redact(
        "{}\n",
        &Rules {
            strings: vec![String::new()],
            ..Rules::default()
        },
    )
    .unwrap_err();
    assert!(empty.0.contains("empty string"));
    let broken = redact(
        "{}\nnot json\n",
        &Rules {
            meta: true,
            ..Rules::default()
        },
    )
    .unwrap_err();
    assert!(broken.0.starts_with("line 2 is not JSON"), "{}", broken.0);
}

/// The verdict word the workbench would show for `text`.
fn verdict_of(text: &str) -> String {
    crate::workbench::entry::Entry::load(
        "t",
        text.as_bytes(),
        crate::workbench::meta::Meta::default(),
        None,
        None,
    )
    .0
    .verdict()
}

/// A session whose client presents a session id (`zzz999`) other than the one
/// the server assigned (`abc123`): TRAN-013 fails.
const SESSION_MISMATCH: &str = concat!(
    r#"{"seq":0,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"accept":"application/json, text/event-stream","content-type":"application/json"}}"#,
    "\n",
    r#"{"seq":1,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"corpus-client","version":"0.1.0"}}}}"#,
    "\n",
    r#"{"seq":2,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":200,"headers":{"content-type":"application/json","mcp-session-id":"abc123"}}"#,
    "\n",
    r#"{"seq":3,"direction":"server-to-client","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"corpus-server","version":"0.1.0"}}}}"#,
    "\n",
    r#"{"seq":4,"direction":"client-to-server","transport":"streamable-http","kind":"http","method":"POST","headers":{"accept":"application/json, text/event-stream","mcp-protocol-version":"2025-11-25","mcp-session-id":"zzz999"}}"#,
    "\n",
    r#"{"seq":5,"direction":"client-to-server","transport":"streamable-http","kind":"message","payload":{"jsonrpc":"2.0","method":"notifications/initialized"}}"#,
    "\n",
    r#"{"seq":6,"direction":"server-to-client","transport":"streamable-http","kind":"http","status":202}"#,
    "\n",
);

/// The values of header `name` in `text`'s http events, in order.
fn header_values(text: &str, name: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|event| event["headers"][name].as_str().map(ToOwned::to_owned))
        .collect()
}

/// A session id is a bearer credential, so it cannot be hashed (a hash of a
/// guessable value is a guess away), but "the client echoed the id the server
/// assigned" is a property the validator judges. A numbered stand-in per
/// distinct value keeps equality and inequality and reveals nothing else.
#[test]
fn distinct_session_ids_keep_distinct_stand_ins_and_equal_ones_stay_equal() {
    let redacted = redact(
        SESSION_MISMATCH,
        &Rules {
            headers: defaults(),
            ..Rules::default()
        },
    )
    .unwrap();
    let ids = header_values(&redacted.text, "mcp-session-id");
    assert_eq!(ids, ["[redacted:1]", "[redacted:2]"]);
    assert!(!redacted.text.contains("abc123") && !redacted.text.contains("zzz999"));

    // The same id twice stays the same stand-in, everywhere it occurs.
    let echoed = SESSION_MISMATCH.replace("zzz999", "abc123");
    let redacted = redact(
        &echoed,
        &Rules {
            headers: defaults(),
            ..Rules::default()
        },
    )
    .unwrap();
    assert_eq!(
        header_values(&redacted.text, "mcp-session-id"),
        ["[redacted:1]", "[redacted:1]"]
    );
}

/// The audit's A13: redaction mapped every session id to one string, so a
/// session with a genuine mismatch (TRAN-013) failed before redaction and
/// passed after it.
#[test]
fn redacting_the_session_ids_does_not_change_a_mismatch_the_validator_finds() {
    assert_eq!(verdict_of(SESSION_MISMATCH), "fail");
    let redacted = redact(
        SESSION_MISMATCH,
        &Rules {
            headers: defaults(),
            ..Rules::default()
        },
    )
    .unwrap();
    assert_eq!(verdict_of(&redacted.text), "fail");
}

/// `--meta` replaced `params._meta.progressToken` and left the same token in
/// the `notifications/progress` it correlates with, so the copy failed a clause
/// the original passed. A token is now one stand-in wherever it occurs.
#[test]
fn a_progress_token_redacted_with_meta_still_matches_its_notifications() {
    let original = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/good/stdio-task-progress.jsonl"),
    )
    .unwrap();
    let before = verdict_of(&original);
    assert_ne!(before, "fail", "the corpus trace conforms");
    let redacted = redact(
        &original,
        &Rules {
            meta: true,
            ..Rules::default()
        },
    )
    .unwrap();
    assert_eq!(verdict_of(&redacted.text), before);

    let tokens: std::collections::BTreeSet<String> = redacted
        .text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .flat_map(|event| {
            let params = event["payload"]["params"].clone();
            [
                params["progressToken"].clone(),
                params["_meta"]["progressToken"].clone(),
            ]
        })
        .filter_map(|token| token.as_str().map(ToOwned::to_owned))
        .collect();
    assert_eq!(tokens.len(), 1, "one token, one stand-in: {tokens:?}");
    assert!(!redacted.text.contains("deploy-1"));
}
