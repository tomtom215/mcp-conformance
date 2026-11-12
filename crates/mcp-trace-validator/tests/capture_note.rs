// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The capture note: what a trace's recorder says it left out. A recorder that
//! forwards a message it cannot hold used to leave a trace whose only durable
//! verdict was `pass`; the note makes the account part of the trace, every
//! format reports it, and `--require-complete-capture` is the opt-in way to make
//! a trace that does not say it is complete fail the run. The verdict itself
//! never moves.

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

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn corpus(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus")
        .join(relative)
}

/// A passing corpus session whose closing event carries `capture` (a JSON
/// object, or none for the trace as the corpus has it), written to a file of
/// its own.
fn trace_noting(name: &str, capture: Option<&str>) -> String {
    let text = std::fs::read_to_string(corpus("good/stdio-full-session.jsonl")).unwrap();
    let last = text.lines().last().unwrap();
    assert!(last.contains("transport-close"), "{last}");
    let closing = capture.map_or_else(
        || last.to_owned(),
        |capture| format!(r#"{},"capture":{capture}}}"#, last.trim_end_matches('}')),
    );
    let body: Vec<&str> = text.lines().collect();
    let path = std::env::temp_dir().join(format!(
        "mcp-trace-validator-capture-note-{name}-{}.jsonl",
        std::process::id()
    ));
    std::fs::write(
        &path,
        format!("{}\n{closing}\n", body[..body.len() - 1].join("\n")),
    )
    .unwrap();
    path.to_str().unwrap().to_owned()
}

const LOSSY: &str = r#"{"unrecorded":{"oversized":2,"over_budget":1}}"#;
const COMPLETE: &str = r#"{"unrecorded":{}}"#;

#[test]
fn a_note_that_reports_a_loss_is_shown_and_the_verdict_still_passes() {
    let trace = trace_noting("lossy", Some(LOSSY));
    let human = run(&["validate", &trace]);
    assert_eq!(human.status.code(), Some(0), "{human:?}");
    let text = stdout(&human);
    assert!(
        text.contains(
            "  NOTE  the recorder reports that the trace does not hold 3 things the session \
             carried.\n        The note is the recorder's own statement and nothing \
             authenticates it; the verdict judges what the trace holds, so it is unchanged.\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("        over_budget: 1 (arrived while the recorder's buffer budget"),
        "{text}"
    );
    assert!(
        text.contains("        oversized: 2 (over the recorder's size limit"),
        "{text}"
    );
    assert!(text.ends_with("verdict: pass\n"), "{text}");
    std::fs::remove_file(&trace).ok();
}

#[test]
fn a_note_that_reports_nothing_left_out_is_quiet_in_text_and_present_in_json() {
    let trace = trace_noting("complete", Some(COMPLETE));
    let text = stdout(&run(&["validate", &trace]));
    assert!(!text.contains("recorder reports"), "{text}");

    let json = run(&["validate", "--format", "json", &trace]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["capture"], serde_json::json!({"unrecorded": {}}));
    assert_eq!(report["verdict"], "pass");
    std::fs::remove_file(&trace).ok();

    // A trace that says nothing has no member at all.
    let bare = trace_noting("bare", None);
    let report: serde_json::Value =
        serde_json::from_str(&stdout(&run(&["validate", "--format", "json", &bare]))).unwrap();
    assert!(report.get("capture").is_none(), "{report}");
    std::fs::remove_file(&bare).ok();
}

#[test]
fn the_json_carries_the_note_as_the_recorder_wrote_it() {
    let trace = trace_noting("json", Some(LOSSY));
    let json = run(&["validate", "--format", "json", &trace]);
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(
        report["capture"],
        serde_json::json!({"unrecorded": {"over_budget": 1, "oversized": 2}})
    );
    std::fs::remove_file(&trace).ok();
}

#[test]
fn the_machine_formats_carry_a_loss_without_failing_anything() {
    let trace = trace_noting("formats", Some(LOSSY));
    let line = "the recorder reports that the trace does not hold 3 things the session \
                carried: over_budget 1, oversized 2";

    let junit = run(&["validate", "--format", "junit", &trace]);
    assert_eq!(junit.status.code(), Some(0), "{junit:?}");
    let xml = stdout(&junit);
    assert!(
        xml.contains(&format!(
            "    <properties><property name=\"capture-unrecorded\" value=\"{line}\"/></properties>\n"
        )),
        "{xml}"
    );
    assert!(xml.contains(r#" failures="0""#), "{xml}");

    let sarif = run(&["validate", "--format", "sarif", &trace]);
    assert_eq!(sarif.status.code(), Some(0), "{sarif:?}");
    let log: serde_json::Value = serde_json::from_str(&stdout(&sarif)).unwrap();
    let invocation = &log["runs"][0]["invocations"][0];
    assert_eq!(
        invocation["executionSuccessful"], true,
        "a note is not a failure"
    );
    let note = &invocation["toolExecutionNotifications"][0];
    assert_eq!(note["level"], "note");
    assert_eq!(note["message"]["text"], line);
    assert_eq!(note["properties"]["kind"], "capture-unrecorded");
    assert_eq!(
        note["properties"]["unrecorded"],
        serde_json::json!({"over_budget": 1, "oversized": 2})
    );

    let html = run(&["validate", "--format", "html", &trace]);
    assert_eq!(html.status.code(), Some(0), "{html:?}");
    let page = stdout(&html);
    assert!(
        page.contains(
            "<aside class=\"note\" role=\"note\"><strong>the recorder reports that the trace \
             does not hold 3 things the session carried.</strong> The note is the recorder"
        ),
        "{page}"
    );
    assert!(
        page.contains(
            "<li><code>oversized</code>: 2 (over the recorder&#39;s size limit, forwarded and \
             not recorded)</li>"
        ),
        "{page}"
    );
    std::fs::remove_file(&trace).ok();

    // A trace whose recorder said nothing was left out adds nothing to any format.
    let complete = trace_noting("formats-complete", Some(COMPLETE));
    for format in ["junit", "sarif", "html"] {
        let output = stdout(&run(&["validate", "--format", format, &complete]));
        assert!(!output.contains("capture-unrecorded"), "{format}: {output}");
        assert!(
            !output.contains("the recorder reports"),
            "{format}: {output}"
        );
    }
    std::fs::remove_file(&complete).ok();
}

#[test]
fn under_two_revisions_the_note_is_written_once() {
    let trace = trace_noting("revisions", Some(LOSSY));
    let both = ["--revision", "2025-11-25", "--revision", "2026-07-28"];
    let text = stdout(&run(&[&["validate", &trace], &both[..]].concat()));
    assert_eq!(
        text.matches("  NOTE  the recorder reports").count(),
        1,
        "{text}"
    );

    let json = run(&[&["validate", "--format", "json", &trace], &both[..]].concat());
    let report: serde_json::Value = serde_json::from_str(&stdout(&json)).unwrap();
    assert_eq!(report["capture"]["unrecorded"]["oversized"], 2, "{report}");
    assert!(report["summaries"][0].get("capture").is_none());

    let sarif = run(&[&["validate", "--format", "sarif", &trace], &both[..]].concat());
    let log: serde_json::Value = serde_json::from_str(&stdout(&sarif)).unwrap();
    let notes = log["runs"][0]["invocations"][0]["toolExecutionNotifications"]
        .as_array()
        .unwrap();
    assert_eq!(
        notes
            .iter()
            .filter(|note| note["properties"]["kind"] == "capture-unrecorded")
            .count(),
        1,
        "{notes:?}"
    );
    std::fs::remove_file(&trace).ok();
}

/// The note is read from a file anyone can write: a kind name with a terminal
/// control sequence in it must not reach the terminal as one.
#[test]
fn a_kind_name_cannot_write_to_the_terminal() {
    let trace = trace_noting("escape", Some(r#"{"unrecorded":{"evil\u001b[2J":1}}"#));
    let output = run(&["validate", &trace]);
    let text = stdout(&output);
    assert!(text.contains("recorder reports"), "{text}");
    assert!(!text.contains('\u{1b}'), "{text:?}");
    assert!(text.contains("\\u{1b}"), "{text}");
    std::fs::remove_file(&trace).ok();
}

/// A note of the wrong shape is a malformed trace, not a complete one.
#[test]
fn a_note_of_the_wrong_shape_is_a_malformed_trace() {
    let trace = trace_noting("malformed", Some(r#"{"unrecorded":{"oversized":"two"}}"#));
    let output = run(&["validate", &trace]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(stderr(&output).contains("line 11"), "{}", stderr(&output));
    std::fs::remove_file(&trace).ok();
}

#[test]
fn by_default_a_trace_that_says_nothing_about_its_capture_is_judged_as_it_always_was() {
    let bare = trace_noting("default-bare", None);
    let output = run(&["validate", &bare]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(!stderr(&output).contains("--require-complete-capture"));
    std::fs::remove_file(&bare).ok();
}

#[test]
fn require_complete_capture_passes_only_a_trace_whose_recorder_says_it_was_complete() {
    let complete = trace_noting("req-complete", Some(COMPLETE));
    let output = run(&["validate", "--require-complete-capture", &complete]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(stderr(&output), "");
    std::fs::remove_file(&complete).ok();

    let bare = trace_noting("req-bare", None);
    let output = run(&["validate", "--require-complete-capture", &bare]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains("--require-complete-capture: the trace carries no capture note"),
        "{message}"
    );
    assert!(
        stdout(&output).ends_with("verdict: pass\n"),
        "the report is still written: {}",
        stdout(&output)
    );
    std::fs::remove_file(&bare).ok();

    let lossy = trace_noting("req-lossy", Some(LOSSY));
    let output = run(&["validate", "--require-complete-capture", &lossy]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(
            "--require-complete-capture: the recorder reports that the trace does not hold 3 \
             things the session carried: over_budget 1, oversized 2"
        ),
        "{message}"
    );
    std::fs::remove_file(&lossy).ok();
}

#[test]
fn require_complete_capture_names_each_trace_that_fails_it_in_a_batch() {
    let complete = trace_noting("batch-complete", Some(COMPLETE));
    let bare = trace_noting("batch-bare", None);
    let lossy = trace_noting("batch-lossy", Some(LOSSY));
    let output = run(&[
        "validate",
        "--require-complete-capture",
        &complete,
        &bare,
        &lossy,
    ]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let message = stderr(&output);
    assert!(
        message.contains(&format!("{bare}: --require-complete-capture")),
        "{message}"
    );
    assert!(
        message.contains(&format!("{lossy}: --require-complete-capture")),
        "{message}"
    );
    assert!(
        !message.contains(&format!("{complete}: --require")),
        "{message}"
    );
    for path in [complete, bare, lossy] {
        std::fs::remove_file(&path).ok();
    }
}

/// A conformance failure keeps exit 1 when the capture is complete, and an
/// incomplete capture takes it to 2: the run could not give the answer that
/// was asked for.
#[test]
fn require_complete_capture_orders_with_the_other_exit_codes() {
    let failing = corpus("violations/base-001-request-id-boolean.jsonl");
    let failing = failing.to_str().unwrap();
    assert_eq!(run(&["validate", failing]).status.code(), Some(1));
    let output = run(&["validate", "--require-complete-capture", failing]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(
        stdout(&output).contains("verdict: fail"),
        "the findings are still reported: {}",
        stdout(&output)
    );
}
