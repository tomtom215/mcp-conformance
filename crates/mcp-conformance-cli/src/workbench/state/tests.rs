// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

#![allow(clippy::unwrap_used)]

use std::io::Write as _;

use super::*;
use crate::workbench::entry::complete;
use crate::workbench::fixture;

#[test]
fn opening_judges_every_entry_and_writes_its_cache() {
    let (workbench, dir) = fixture::workbench("open");
    assert_eq!(workbench.entries.len(), 11);
    assert_eq!(workbench.skipped.len(), 0);
    for id in workbench.entries.keys() {
        let meta = workbench.library.read_meta(id).meta;
        assert!(
            meta.verdict_cache.is_some(),
            "{id}: the cache is written back"
        );
    }
    // Reopening reads the caches: the same verdicts.
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
