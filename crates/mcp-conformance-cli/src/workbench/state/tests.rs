// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use std::io::Write as _;

use super::*;
use crate::workbench::entry::complete;
use crate::workbench::fixture;

#[test]
fn opening_judges_every_entry_and_writes_no_verdict_to_a_sidecar() {
    let (workbench, dir) = fixture::workbench("open");
    assert_eq!(workbench.entries.len(), 11);
    assert_eq!(workbench.skipped.len(), 0);
    for id in workbench.entries.keys() {
        let sidecar = std::fs::read_to_string(workbench.library.meta_path(id)).unwrap_or_default();
        assert!(
            !sidecar.contains("verdict_cache"),
            "{id}: a verdict is computed from the bytes, never stored"
        );
    }
    // Reopening judges again: the same verdicts.
    let again = Workbench::open(&dir).unwrap();
    for (id, entry) in &workbench.entries {
        assert_eq!(again.entry(id).unwrap().verdict(), entry.verdict(), "{id}");
    }
    assert!(matches!(workbench.entry("nope"), Err(Failure::NotFound(_))));
}

#[test]
fn a_trace_being_written_is_followed_and_its_unfinished_line_waits() {
    let (mut workbench, dir) = fixture::workbench("follow");
    let path = dir.join("everything-stdio.jsonl");
    let before = workbench.events("everything-stdio").unwrap().len();
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    let ping = format!(
        "{{\"seq\":{before},\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"ping\"}}}}"
    );
    // Half a line: not yet an event.
    file.write_all(&ping.as_bytes()[..20]).unwrap();
    file.flush().unwrap();
    workbench.refresh().unwrap();
    assert_eq!(workbench.events("everything-stdio").unwrap().len(), before);
    assert_eq!(
        workbench.entry("everything-stdio").unwrap().verdict(),
        "pass"
    );
    file.write_all(&ping.as_bytes()[20..]).unwrap();
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
    workbench.refresh().unwrap();
    assert_eq!(
        workbench.events("everything-stdio").unwrap().len(),
        before + 1
    );
    assert_eq!(
        workbench.entry("everything-stdio").unwrap().facts.events,
        before + 1
    );
}

#[test]
fn an_unfinished_last_line_is_left_out_and_a_finished_one_kept() {
    assert_eq!(complete(""), "");
    assert_eq!(complete("{}\n"), "{}\n");
    assert_eq!(complete("{}\n{}"), "{}\n{}");
    assert_eq!(complete("{}\n{\"a\""), "{}\n");
    assert_eq!(complete("{\"a\""), "");
}

#[test]
fn files_added_and_removed_outside_are_noticed() {
    let (mut workbench, dir) = fixture::workbench("outside");
    std::fs::copy(dir.join("everything-http.jsonl"), dir.join("copy.jsonl")).unwrap();
    std::fs::remove_file(dir.join("tool-005-name-length.jsonl")).unwrap();
    std::fs::write(dir.join("bad name.jsonl"), "").unwrap();
    workbench.refresh().unwrap();
    assert!(workbench.entries.contains_key("copy"));
    assert!(!workbench.entries.contains_key("tool-005-name-length"));
    assert_eq!(workbench.skipped.len(), 1);
}

#[test]
fn an_entry_is_reloaded_when_its_size_or_time_changes_and_only_then() {
    let (mut workbench, dir) = fixture::workbench("unchanged");
    let id = "everything-stdio";
    let path = dir.join(format!("{id}.jsonl"));
    let tagged = |workbench: &Workbench| workbench.entry(id).unwrap().meta.tags.clone();
    let before = tagged(&workbench);
    // A sidecar changed behind the workbench's back, the trace untouched:
    // the entry is not read again.
    let mut meta = workbench.library.read_meta(id).meta;
    meta.set_tags(["outside"]);
    workbench.library.write_meta(id, &meta).unwrap();
    workbench.refresh().unwrap();
    assert_eq!(tagged(&workbench), before);
    // The same bytes with another modification time: read again.
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(86_400))
        .unwrap();
    workbench.refresh().unwrap();
    assert_eq!(tagged(&workbench), ["outside"]);
    // Another size with the same modification time: read again.
    meta.set_tags(["again"]);
    workbench.library.write_meta(id, &meta).unwrap();
    let mut longer = bytes;
    longer.extend_from_slice(b"\n");
    std::fs::write(&path, &longer).unwrap();
    file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(86_400))
        .unwrap();
    workbench.refresh().unwrap();
    assert_eq!(tagged(&workbench), ["again"]);
}

