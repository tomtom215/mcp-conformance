// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use std::path::Path;
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
fn an_entry_is_judged_as_the_validator_judges_it() {
    let bytes = read("base-001-request-id-boolean.jsonl");
    let (entry, _) = Entry::load("b", &bytes, Meta::default(), None, None);
    assert_eq!(entry.verdict(), "fail");
    let text = String::from_utf8(bytes).unwrap();
    let direct = judge::judge(&text).unwrap();
    assert_eq!(entry.reports, direct.reports);
    assert_eq!(entry.findings(), 2);
    assert!(
        !entry.meta.to_text().contains("verdict_cache"),
        "no verdict is written to the sidecar"
    );
}

/// A sidecar is a file anyone who can write the library can edit, or hand over
/// in an export; its digest only says which bytes it was written for, not that
/// the verdict in it was computed from them. So a verdict is never read from
/// one: a forged cache, empty or not, with the right digest, changes nothing.
#[test]
fn a_verdict_is_never_read_from_a_sidecar() {
    let failing = read("base-001-request-id-boolean.jsonl");
    let passing = read("everything-stdio.jsonl");
    let forged = |digest: &str, reports: &str| -> Meta {
        serde_json::from_str(&format!(
            r#"{{"verdict_cache":{{"sha256":"{digest}","tool_version":"{}","reports":{reports}}}}}"#,
            env!("CARGO_PKG_VERSION")
        ))
        .unwrap()
    };

    // An empty report vector, the right digest and this build's version.
    let (entry, _) = Entry::load("f", &failing, forged(&sha256(&failing), "[]"), None, None);
    assert_eq!(entry.verdict(), "fail");
    assert_eq!(
        entry.reports.len(),
        1,
        "the report of the bytes, not the forged empty one"
    );

    // A well-formed report transplanted from a passing trace.
    let (donor, _) = Entry::load("p", &passing, Meta::default(), None, None);
    let transplanted = serde_json::to_string(&donor.reports).unwrap();
    let (entry, _) = Entry::load(
        "f",
        &failing,
        forged(&sha256(&failing), &transplanted),
        None,
        None,
    );
    assert_eq!(entry.verdict(), "fail");

    // A file that is not a trace stays unjudged, whatever its sidecar claims.
    let junk = b"this is not a trace\n";
    let (entry, _) = Entry::load("j", junk, forged(&sha256(junk), "[]"), None, None);
    assert_eq!(entry.verdict(), "unjudged");
    assert!(entry.unjudged.is_some());
}

/// A sidecar an earlier build wrote carries the old `verdict_cache`, whose
/// findings quote payload values. Loading drops it and says the sidecar should
/// be written back, so it does not linger on disk.
#[test]
fn a_legacy_cache_is_dropped_and_the_sidecar_marked_for_rewriting() {
    let bytes = read("everything-stdio.jsonl");
    let legacy: Meta = serde_json::from_str(
        r#"{"tags":["keep"],"verdict_cache":{"sha256":"x","tool_version":"0.6.0","reports":[]}}"#,
    )
    .unwrap();
    let (entry, rewrite) = Entry::load("l", &bytes, legacy, None, None);
    assert!(rewrite);
    assert_eq!(entry.meta.tags, ["keep"]);
    assert!(!entry.meta.to_text().contains("verdict_cache"));
    let (_, rewrite_again) = Entry::load("l", &bytes, entry.meta, None, None);
    assert!(!rewrite_again, "nothing to drop the second time");
}

