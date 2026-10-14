// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::workbench::fixture::fixtures;

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(name)).unwrap()
}

#[test]
fn the_digest_is_sha256() {
    assert_eq!(
        sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(sha256(b"").len(), 64);
}

#[test]
fn an_entry_is_judged_as_the_validator_judges_it_and_cached() {
    let bytes = read("base-001-request-id-boolean.jsonl");
    let (entry, stale) = Entry::load("b", &bytes, Meta::default(), None, None);
    assert!(stale, "a sidecar without a cache is recomputed");
    assert_eq!(entry.verdict(), "fail");
    let text = String::from_utf8(bytes.clone()).unwrap();
    let direct = judge::judge(&text).unwrap();
    assert_eq!(entry.reports, direct.reports);
    assert_eq!(entry.findings(), 2);
    let cache = entry.meta.verdict_cache.clone().unwrap();
    assert_eq!(cache.sha256, sha256(&bytes));
    assert_eq!(cache.tool_version, tool_version());

    // The cache is read back instead of judging again …
    let (again, stale) = Entry::load("b", &bytes, entry.meta.clone(), None, None);
    assert!(!stale);
    assert_eq!(again.reports, entry.reports);
    // … and a cache of other bytes is not.
    let mut tampered = entry.meta.clone();
    if let Some(cache) = &mut tampered.verdict_cache {
        cache.reports.clear();
    }
    let (from_cache, _) = Entry::load("b", &bytes, tampered.clone(), None, None);
    assert!(
        from_cache.reports.is_empty(),
        "a fresh cache is trusted as written"
    );
    let mut other = tampered;
    if let Some(cache) = &mut other.verdict_cache {
        cache.sha256 = "0".repeat(64);
    }
    let (rejudged, stale) = Entry::load("b", &bytes, other, None, None);
    assert!(stale);
    assert_eq!(rejudged.reports, entry.reports);
}

#[test]
fn an_unjudgeable_trace_is_unjudged_with_the_reason() {
    let (entry, _) = Entry::load("x", b"not a trace\n", Meta::default(), None, None);
    assert_eq!(entry.verdict(), UNJUDGED);
    assert!(
        entry
            .unjudged
            .as_deref()
            .unwrap()
            .starts_with("malformed trace")
    );
    assert_eq!(entry.findings(), 0);
    assert_eq!(entry.facts.transport, "none");
    let (empty, _) = Entry::load("e", b"", Meta::default(), None, None);
    assert_eq!(empty.verdict(), UNJUDGED);
}

#[test]
fn facts_name_the_implementations_and_the_transport() {
    let (entry, _) = Entry::load(
        "s",
        &read("everything-http.jsonl"),
        Meta::default(),
        None,
        None,
    );
    assert_eq!(entry.facts.server.as_deref(), Some("corpus-server 0.1.0"));
    assert_eq!(entry.facts.client.as_deref(), Some("corpus-client 0.1.0"));
    assert_eq!(entry.facts.transport, "streamable-http");
    assert_eq!(entry.facts.events, 16);
    assert!(entry.facts.messages > 0 && entry.facts.messages < 16);
    let (stdio, _) = Entry::load(
        "t",
        &read("everything-stdio.jsonl"),
        Meta::default(),
        None,
        None,
    );
    assert_eq!(stdio.facts.transport, "stdio");
    let mixed = "{\"seq\":0,\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"lifecycle\",\"event\":\"transport-open\"}\n{\"seq\":1,\"direction\":\"client-to-server\",\"transport\":\"streamable-http\",\"kind\":\"lifecycle\",\"event\":\"transport-open\"}\n";
    let (entry, _) = Entry::load("m", mixed.as_bytes(), Meta::default(), None, None);
    assert_eq!(entry.facts.transport, "mixed");
    assert_eq!(
        implementation(&serde_json::json!({"name": "n"})).as_deref(),
        Some("n")
    );
    assert_eq!(implementation(&serde_json::json!({"version": "1"})), None);
}

#[test]
fn when_prefers_what_the_sidecar_says_then_the_trace_then_the_file() {
    let bytes = read("everything-stdio.jsonl");
    let modified = Some(UNIX_EPOCH + Duration::from_secs(86_400));
    let mut meta = Meta::default();
    let (entry, _) = Entry::load("w", &bytes, meta.clone(), None, modified);
    assert_eq!(entry.when().as_deref(), Some("1970-01-02T00:00:00Z"));
    assert_eq!(entry.day().as_deref(), Some("1970-01-02"));
    meta.added_at = Some("2026-01-02T00:00:00Z".into());
    let (entry, _) = Entry::load("w", &bytes, meta.clone(), None, modified);
    assert_eq!(entry.day().as_deref(), Some("2026-01-02"));
    meta.recorded_at = Some("2025-12-31T23:00:00Z".into());
    let (entry, _) = Entry::load("w", &bytes, meta, None, modified);
    assert_eq!(entry.day().as_deref(), Some("2025-12-31"));
    let (none, _) = Entry::load("w", &bytes, Meta::default(), None, None);
    assert_eq!(none.when(), None);
    assert_eq!(none.day(), None);
}

#[test]
fn the_summary_carries_what_the_list_shows() {
    let mut meta = Meta::default();
    meta.set_tags(["a"]);
    let (entry, _) = Entry::load(
        "s",
        &read("tool-005-name-length.jsonl"),
        meta,
        Some("odd".into()),
        None,
    );
    let summary = entry.summary();
    assert_eq!(summary["id"], "s");
    assert_eq!(summary["verdict"], "pass-with-warnings");
    assert_eq!(summary["revisions"], serde_json::json!(["2025-11-25"]));
    assert_eq!(summary["findings"], 1);
    assert_eq!(summary["tags"], serde_json::json!(["a"]));
    assert_eq!(summary["sidecar_problem"], "odd");
    assert_eq!(summary["bytes"], entry.size);
}

#[test]
fn the_first_events_timestamp_is_a_fact() {
    let timed = "{\"seq\":0,\"ts\":\"2026-01-02T03:04:05Z\",\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"lifecycle\",\"event\":\"transport-open\"}\n{\"seq\":1,\"ts\":\"2026-01-02T03:04:06Z\",\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"lifecycle\",\"event\":\"transport-close\"}\n";
    let (entry, _) = Entry::load("t", timed.as_bytes(), Meta::default(), None, None);
    assert_eq!(
        entry.facts.first_ts.as_deref(),
        Some("2026-01-02T03:04:05Z")
    );
    let (untimed, _) = Entry::load(
        "u",
        &read("everything-stdio.jsonl"),
        Meta::default(),
        None,
        None,
    );
    assert_eq!(untimed.facts.first_ts, None);
}