/// Opening a library judges every trace from its bytes and writes nothing: a
/// bare trace gets no sidecar until something about it is changed. (The sidecar
/// used to be written on open, to cache the verdict, and quoted payload values.)
#[test]
fn opening_a_bare_trace_writes_nothing() {
    let dir = fixture::scratch("bare");
    std::fs::copy(
        fixture::fixtures().join("everything-stdio.jsonl"),
        dir.join("bare.jsonl"),
    )
    .unwrap();
    let workbench = Workbench::open(&dir).unwrap();
    assert!(workbench.entry("bare").is_ok());
    assert!(!workbench.library.meta_path("bare").exists());
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(files.len(), 1, "only the trace itself is there");
}

#[test]
fn a_sidecar_written_by_a_change_is_of_the_current_format() {
    let dir = fixture::scratch("bare-tagged");
    std::fs::copy(
        fixture::fixtures().join("everything-stdio.jsonl"),
        dir.join("bare.jsonl"),
    )
    .unwrap();
    let mut workbench = Workbench::open(&dir).unwrap();
    workbench.set_tags("bare", &["seen".to_owned()]).unwrap();
    let text = std::fs::read_to_string(workbench.library.meta_path("bare")).unwrap();
    let written: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(written["format"], crate::workbench::meta::FORMAT);
    assert_eq!(written["tags"], serde_json::json!(["seen"]));
}

#[test]
fn tags_are_written_to_the_sidecar_and_searchable() {
    let (mut workbench, _dir) = fixture::workbench("tags");
    workbench
        .set_tags(
            "everything-http",
            &["  ci ".into(), "ci".into(), "x".into()],
        )
        .unwrap();
    assert_eq!(
        workbench.library.read_meta("everything-http").meta.tags,
        ["ci", "x"]
    );
    let query = crate::workbench::query::parse("tag:ci").unwrap();
    let hits = crate::workbench::search::search(
        &query,
        workbench
            .entries
            .iter()
            .map(|(id, e)| (id.as_str(), &e.doc)),
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "everything-http");
    let error = workbench
        .set_tags("everything-http", &["a\u{7}".into()])
        .unwrap_err();
    assert!(matches!(error, Failure::Invalid(_)));
    assert!(matches!(
        workbench.set_tags("nope", &[]),
        Err(Failure::NotFound(_))
    ));
}

#[test]
fn an_unreadable_sidecar_is_never_overwritten() {
    let (mut workbench, dir) = fixture::workbench("sidecar");
    std::fs::write(dir.join("everything-http.meta.json"), "{broken").unwrap();
    // Touch the trace so it reloads.
    let trace = std::fs::read(dir.join("everything-http.jsonl")).unwrap();
    std::fs::write(
        dir.join("everything-http.jsonl"),
        [&trace[..], b"\n"].concat(),
    )
    .unwrap();
    workbench.refresh().unwrap();
    let entry = workbench.entry("everything-http").unwrap();
    assert!(entry.meta_problem.is_some());
    assert_eq!(
        std::fs::read_to_string(dir.join("everything-http.meta.json")).unwrap(),
        "{broken"
    );
    assert!(matches!(
        workbench.set_tags("everything-http", &[]),
        Err(Failure::Invalid(_))
    ));
}

#[test]
fn delete_removes_both_files_and_refuses_what_is_not_an_entry() {
    let (mut workbench, dir) = fixture::workbench("delete");
    workbench.delete("everything-http").unwrap();
    assert!(!dir.join("everything-http.jsonl").exists());
    assert!(!dir.join("everything-http.meta.json").exists());
    assert!(matches!(
        workbench.delete("everything-http"),
        Err(Failure::NotFound(_))
    ));
    assert!(matches!(workbench.delete("../x"), Err(Failure::Invalid(_))));
    let id = workbench
        .add("new.jsonl", b"", &Meta::new("imported"))
        .unwrap();
    assert_eq!(workbench.entry(&id).unwrap().verdict(), "unjudged");
    assert_eq!(
        Failure::from(io::Error::other("x")),
        Failure::Io("x".into())
    );
}