/// With neither reports nor a reason, an entry is not a pass: a verdict is
/// claimed only for something that was judged.
#[test]
fn an_entry_with_nothing_judged_and_no_reason_is_unjudged_not_a_pass() {
    let (mut entry, _) = Entry::load("e", b"", Meta::default(), None, None);
    entry.unjudged = None;
    entry.reports.clear();
    assert_eq!(entry.verdict(), "unjudged");
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

/// At `2026-07-28` there is no `initialize`: the server names itself in
/// `_meta["io.modelcontextprotocol/serverInfo"]` of every result and the client
/// in `_meta["io.modelcontextprotocol/clientInfo"]` of every request, so the
/// library showed neither for such a session (the dogfood audit's finding 3.5).
#[test]
fn a_stateless_session_names_its_server_and_client_from_meta() {
    let draft = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/draft/good");
    let (entry, _) = Entry::load(
        "s",
        &std::fs::read(draft.join("stateless-session.jsonl")).unwrap(),
        Meta::default(),
        None,
        None,
    );
    assert_eq!(entry.facts.server.as_deref(), Some("corpus-server 0.1.0"));
    assert_eq!(entry.facts.client.as_deref(), Some("corpus-client 0.1.0"));

    // The first message that names an implementation names it, whichever era:
    // a legacy `initialize` first, then a request carrying `_meta`, keeps the
    // handshake's names.
    let message = |seq: u32, direction: &str, payload: &str| {
        format!(
            r#"{{"seq":{seq},"direction":"{direction}","transport":"stdio","kind":"message","payload":{payload}}}"#
        )
    };
    let mixed = [
        message(
            0,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"result":{"serverInfo":{"name":"first","version":"1"}}}"#,
        ),
        message(
            1,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":2,"result":{"_meta":{"io.modelcontextprotocol/serverInfo":{"name":"second","version":"2"}}}}"#,
        ),
    ]
    .join("\n");
    let (entry, _) = Entry::load("m", mixed.as_bytes(), Meta::default(), None, None);
    assert_eq!(entry.facts.server.as_deref(), Some("first 1"));

    // A `_meta` name that is not an `Implementation` names nothing, and a
    // server's `_meta` is not a client's.
    let odd = [
        message(
            0,
            "server-to-client",
            r#"{"jsonrpc":"2.0","id":1,"result":{"_meta":{"io.modelcontextprotocol/serverInfo":"nope","io.modelcontextprotocol/clientInfo":{"name":"c","version":"1"}}}}"#,
        ),
        message(
            1,
            "client-to-server",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping","params":{"_meta":{"io.modelcontextprotocol/serverInfo":{"name":"s","version":"1"}}}}"#,
        ),
    ]
    .join("\n");
    let (entry, _) = Entry::load("o", odd.as_bytes(), Meta::default(), None, None);
    assert_eq!(entry.facts.server, None);
    assert_eq!(entry.facts.client, None);
}

#[test]
fn an_implementation_with_an_empty_version_is_its_name_alone() {
    let named =
        |version: &str| implementation(&serde_json::json!({"name": "n", "version": version}));
    assert_eq!(named("1.0").as_deref(), Some("n 1.0"));
    assert_eq!(named("").as_deref(), Some("n"), "no trailing space");
    assert_eq!(
        implementation(&serde_json::json!({"name": "n", "version": 3})).as_deref(),
        Some("n")
    );
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

/// The last `n` bytes of `text` cut off, leaving an unfinished last line.
fn cut(text: &[u8], n: usize) -> Vec<u8> {
    text[..text.len() - n].to_vec()
}

/// A trace whose last line is unfinished may still be being written, so its
/// complete lines are judged and the entry says it is provisional: a verdict
/// that covers only what had been written must not read as a plain pass.
#[test]
fn a_trace_cut_off_mid_line_is_judged_as_far_as_it_goes_and_marked_provisional() {
    let whole = read("everything-stdio.jsonl");
    let (finished, _) = Entry::load("w", &whole, Meta::default(), None, None);
    assert!(finished.provisional.is_none());

    let cut_off = cut(&whole, 15);
    let (entry, _) = Entry::load("c", &cut_off, Meta::default(), None, None);
    assert_ne!(
        entry.verdict(),
        "unjudged",
        "the complete lines were judged"
    );
    let reason = entry.provisional.as_deref().unwrap();
    assert!(reason.contains("incomplete"), "{reason}");
    assert_eq!(entry.summary()["provisional"], reason);
    assert_eq!(finished.summary()["provisional"], serde_json::Value::Null);
}

/// A recording that has a closing event is finished, so an unfinished line
/// after it is damage, not a write in progress: it is judged whole, and a trace
/// the validator cannot read is unjudged, as `mcp-trace-validator` exits 3 on it.
#[test]
fn an_unfinished_line_after_a_closing_event_is_damage_not_a_write_in_progress() {
    let mut closed = read("everything-http.jsonl");
    closed.extend_from_slice(b"\n{\"seq\":999,\"direction\":\"clie");
    let (entry, _) = Entry::load("d", &closed, Meta::default(), None, None);
    assert_eq!(entry.verdict(), "unjudged");
    assert!(entry.unjudged.is_some());
    assert!(entry.provisional.is_none());

    // Without the closing event the same tail is taken as a write in progress.
    let whole = read("everything-http.jsonl");
    let lines: Vec<&str> = std::str::from_utf8(&whole).unwrap().lines().collect();
    let mut open = lines[..lines.len() - 1].join("\n").into_bytes();
    open.extend_from_slice(b"\n{\"seq\":999,\"direction\":\"clie");
    let (entry, _) = Entry::load("o", &open, Meta::default(), None, None);
    assert!(entry.provisional.is_some());
    assert_ne!(entry.verdict(), "unjudged");
}

#[test]
fn a_finished_trace_is_not_provisional_with_or_without_its_final_newline() {
    let bytes = read("everything-stdio.jsonl");
    let mut with_newline = bytes.clone();
    if !with_newline.ends_with(b"\n") {
        with_newline.push(b'\n');
    }
    for text in [bytes, with_newline] {
        let (entry, _) = Entry::load("f", &text, Meta::default(), None, None);
        assert!(entry.provisional.is_none());
    }
}
