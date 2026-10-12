// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! What `validate` says about a trace it cannot read or parse.
//!
//! The error names the trace whenever there is more than one to tell apart, and
//! the hints after it cover the mistakes the error alone does not explain: a
//! shell that passed a pattern or a directory through, and a trace that is not
//! one.

#![cfg(feature = "cli")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcp-trace-validator"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn good() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/good/stdio-minimal-init.jsonl")
        .to_str()
        .unwrap()
        .to_owned()
}

/// A fresh directory under the system temp dir for one test's files.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcp-tv-inputs-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_malformed_trace_among_several_is_named_in_its_error() {
    let dir = scratch("named");
    let broken = dir.join("broken.jsonl");
    std::fs::write(&broken, "not json\n").unwrap();
    let broken = broken.to_str().unwrap();

    let several = run(&["validate", &good(), broken]);
    assert_eq!(several.status.code(), Some(3), "{several:?}");
    assert!(
        stderr(&several).contains(&format!("error: {broken}: malformed trace: line 1:")),
        "{several:?}"
    );

    // One trace needs no name to tell it apart: the line reads as it always has.
    let alone = run(&["validate", broken]);
    assert!(
        stderr(&alone).starts_with("error: malformed trace: line 1:"),
        "{alone:?}"
    );

    // Bytes that are not UTF-8 are a malformation like any other, and named so.
    let latin1 = dir.join("latin1.jsonl");
    std::fs::write(&latin1, b"\xE9\n").unwrap();
    let latin1 = latin1.to_str().unwrap();
    let encoded = run(&["validate", &good(), latin1]);
    assert!(
        stderr(&encoded).contains(&format!(
            "error: {latin1}: malformed trace: line 1: not valid UTF-8"
        )),
        "{encoded:?}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_trace_over_a_limit_among_several_is_named_and_told_the_flag() {
    let output = run(&["validate", "--max-events", "2", &good(), &good()]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let text = stderr(&output);
    let named = format!(
        "error: {}: malformed trace: trace exceeds the 2-event limit",
        good()
    );
    assert_eq!(text.matches(&named).count(), 2, "{text}");
    assert!(text.contains("hint: if the recording is sound, re-run with --max-events above 2"));
}

#[test]
fn a_pattern_the_shell_passed_through_is_called_one() {
    let unmatched = run(&["validate", "no-such-dir/*.jsonl"]);
    assert_eq!(unmatched.status.code(), Some(2), "{unmatched:?}");
    assert!(
        stderr(&unmatched).contains("hint: no file matched this pattern"),
        "{unmatched:?}"
    );
    // A plain missing file is just missing: no talk of patterns.
    let missing = run(&["validate", "no-such-dir/trace.jsonl"]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(!stderr(&missing).contains("hint:"), "{missing:?}");
}

#[test]
fn a_directory_is_answered_with_the_pattern_for_its_traces() {
    let dir = scratch("directory");
    let output = run(&["validate", dir.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let expected = format!(
        "hint: {} is a directory; pass the traces in it, e.g. `{}`",
        dir.display(),
        dir.join("*.jsonl").display()
    );
    assert!(stderr(&output).contains(&expected), "{output:?}");
}

#[test]
fn a_trailing_blank_line_and_a_bare_message_are_named_as_such() {
    let dir = scratch("shapes");
    let trailing = dir.join("trailing.jsonl");
    let mut text = std::fs::read_to_string(good()).unwrap();
    text.push('\n');
    std::fs::write(&trailing, text).unwrap();
    let output = run(&["validate", trailing.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        stderr(&output).contains("a trailing blank line at the end of the file"),
        "{output:?}"
    );

    let bare = dir.join("bare.jsonl");
    std::fs::write(
        &bare,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    )
    .unwrap();
    let output = run(&["validate", bare.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        stderr(&output).contains("hint: this is a bare JSON-RPC message"),
        "{output:?}"
    );
}