/// Viewing a session again costs no parse; a trace whose bytes changed is
/// parsed again; the cache holds the few most recently viewed, and viewing
/// one keeps it.
#[test]
fn events_are_parsed_once_per_bytes_and_the_cache_is_small_and_recent() {
    let (mut workbench, dir) = fixture::workbench("events-cache");
    let id = "everything-stdio";
    let first = workbench.events(id).unwrap();
    let again = workbench.events(id).unwrap();
    assert!(Arc::ptr_eq(&first, &again), "the same parse, not a new one");
    // The trace grows (a recorder appending): another hash, a new parse,
    // and the new line in it.
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join(format!("{id}.jsonl")))
        .unwrap();
    writeln!(
        file,
        "{{\"seq\":{},\"direction\":\"client-to-server\",\"transport\":\"stdio\",\"kind\":\"message\",\"payload\":{{\"jsonrpc\":\"2.0\",\"id\":99,\"method\":\"ping\"}}}}",
        first.len()
    )
    .unwrap();
    workbench.refresh().unwrap();
    let grown = workbench.events(id).unwrap();
    assert!(
        !Arc::ptr_eq(&first, &grown),
        "changed bytes are not served stale"
    );
    assert_eq!(grown.len(), first.len() + 1);
    // Most recently viewed first: viewing `id` after EVENTS_CACHED - 1
    // others keeps it, and one more other then evicts the oldest, not it.
    let others: Vec<String> = workbench
        .entries
        .keys()
        .filter(|other| *other != id)
        .take(EVENTS_CACHED)
        .cloned()
        .collect();
    assert_eq!(others.len(), EVENTS_CACHED);
    let oldest = workbench.events(&others[0]).unwrap();
    for other in &others[1..EVENTS_CACHED - 1] {
        workbench.events(other).unwrap();
    }
    assert!(Arc::ptr_eq(&grown, &workbench.events(id).unwrap()));
    workbench.events(&others[EVENTS_CACHED - 1]).unwrap();
    assert!(
        Arc::ptr_eq(&grown, &workbench.events(id).unwrap()),
        "viewed more recently than the others"
    );
    assert!(
        !Arc::ptr_eq(&oldest, &workbench.events(&others[0]).unwrap()),
        "the least recently viewed was evicted"
    );
    assert!(workbench.events.len() <= EVENTS_CACHED);
}

/// The audit's A12: `fs::read` held a whole file before any limit could
/// refuse it (a 150 MiB line cost 165 MiB, where the validator stopped at 75).
/// A trace over the workbench's limit is listed, fingerprinted by streaming it,
/// and never read into memory or judged here.
#[test]
fn a_trace_over_the_limit_is_listed_unjudged_and_never_read() {
    let (mut workbench, _dir) = fixture::workbench("too-large");
    workbench.max_trace_bytes = 4096;
    let trace: Vec<u8> = std::iter::repeat_n(b'x', 10_000).chain(*b"\n").collect();
    let id = workbench
        .add("huge.jsonl", &trace, &Meta::new("imported"))
        .unwrap();
    let entry = workbench.entry(&id).unwrap();
    assert_eq!(entry.size, 10_001);
    assert_eq!(
        entry.sha256,
        entry::sha256(&trace),
        "fingerprinted by streaming, the same digest"
    );
    let why = entry.unjudged.as_deref().unwrap();
    assert!(
        why.contains("10001 bytes") && why.contains("4096"),
        "says how big it is and what the limit is: {why}"
    );
    assert_eq!(entry.reports.len(), 0);
    assert_eq!(entry.facts.events, 0);
    assert!(
        matches!(workbench.text(&id), Err(Failure::Invalid(_))),
        "its text is not served"
    );
    assert_eq!(workbench.events(&id).unwrap().len(), 0);

    // The others in the library are judged as before.
    assert_eq!(
        workbench.entry("everything-stdio").unwrap().verdict(),
        "pass"
    );
}

/// A refresh is a cheap scan, a load that holds none of the workbench, and an
/// apply: what the request handler runs with the library's lock released for
/// the slow middle.
#[test]
fn a_refresh_scans_loads_off_the_workbench_and_applies() {
    let (mut workbench, dir) = fixture::workbench("split");
    assert!(
        workbench.scan().unwrap().ids.is_empty(),
        "an unchanged library has nothing to load"
    );
    let path = dir.join("everything-stdio.jsonl");
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    file.write_all(b"\n").unwrap();
    file.flush().unwrap();
    let reload = workbench.scan().unwrap();
    let ids: Vec<&str> = reload.ids.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["everything-stdio"]);
    // The slow half takes the list, not the workbench.
    let loaded = Workbench::load_all(&reload, workbench.max_trace_bytes).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].0, "everything-stdio");
    workbench.apply(loaded);
    assert!(
        workbench.scan().unwrap().ids.is_empty(),
        "applied: the entry is current"
    );
    assert!(workbench.entry("everything-stdio").is_ok());
}
