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
            "headers authorization, cookie, set-cookie, proxy-authorization: 7 values",
            "pointer /params/arguments/apiKey: 1 values",
            "_meta members: 2 values",
            "literal string 1: 1 occurrences",
        ]
    );
    // What no rule named is left as it was.
    assert!(redacted.text.contains("\"q\":\"weather\""));
    assert!(redacted.text.contains("fixture-session-7"));
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
