// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! A session that completed an `initialize` handshake runs under the revision
//! the server's result names — that is what negotiation means. The validator
//! judges it there, says so when the client proposed something else, and
//! refuses (exit 2) when the negotiated revision is one it cannot judge,
//! rather than judging the session under the proposal it did not run under.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn corpus(relative: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
        .to_str()
        .unwrap()
        .to_owned()
}

/// Client `2025-11-25`, server `2026-07-28`: judgeable, under the server's —
/// where the legacy `initialize` is a request without its `_meta` (BASE-030).
const NEWER: &str = "draft/violations/base-030-legacy-initialize-negotiated-2026-07-28.jsonl";
/// Client `2025-11-25`, server `2024-11-05`: not judgeable by this build.
const OLDER: &str = "good/stdio-server-negotiates-2024-11-05.jsonl";
/// Client `2025-06-18`, server `2025-11-25`: judgeable, under the server's.
const DECLINED: &str = "violations/tran-018-declined-version-header-refused-400.jsonl";

fn note(proposed: &str, negotiated: &str) -> String {
    format!(
        "note: the client proposed protocol revision {proposed}; the server's initialize \
         result negotiated {negotiated}, which the session then ran under, so it is judged \
         there (pass --revision {proposed} to judge it under the proposal instead)\n"
    )
}

#[test]
fn a_session_is_judged_under_the_revision_the_server_negotiated() {
    let output = run(&["validate", &corpus(NEWER)]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.starts_with("MCP trace validation — revision 2026-07-28 (declared by the trace)\n"),
        "{text}"
    );
    // One report, not a side-by-side of the proposal and the negotiation: the
    // `2025-11-25` rules (LIFE-003, for the `initialized` that never came)
    // are not this session's, and the negotiated revision's are.
    assert!(!text.contains("2025-11-25"), "{text}");
    assert!(!text.contains("overall verdict"), "{text}");
    assert!(text.contains("  FAIL  BASE-030 (MUST)\n"), "{text}");
    assert!(text.ends_with("verdict: fail\n"), "{text}");
    assert_eq!(stderr(&output), note("2025-11-25", "2026-07-28"));

    let json = run(&["validate", "--format", "json", &corpus(NEWER)]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["revision"], "2026-07-28");
    assert_eq!(report["revision_source"], "declared");
    assert!(report.get("revisions").is_none(), "{report}");
    assert_eq!(stderr(&json), note("2025-11-25", "2026-07-28"));
}

#[test]
fn a_negotiated_revision_this_build_cannot_judge_is_refused() {
    let output = run(&["validate", &corpus(OLDER)]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(stdout(&output).is_empty(), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(
            "error: the server's initialize result negotiated protocol revision 2024-11-05 \
             (the client proposed 2025-11-25), which no available registry describes \
             (supported: 2025-11-25, 2026-07-28); the session ran under it"
        ),
        "{message}"
    );
    assert!(message.contains("hint: pass --revision"), "{message}");
    // The same refusal from the JSON path: no report is invented.
    let json = run(&["validate", "--format", "json", &corpus(OLDER)]);
    assert_eq!(json.status.code(), Some(2), "{json:?}");
    assert!(stdout(&json).is_empty(), "{json:?}");
}

#[test]
fn the_proposal_is_still_a_question_the_caller_can_ask() {
    // `--revision` names the proposal: judged there, and the mismatch note
    // stays quiet, because the client did state that revision.
    let output = run(&["validate", "--revision", "2025-11-25", &corpus(OLDER)]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("revision 2025-11-25 (requested)"), "{text}");
    assert!(!text.contains("NOTE  this session declares"), "{text}");
    assert!(stderr(&output).is_empty(), "{output:?}");
}

#[test]
fn a_declined_proposal_is_noted_and_the_negotiated_revision_judged_alone() {
    // The client asked for a revision this build does not know and the server
    // declined it: judged under what the server named, with the note.
    let output = run(&["validate", &corpus(DECLINED)]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = stdout(&output);
    assert!(
        text.contains("revision 2025-11-25 (declared by the trace)"),
        "{text}"
    );
    assert!(text.contains("WARN  TRAN-018"), "{text}");
    assert_eq!(stderr(&output), note("2025-06-18", "2025-11-25"));
}

#[test]
fn an_agreed_handshake_draws_no_note() {
    let output = run(&["validate", &corpus("good/stdio-minimal-init.jsonl")]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stderr(&output).is_empty(), "{output:?}");
}

#[test]
fn several_traces_name_the_trace_the_note_is_about() {
    let newer = corpus(NEWER);
    let output = run(&["validate", &corpus("good/stdio-minimal-init.jsonl"), &newer]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        stderr(&output),
        format!(
            "note: {newer}: {}",
            note("2025-11-25", "2026-07-28").trim_start_matches("note: ")
        )
    );
}
